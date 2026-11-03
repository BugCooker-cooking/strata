//! Small, bounded resolution metadata. Malformed/absent density falls back to
//! 72 dpi; it never changes dimensions, pixels, profiles or orientation.
use std::{
    fs::File,
    io::{BufReader, Read, Seek, SeekFrom},
    path::Path,
};
pub(crate) fn exif(dpi: u32) -> Vec<u8> {
    let mut data = b"II\x2a\0\x08\0\0\0".to_vec();
    data.extend(3u16.to_le_bytes());
    for (tag, kind, value) in [(0x11au16, 5u16, 50u32), (0x11b, 5, 58), (0x128, 3, 2)] {
        data.extend(tag.to_le_bytes());
        data.extend(kind.to_le_bytes());
        data.extend(1u32.to_le_bytes());
        data.extend(value.to_le_bytes());
    }
    data.extend(0u32.to_le_bytes());
    for _ in 0..2 {
        data.extend(dpi.to_le_bytes());
        data.extend(1u32.to_le_bytes());
    }
    data
}
fn valid(value: f64) -> Option<u32> {
    (value.is_finite() && (1.0..=2400.0).contains(&value)).then(|| value.round() as u32)
}
fn exif_resolution(bytes: &[u8]) -> Option<u32> {
    let bytes = bytes.strip_prefix(b"Exif\0\0").unwrap_or(bytes);
    let little = match bytes.get(..2)? {
        b"II" => true,
        b"MM" => false,
        _ => return None,
    };
    let u16at = |at: usize| -> Option<u16> {
        let v = bytes.get(at..at.checked_add(2)?)?.try_into().ok()?;
        Some(if little {
            u16::from_le_bytes(v)
        } else {
            u16::from_be_bytes(v)
        })
    };
    let u32at = |at: usize| -> Option<u32> {
        let v = bytes.get(at..at.checked_add(4)?)?.try_into().ok()?;
        Some(if little {
            u32::from_le_bytes(v)
        } else {
            u32::from_be_bytes(v)
        })
    };
    if u16at(2)? != 42 {
        return None;
    }
    let ifd = u32at(4)? as usize;
    let count = usize::from(u16at(ifd)?).min(256);
    let (mut x, mut y, mut unit) = (None, None, 0);
    for index in 0..count {
        let at = ifd.checked_add(2 + index * 12)?;
        let tag = u16at(at)?;
        if u32at(at + 4)? != 1 {
            continue;
        }
        if tag == 0x128 && u16at(at + 2)? == 3 {
            unit = u16at(at + 8)?;
        }
        if matches!(tag, 0x11a | 0x11b) && u16at(at + 2)? == 5 {
            let offset = u32at(at + 8)? as usize;
            let numerator = u32at(offset)?;
            let denominator = u32at(offset.checked_add(4)?)?;
            if denominator == 0 {
                return None;
            }
            let value = f64::from(numerator) / f64::from(denominator);
            if tag == 0x11a {
                x = Some(value)
            } else {
                y = Some(value)
            }
        }
    }
    let (x, y) = (x?, y?);
    if (x - y).abs() > 0.02 {
        return None;
    }
    valid(
        x * match unit {
            2 => 1.0,
            3 => 2.54,
            _ => return None,
        },
    )
}
pub(crate) fn resolution(path: &Path) -> u32 {
    read_resolution(path).unwrap_or(72)
}
fn read_resolution(path: &Path) -> Option<u32> {
    let mut reader = BufReader::new(File::open(path).ok()?);
    let mut signature = [0; 12];
    reader.read_exact(&mut signature).ok()?;
    reader.rewind().ok()?;
    if signature.starts_with(b"\x89PNG\r\n\x1a\n") {
        let mut decoder = png::Decoder::new(reader);
        decoder.set_limits(png::Limits {
            bytes: 8 * 1024 * 1024,
        });
        let png = decoder.read_info().ok()?;
        let dimensions = png.info().pixel_dims?;
        return if dimensions.unit == png::Unit::Meter && dimensions.xppu == dimensions.yppu {
            valid(f64::from(dimensions.xppu) * 0.0254)
        } else {
            None
        };
    }
    if signature.starts_with(b"II\x2a\0") || signature.starts_with(b"MM\0\x2a") {
        let mut limits = tiff::decoder::Limits::default();
        limits.ifd_value_size = 8 * 1024 * 1024;
        let mut decoder = tiff::decoder::Decoder::new(reader)
            .ok()?
            .with_limits(limits);
        let unit = decoder.get_tag_u32(tiff::tags::Tag::ResolutionUnit).ok()?;
        let value = |value: tiff::decoder::ifd::Value| match value {
            tiff::decoder::ifd::Value::Rational(n, d) if d > 0 => Some(f64::from(n) / f64::from(d)),
            tiff::decoder::ifd::Value::Double(v) => Some(v),
            _ => None,
        };
        let x = value(decoder.get_tag(tiff::tags::Tag::XResolution).ok()?)?;
        let y = value(decoder.get_tag(tiff::tags::Tag::YResolution).ok()?)?;
        return if (x - y).abs() <= 0.02 {
            valid(
                x * match unit {
                    2 => 1.0,
                    3 => 2.54,
                    _ => return None,
                },
            )
        } else {
            None
        };
    }
    if signature.starts_with(b"\xff\xd8") {
        reader.seek(SeekFrom::Start(2)).ok()?;
        let mut jfif = None;
        for _ in 0..256 {
            let mut marker = [0; 2];
            reader.read_exact(&mut marker).ok()?;
            if marker[0] != 255 || matches!(marker[1], 0xda | 0xd9) {
                break;
            }
            let mut length = [0; 2];
            reader.read_exact(&mut length).ok()?;
            let len = usize::from(u16::from_be_bytes(length)).checked_sub(2)?;
            let mut data = vec![0; len];
            reader.read_exact(&mut data).ok()?;
            if marker[1] == 0xe1
                && let Some(dpi) = exif_resolution(&data)
            {
                return Some(dpi);
            }
            if marker[1] == 0xe0 && data.starts_with(b"JFIF\0") && data.len() >= 12 {
                let x = u16::from_be_bytes([data[8], data[9]]);
                let y = u16::from_be_bytes([data[10], data[11]]);
                if x == y {
                    jfif = match data[7] {
                        1 => valid(f64::from(x)),
                        2 => valid(f64::from(x) * 2.54),
                        _ => None,
                    };
                }
            }
        }
        return jfif;
    }
    if &signature[..4] == b"RIFF" && &signature[8..] == b"WEBP" {
        reader.seek(SeekFrom::Start(12)).ok()?;
        let end = u64::from(u32::from_le_bytes(signature[4..8].try_into().ok()?)) + 8;
        for _ in 0..128 {
            let pos = reader.stream_position().ok()?;
            if pos + 8 > end {
                break;
            }
            let mut chunk = [0; 8];
            reader.read_exact(&mut chunk).ok()?;
            let len = u32::from_le_bytes(chunk[4..].try_into().ok()?);
            let next = pos.checked_add(8 + u64::from(len) + u64::from(len % 2))?;
            if next > end {
                return None;
            }
            if &chunk[..4] == b"EXIF" && len <= 4 * 1024 * 1024 {
                let mut data = vec![0; len as usize];
                reader.read_exact(&mut data).ok()?;
                return exif_resolution(&data);
            }
            reader.seek(SeekFrom::Start(next)).ok()?;
        }
    }
    None
}
