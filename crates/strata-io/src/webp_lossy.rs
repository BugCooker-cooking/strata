use crate::project::StorageError;
use std::io::Write;

pub(crate) fn encode(
    out: &mut impl Write,
    rgba: &[u8],
    width: u32,
    height: u32,
    quality: u8,
    icc: &[u8],
    dpi: u32,
) -> Result<(), StorageError> {
    let encoded = webp::Encoder::from_rgba(rgba, width, height)
        .encode_simple(false, f32::from(quality))
        .map_err(|e| StorageError::Invalid(format!("WebP encode failed: {e:?}")))?;
    if encoded.len() < 12 || &encoded[..4] != b"RIFF" || &encoded[8..12] != b"WEBP" {
        return Err(StorageError::Invalid(
            "WebP encoder returned an invalid container".into(),
        ));
    }
    // Add the standard extended header and metadata without changing compressed
    // VP8/alpha chunks. Chunk boundaries are checked before any output is used.
    let mut chunks = Vec::new();
    let mut offset = 12usize;
    let mut flags = 0x20 | 0x08;
    while offset < encoded.len() {
        let header = encoded
            .get(offset..offset + 8)
            .ok_or_else(|| StorageError::Invalid("WebP chunk header".into()))?;
        let len = u32::from_le_bytes(header[4..].try_into().unwrap()) as usize;
        let end = offset
            .checked_add(8)
            .and_then(|v| v.checked_add(len))
            .and_then(|v| v.checked_add(len % 2))
            .filter(|&v| v <= encoded.len())
            .ok_or_else(|| StorageError::Invalid("WebP chunk size".into()))?;
        match &header[..4] {
            b"VP8X" => {
                if len != 10 {
                    return Err(StorageError::Invalid("WebP extended header".into()));
                }
                flags |= encoded[offset + 8] & 0x10;
            }
            b"ICCP" | b"EXIF" => {}
            b"ALPH" => {
                flags |= 0x10;
                chunks.push(&encoded[offset..end]);
            }
            _ => chunks.push(&encoded[offset..end]),
        }
        offset = end;
    }
    let exif = crate::metadata::exif(dpi);
    let size = 4
        + 18
        + 8
        + icc.len()
        + icc.len() % 2
        + 8
        + exif.len()
        + exif.len() % 2
        + chunks.iter().map(|c| c.len()).sum::<usize>();
    let size = u32::try_from(size)
        .map_err(|_| StorageError::Invalid("WebP container exceeds 4 GiB".into()))?;
    out.write_all(b"RIFF")?;
    out.write_all(&size.to_le_bytes())?;
    out.write_all(b"WEBP")?;
    let mut extended = [0u8; 10];
    extended[0] = flags;
    extended[4..7].copy_from_slice(&(width - 1).to_le_bytes()[..3]);
    extended[7..10].copy_from_slice(&(height - 1).to_le_bytes()[..3]);
    chunk(out, b"VP8X", &extended)?;
    chunk(out, b"ICCP", icc)?;
    for bytes in chunks {
        out.write_all(bytes)?;
    }
    chunk(out, b"EXIF", &exif)?;
    Ok(())
}
fn chunk(out: &mut impl Write, kind: &[u8; 4], bytes: &[u8]) -> Result<(), StorageError> {
    out.write_all(kind)?;
    out.write_all(&(bytes.len() as u32).to_le_bytes())?;
    out.write_all(bytes)?;
    if bytes.len() % 2 == 1 {
        out.write_all(&[0])?;
    }
    Ok(())
}
