//! Bounded WebP animation decoding and presentation timing.
use image::{AnimationDecoder, DynamicImage, ImageDecoder, codecs::webp::WebPDecoder};
use std::io::Cursor;
use tokio::sync::watch;

pub const MAX_FRAMES: usize = 96;
const MAX_SOURCE_FRAMES: usize = 1_500;
const MAX_DURATION: u64 = 30_000;

pub struct Frame {
    pub image: DynamicImage,
    pub duration_ms: u64,
}
pub struct Preview {
    pub frames: Vec<Frame>,
    /// None means loop forever; finite counts include the first play.
    pub loops: Option<u32>,
}
impl Preview {
    fn still(image: DynamicImage) -> Self {
        Self {
            frames: vec![Frame {
                image,
                duration_ms: 0,
            }],
            loops: Some(1),
        }
    }
}

/// A timeline skips missed frames after a stalled UI instead of replaying a backlog.
pub struct Timeline {
    ends: Vec<u64>,
    loops: Option<u32>,
}
impl Timeline {
    pub fn new(durations: impl IntoIterator<Item = u64>, loops: Option<u32>) -> Self {
        let mut total = 0u64;
        let ends = durations
            .into_iter()
            .map(|ms| {
                total = total.saturating_add(ms.max(1));
                total
            })
            .collect();
        Self { ends, loops }
    }
    pub fn frame_at(&self, elapsed_ms: u64) -> usize {
        let total = self.ends.last().copied().unwrap_or(1);
        if self
            .loops
            .is_some_and(|n| elapsed_ms >= total.saturating_mul(u64::from(n)))
        {
            return self.ends.len().saturating_sub(1);
        }
        self.ends.partition_point(|end| *end <= elapsed_ms % total)
    }
}

pub fn decode(bytes: &[u8], cancel: &watch::Receiver<bool>) -> Result<Preview, String> {
    super::check_cancel(cancel)?;
    let still = super::preview::decode(bytes)?.thumbnail(640, 640);
    let animation = animated(bytes, cancel);
    // Cancellation must never be turned into a successful still fallback.
    super::check_cancel(cancel)?;
    Ok(animation.unwrap_or_else(|| Preview::still(still)))
}

fn animated(bytes: &[u8], cancel: &watch::Receiver<bool>) -> Option<Preview> {
    let decoder = WebPDecoder::new(Cursor::new(bytes)).ok()?;
    if !decoder.has_animation() {
        return None;
    }
    let (width, height) = decoder.dimensions();
    let pixels = u64::from(width) * u64::from(height);
    if pixels > 1_048_576 {
        return None;
    }
    let delays = delays(bytes)?;
    // Bound total decode work as well as retained memory.
    if pixels.saturating_mul(delays.len() as u64) > 64_000_000 {
        return None;
    }
    let loops = match decoder.loop_count() {
        image::metadata::LoopCount::Infinite => None,
        image::metadata::LoopCount::Finite(n) => Some(n.get()),
    };
    let selected = samples(&delays);
    let mut frames = Vec::with_capacity(selected.len());
    let mut next = 0;
    let mut decoded = 0;
    for (index, frame) in decoder.into_frames().enumerate() {
        super::check_cancel(cancel).ok()?;
        if index >= delays.len() {
            return None;
        }
        let frame = frame.ok()?;
        decoded += 1;
        if next < selected.len() && selected[next].0 == index {
            frames.push(Frame {
                image: DynamicImage::ImageRgba8(frame.into_buffer()).thumbnail(160, 160),
                duration_ms: selected[next].1,
            });
            next += 1;
        }
    }
    if decoded != delays.len() || frames.len() < 2 {
        return None;
    }
    Some(Preview { frames, loops })
}

fn delays(bytes: &[u8]) -> Option<Vec<u64>> {
    if bytes.get(..4)? != b"RIFF" || bytes.get(8..12)? != b"WEBP" {
        return None;
    }
    let canvas = super::webp_bounds::canvas(bytes)?;
    let mut offset = 12usize;
    let mut delays = vec![];
    let mut total = 0;
    while offset < bytes.len() {
        let header = bytes.get(offset..offset.checked_add(8)?)?;
        let size = u32::from_le_bytes(header[4..8].try_into().ok()?) as usize;
        let start = offset + 8;
        let data = bytes.get(start..start.checked_add(size)?)?;
        if &header[..4] == b"ANMF" {
            if data.len() < 16 || delays.len() >= MAX_SOURCE_FRAMES {
                return None;
            }
            if !super::webp_bounds::frame_is_safe(data, canvas) {
                return None;
            }
            let ms = u64::from(u32::from_le_bytes([data[12], data[13], data[14], 0])).max(10);
            total += ms;
            if total > MAX_DURATION {
                return None;
            }
            delays.push(ms);
        }
        offset = start.checked_add(size)?.checked_add(size % 2)?;
    }
    (delays.len() > 1).then_some(delays)
}

