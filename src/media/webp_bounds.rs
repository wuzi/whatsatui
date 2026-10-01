//! Check dimensions inside WebP bitstreams before handing them to the codec.
//! The pinned VP8 decoder allocates before checking ANMF/canvas consistency.

fn chunk(bytes: &[u8], offset: usize) -> Option<(&[u8], &[u8], usize)> {
    let header = bytes.get(offset..offset.checked_add(8)?)?;
    let size = u32::from_le_bytes(header[4..8].try_into().ok()?) as usize;
    let start = offset + 8;
    let end = start.checked_add(size)?;
    Some((
        &header[..4],
        bytes.get(start..end)?,
        end.checked_add(size % 2)?,
    ))
}
fn uint24(bytes: &[u8]) -> Option<u32> {
    Some(u32::from_le_bytes([
        *bytes.first()?,
        *bytes.get(1)?,
        *bytes.get(2)?,
        0,
    ]))
}
fn dimensions(tag: &[u8], data: &[u8]) -> Option<(u32, u32)> {
    match tag {
        b"VP8 " if data.len() >= 10 && data[0] & 1 == 0 && &data[3..6] == b"\x9d\x01\x2a" => {
            Some((
                u32::from(u16::from_le_bytes([data[6], data[7]]) & 0x3fff),
                u32::from(u16::from_le_bytes([data[8], data[9]]) & 0x3fff),
            ))
        }
        b"VP8L" if data.len() >= 5 && data[0] == 0x2f => {
            let bits = u32::from_le_bytes(data[1..5].try_into().ok()?);
            Some(((bits & 0x3fff) + 1, ((bits >> 14) & 0x3fff) + 1))
        }
        _ => None,
    }
}
pub(super) fn canvas(bytes: &[u8]) -> Option<(u32, u32)> {
    let (tag, data, _) = chunk(bytes, 12)?;
    (tag == b"VP8X").then_some(())?;
    Some((uint24(data.get(4..7)?)? + 1, uint24(data.get(7..10)?)? + 1))
}
pub(super) fn frame_is_safe(data: &[u8], canvas: (u32, u32)) -> bool {
    frame_dimensions(data, canvas).is_some()
}
fn frame_dimensions(data: &[u8], canvas: (u32, u32)) -> Option<()> {
    let x = uint24(data.get(..3)?)? * 2;
    let y = uint24(data.get(3..6)?)? * 2;
    let width = uint24(data.get(6..9)?)? + 1;
    let height = uint24(data.get(9..12)?)? + 1;
    if x + width > canvas.0 || y + height > canvas.1 {
        return None;
    }
    let (mut tag, mut payload, next) = chunk(data, 16)?;
    if tag == b"ALPH" {
        (tag, payload, _) = chunk(data, next)?;
        if tag != b"VP8 " {
            return None;
        }
    }
    (dimensions(tag, payload)? == (width, height)).then_some(())
}
/// The still fallback also needs a safe first frame. Do not decode later frames
/// here: a damaged animation may still have a perfectly usable first image.
pub(super) fn first_frame_is_safe(bytes: &[u8], canvas: (u32, u32)) -> bool {
    let mut offset = 12;
    while let Some((tag, data, next)) = chunk(bytes, offset) {
        match tag {
            b"ANMF" => return frame_is_safe(data, canvas),
            b"VP8 " | b"VP8L" => return dimensions(tag, data) == Some(canvas),
            _ => offset = next,
        }
    }
    false
}
