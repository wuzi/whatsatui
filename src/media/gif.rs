//! Decode verified GIF/MP4 bytes with bounded, cancellable local FFmpeg workers.
use super::{
    animation::{Frame, MAX_FRAMES, Preview},
    check_cancel,
};
use image::{DynamicImage, RgbaImage};
use std::{io::Write, path::Path, process::Stdio, time::Duration};
use tokio::{io::AsyncReadExt, process::Command, sync::watch};

const MAX_PIXELS: u64 = 4_194_304;
const MAX_DURATION_MS: u64 = 30_000;
const UNAVAILABLE: &str = "GIF preview unavailable; use Play or Open";

struct Plan {
    width: u32,
    height: u32,
    duration_ms: u64,
}
impl Plan {
    fn from_probe(bytes: &[u8]) -> Result<Self, String> {
        let data: serde_json::Value = serde_json::from_slice(bytes).map_err(|_| UNAVAILABLE)?;
        let stream = data["streams"]
            .as_array()
            .and_then(|s| s.first())
            .ok_or(UNAVAILABLE)?;
        let (mut width, mut height) = (
            stream["width"].as_u64().ok_or(UNAVAILABLE)?,
            stream["height"].as_u64().ok_or(UNAVAILABLE)?,
        );
        if width == 0 || height == 0 || width.saturating_mul(height) > MAX_PIXELS {
            return Err("GIF preview exceeds 4 megapixels; use Play or Open".into());
        }
        let duration = stream["duration"]
            .as_str()
            .and_then(|s| s.parse::<f64>().ok())
            .or_else(|| {
                data["format"]["duration"]
                    .as_str()
                    .and_then(|s| s.parse().ok())
            })
            .ok_or(UNAVAILABLE)?;
        if !duration.is_finite() || duration <= 0.0 || duration > MAX_DURATION_MS as f64 / 1000.0 {
            return Err("GIF preview needs a duration up to 30 seconds; use Play or Open".into());
        }
        if stream["side_data_list"].as_array().is_some_and(|items| {
            items.iter().any(|s| {
                s["rotation"]
                    .as_i64()
                    .is_some_and(|r| matches!(r.rem_euclid(360), 90 | 270))
            })
        }) {
            std::mem::swap(&mut width, &mut height);
        }
        let longest = width.max(height).max(160);
        Ok(Self {
            width: (width * 160 / longest).max(1) as u32,
            height: (height * 160 / longest).max(1) as u32,
            duration_ms: (duration * 1000.0).round().max(1.0) as u64,
        })
    }
    fn frame_bytes(&self) -> usize {
        self.width as usize * self.height as usize * 4
    }
    fn frames(&self, bytes: Vec<u8>) -> Result<Preview, String> {
        let size = self.frame_bytes();
        let count = bytes.len() / size;
        if count == 0 || count > MAX_FRAMES || !bytes.len().is_multiple_of(size) {
            return Err(UNAVAILABLE.into());
        }
        let frames = bytes
            .chunks_exact(size)
            .enumerate()
            .map(|(i, bytes)| Frame {
                image: DynamicImage::ImageRgba8(
                    RgbaImage::from_raw(self.width, self.height, bytes.to_vec())
                        .expect("bounded RGBA frame"),
                ),
                duration_ms: self.duration_ms * (i as u64 + 1) / count as u64
                    - self.duration_ms * i as u64 / count as u64,
            })
            .collect();
        Ok(Preview {
            frames,
            loops: None,
        })
    }
}

fn input(executable: &str, path: &Path, gif_file: bool) -> Command {
    let mut command = Command::new(executable);
    command.args([
        "-v",
        "error",
        "-threads",
        "1",
        "-max_alloc",
        "67108864",
        "-max_pixels",
        "4194304",
        "-max_streams",
        "16",
        "-protocol_whitelist",
        "file",
        "-format_whitelist",
        "mov,gif",
    ]);
    if gif_file {
        command.args(["-ignore_loop", "1"]);
    } else {
        // Never follow external MOV tracks, including local-file references.
        command.args(["-enable_drefs", "0", "-use_absolute_path", "0"]);
    }
    command.arg("-i").arg(path);
    command
}

