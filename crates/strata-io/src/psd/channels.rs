use super::*;
use flate2::{Decompress, FlushDecompress, Status};

pub(super) fn decode(
    encoded: &[u8],
    width: u32,
    height: u32,
    format: PixelFormat,
) -> Result<Vec<u8>> {
    let row = width as usize * format.bytes_per_sample();
    let size = row.checked_mul(height as usize).ok_or(Limit)?;
    if size as u64 > crate::DEFAULT_DECODE_LIMIT {
        return Err(Limit);
    }
    let mut src = Reader(encoded);
    match src.short()? {
        0 if src.0.len() == size => Ok(src.0.to_vec()),
        0 => Err(Invalid("raw channel length")),
        1 => {
            let mut counts = Reader(src.take(height as usize * 2)?);
            let mut out = Vec::with_capacity(size);
            while !counts.0.is_empty() {
                let line = src.take(counts.short()? as usize)?;
                unpack_row(line, row, &mut out)?;
            }
            src.padding(0)?;
            Ok(out)
        }
        method @ (2 | 3) => {
            // The extra byte distinguishes an oversized stream from one ending exactly at size.
            let mut out = vec![0; size + 1];
            let mut stream = Decompress::new(true);
            let status = stream
                .decompress(src.0, &mut out, FlushDecompress::Finish)
                .map_err(|_| Invalid("ZIP channel"))?;
            if status != Status::StreamEnd
                || stream.total_in() != src.0.len() as u64
                || stream.total_out() != size as u64
            {
                return Err(Invalid("ZIP channel length"));
            }
            out.truncate(size);
            if method == 3 && row > 0 {
                for line in out.chunks_exact_mut(row) {
                    if format == PixelFormat::Rgba8 {
                        for i in 1..line.len() {
                            line[i] = line[i].wrapping_add(line[i - 1]);
                        }
                    } else {
                        let mut last = 0u16;
                        for pair in line.chunks_exact_mut(2) {
                            last = last.wrapping_add(u16::from_be_bytes([pair[0], pair[1]]));
                            pair.copy_from_slice(&last.to_be_bytes());
                        }
                    }
                }
            }
            Ok(out)
        }
        _ => Err(Unsupported("channel compression".into())),
    }
}
fn unpack_row(line: &[u8], row: usize, out: &mut Vec<u8>) -> Result<()> {
    let end = out.len() + row;
    let mut src = Reader(line);
    while !src.0.is_empty() {
        let header = src.byte()? as i8;
        if header == -128 {
            continue;
        }
        let count = usize::from(header.unsigned_abs()) + 1;
        if count > end - out.len() {
            return Err(Invalid("PackBits row overflow"));
        }
        if header < 0 {
            out.resize(out.len() + count, src.byte()?);
        } else {
            out.extend_from_slice(src.take(count)?);
        }
    }
    if out.len() != end {
        return Err(Invalid("short PackBits row"));
    }
    Ok(())
}
pub(super) fn opaque(width: u32, height: u32, format: PixelFormat) -> Result<PixelBuffer> {
    let count = format.sample_count(width, height).map_err(|_| Limit)?;
    if count as u64 * format.bytes_per_sample() as u64 > crate::DEFAULT_DECODE_LIMIT {
        return Err(Limit);
    }
    Ok(match format {
        PixelFormat::Rgba8 => PixelBuffer::Rgba8(vec![255; count]),
        PixelFormat::Rgba16 => PixelBuffer::Rgba16(vec![65535; count]),
    })
}
pub(super) fn insert(pixels: &mut PixelBuffer, id: usize, plane: &[u8]) {
    match pixels {
        PixelBuffer::Rgba8(data) => {
            for (p, &v) in data.chunks_exact_mut(4).zip(plane) {
                p[id] = v;
            }
        }
        PixelBuffer::Rgba16(data) => {
            for (p, v) in data.chunks_exact_mut(4).zip(plane.chunks_exact(2)) {
                p[id] = u16::from_be_bytes([v[0], v[1]]);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::{Compression, write::ZlibEncoder};
    use std::io::Write;

    #[test]
    fn packbits_exact_row_sizes_literals_runs_and_noops() {
        let channel = [0, 1, 0, 4, 0, 4, 128, 254, 7, 128, 2, 1, 2, 3];
        assert_eq!(
            decode(&channel, 3, 2, PixelFormat::Rgba8).unwrap(),
            [7, 7, 7, 1, 2, 3]
        );
        for width in [2, 4] {
            assert!(decode(&channel, width, 2, PixelFormat::Rgba8).is_err());
        }
        assert!(decode(&channel[..channel.len() - 1], 3, 2, PixelFormat::Rgba8).is_err());
    }
    #[test]
    fn zip_checks_exact_size_end_of_stream_and_predictor_row_reset() {
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
        encoder.write_all(&[240, 30, 20, 10]).unwrap();
        let mut channel = vec![0, 2];
        channel.extend_from_slice(&encoder.finish().unwrap());
        assert_eq!(
            decode(&channel, 2, 2, PixelFormat::Rgba8).unwrap(),
            [240, 30, 20, 10]
        );
        channel[1] = 3;
        assert_eq!(
            decode(&channel, 2, 2, PixelFormat::Rgba8).unwrap(),
            [240, 14, 20, 30]
        );
        assert!(decode(&channel, 1, 2, PixelFormat::Rgba8).is_err());
        assert!(decode(&channel[..channel.len() - 1], 2, 2, PixelFormat::Rgba8).is_err());
        channel.push(0);
        assert!(decode(&channel, 2, 2, PixelFormat::Rgba8).is_err());
    }
}