/// Sample by elapsed time, coalescing repeated frames. Preserve total duration.
fn samples(delays: &[u64]) -> Vec<(usize, u64)> {
    let total: u64 = delays.iter().sum();
    let step = total.div_ceil(MAX_FRAMES as u64).max(50);
    let mut starts = vec![];
    let mut index = 0;
    let mut end = delays[0];
    for at in (0..total).step_by(step as usize) {
        while at >= end && index + 1 < delays.len() {
            index += 1;
            end += delays[index];
        }
        if starts.last().is_none_or(|&(previous, _)| previous != index) {
            starts.push((index, at));
        }
    }
    // A sub-tick animation should still move, rather than alias to a still.
    if starts.len() == 1 {
        starts.push((delays.len() - 1, total.max(100) / 2));
    }
    let end = total.max(starts.last().unwrap().1 + 50);
    starts
        .iter()
        .enumerate()
        .map(|(i, &(frame, start))| (frame, starts.get(i + 1).map_or(end, |v| v.1) - start))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    const MOVING: &[u8] = include_bytes!("../../tests/fixtures/moving-sticker.webp");
    #[test]
    fn malformed_later_bitstream_is_rejected_before_decode_and_keeps_a_safe_still() {
        fn chunk(tag: &[u8; 4], bytes: &[u8]) -> Vec<u8> {
            let mut c = tag.to_vec();
            c.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
            c.extend_from_slice(bytes);
            if !bytes.len().is_multiple_of(2) {
                c.push(0);
            }
            c
        }
        for alpha in [false, true] {
            // Valid small canvas/first frame, hostile dimensions in the next
            // lossy payload (including the ALPH + VP8 representation).
            let offsets: Vec<_> = MOVING
                .windows(4)
                .enumerate()
                .filter(|(_, v)| *v == b"ANMF")
                .map(|(i, _)| i)
                .collect();
            let mut frame = MOVING[offsets[1] + 8..offsets[1] + 24].to_vec();
            if alpha {
                frame.extend(chunk(b"ALPH", &[0; 1 + 32 * 32]));
            }
            frame.extend(chunk(
                b"VP8 ",
                &[0x10, 0, 0, 0x9d, 0x01, 0x2a, 0xff, 0x3f, 0xff, 0x3f],
            ));
            let mut bytes = MOVING[..offsets[1]].to_vec();
            bytes.extend(chunk(b"ANMF", &frame));
            let length = (bytes.len() - 8) as u32;
            bytes[4..8].copy_from_slice(&length.to_le_bytes());
            // Assert the preflight gate before exercising decode, so RED
            // never makes the huge allocation on the developer's machine.
            assert!(
                delays(&bytes).is_none(),
                "unsafe nested dimensions passed preflight"
            );
            let (_stop, cancel) = watch::channel(false);
            assert_eq!(decode(&bytes, &cancel).unwrap().frames.len(), 1);
        }
    }
    #[test]
    fn composed_frames_keep_colors_delays_and_loop_timing() {
        let (_stop, cancel) = watch::channel(false);
        let preview = decode(MOVING, &cancel).unwrap();
        assert_eq!(preview.frames.len(), 2);
        assert_ne!(
            preview.frames[0].image.to_rgba8(),
            preview.frames[1].image.to_rgba8()
        );
        let durations: Vec<_> = preview.frames.iter().map(|f| f.duration_ms).collect();
        assert_eq!(durations, [100, 200]);
        assert_eq!(preview.loops, None);
        let time = Timeline::new(durations.clone(), None);
        for (at, frame) in [(0, 0), (99, 0), (100, 1), (299, 1), (300, 0), (60_100, 1)] {
            assert_eq!(time.frame_at(at), frame);
        }
        let time = Timeline::new(durations, Some(2));
        assert_eq!(time.frame_at(600), 1);
        assert_eq!(time.frame_at(u64::MAX), 1);
    }
    #[test]
    fn sampling_bounds_memory_without_stretching_long_animations() {
        let frames = samples(&vec![20; 1_000]);
        assert!(frames.len() <= MAX_FRAMES);
        assert_eq!(frames.iter().map(|v| v.1).sum::<u64>(), 20_000);
        assert!(frames.iter().all(|v| v.1 >= 50));
        assert_eq!(samples(&[10, 10]), [(0, 50), (1, 50)]);
    }
    #[test]
    fn excessive_duration_falls_back_and_cancellation_does_not() {
        let (stop, cancel) = watch::channel(false);
        let mut bytes = MOVING.to_vec();
        let offset = bytes.windows(4).position(|v| v == b"ANMF").unwrap() + 8 + 12;
        bytes[offset..offset + 3].copy_from_slice(&[0xff; 3]);
        assert_eq!(decode(&bytes, &cancel).unwrap().frames.len(), 1);
        stop.send(true).unwrap();
        assert!(decode(MOVING, &cancel).is_err());
    }
}