pub(super) async fn decode(
    bytes: Vec<u8>,
    cancel: watch::Receiver<bool>,
) -> Result<Preview, String> {
    check_cancel(&cancel)?;
    let gif_file = bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a");
    // Own the file in the blocking task so cancelling a queued write cannot
    // recreate an unowned temporary file. FFmpeg receives only verified bytes.
    let file = tokio::task::spawn_blocking(move || {
        let mut file = tempfile::Builder::new()
            .prefix("whatsapp-tui-gif-")
            .tempfile()?;
        file.write_all(&bytes)?;
        Ok::<_, std::io::Error>(file)
    })
    .await
    .map_err(|_| UNAVAILABLE)?
    .map_err(|_| UNAVAILABLE)?;
    let mut probe = input("ffprobe", file.path(), gif_file);
    probe.args([
        "-select_streams",
        "v:0",
        "-show_entries",
        "stream=width,height,duration:stream_side_data=rotation:format=duration",
        "-of",
        "json",
    ]);
    let plan =
        Plan::from_probe(&run(probe, 16 * 1024, cancel.clone(), Duration::from_secs(3)).await?)?;
    let fps = (MAX_FRAMES as f64 * 1000.0 / plan.duration_ms as f64).min(20.0);
    let mut decoder = input("ffmpeg", file.path(), gif_file);
    decoder.args(["-nostdin", "-filter_threads", "1", "-map", "0:v:0", "-an", "-sn", "-dn", "-vf"])
        .arg(format!("setpts=PTS-STARTPTS,fps={fps:.9}:round=up,scale={w}:{h}:force_original_aspect_ratio=decrease,format=rgba,pad={w}:{h}:(ow-iw)/2:(oh-ih)/2:color=black@0,setsar=1", w=plan.width, h=plan.height))
        .arg("-t").arg(format!("{:.3}", plan.duration_ms as f64 / 1000.0))
        .arg("-frames:v").arg(MAX_FRAMES.to_string())
        .args(["-threads", "1", "-fps_mode", "passthrough", "-f", "rawvideo", "-pix_fmt", "rgba", "pipe:1"]);
    let bytes = run(
        decoder,
        plan.frame_bytes() * MAX_FRAMES,
        cancel.clone(),
        Duration::from_secs(8),
    )
    .await?;
    check_cancel(&cancel)?;
    plan.frames(bytes)
}

async fn run(
    mut command: Command,
    limit: usize,
    mut cancel: watch::Receiver<bool>,
    timeout: Duration,
) -> Result<Vec<u8>, String> {
    check_cancel(&cancel)?;
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .env_remove("FFREPORT")
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                "GIF previews need ffmpeg and ffprobe installed"
            } else {
                UNAVAILABLE
            }
        })?;
    let stdout = child.stdout.take().ok_or(UNAVAILABLE)?;
    let result = tokio::select! {
        _ = cancel.changed() => Err("GIF preview canceled".into()),
        result = tokio::time::timeout(timeout, async {
            let mut bytes = Vec::new();
            stdout.take(limit as u64 + 1).read_to_end(&mut bytes).await.map_err(|_| UNAVAILABLE)?;
            if bytes.len() > limit { return Err("GIF preview output exceeds its limit".into()); }
            if !child.wait().await.map_err(|_| UNAVAILABLE)?.success() { return Err(UNAVAILABLE.into()); }
            Ok(bytes)
        }) => result.unwrap_or_else(|_| Err("GIF preview timed out; use Play or Open".into())),
    };
    if result.is_err() {
        // kill() waits/reaps as well; dropping the entire future also kills it.
        let _ = child.kill().await;
    }
    result
}

#[cfg(test)]
#[path = "gif_tests.rs"]
mod tests;
