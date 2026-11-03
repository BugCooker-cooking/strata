//! Row streaming, linear-light, premultiplied resampling. Area reduction avoids
//! aliasing; enlargement uses pixel-center bilinear interpolation. No display
//! cache participates, and no full-image floating-point copy is allocated.
use super::CommandError;
use crate::DocumentError;
use std::collections::VecDeque;
use strata_pixels::PixelBuffer;

fn weights(index: u32, input: u32, output: u32) -> Vec<(u32, f64)> {
    let scale = f64::from(input) / f64::from(output);
    if input > output {
        let start = f64::from(index) * scale;
        let end = (f64::from(index) + 1.0) * scale;
        (start.floor() as u32..(end.ceil() as u32).min(input))
            .map(|i| {
                (
                    i,
                    (end.min(f64::from(i) + 1.0) - start.max(f64::from(i))) / scale,
                )
            })
            .collect()
    } else {
        let center = ((f64::from(index) + 0.5) * scale - 0.5).clamp(0.0, f64::from(input - 1));
        let first = center.floor() as u32;
        let fraction = center - f64::from(first);
        vec![
            (first, 1.0 - fraction),
            ((first + 1).min(input - 1), fraction),
        ]
    }
}

pub(super) fn resize(
    source: &PixelBuffer,
    width: u32,
    height: u32,
    out_width: u32,
    out_height: u32,
    source_icc: &[u8],
    target_icc: &[u8],
) -> Result<PixelBuffer, CommandError> {
    let format = source.format();
    let count = format
        .sample_count(out_width, out_height)
        .map_err(DocumentError::from)?;
    let mut out = match source {
        PixelBuffer::Rgba8(_) => PixelBuffer::Rgba8(Vec::with_capacity(count)),
        PixelBuffer::Rgba16(_) => PixelBuffer::Rgba16(Vec::with_capacity(count)),
    };
    let mut rows: VecDeque<(u32, Vec<[f64; 4]>)> = VecDeque::new();
    for y in 0..out_height {
        let mut sum = vec![[0.0f64; 4]; out_width as usize];
        for (sy, wy) in weights(y, height, out_height) {
            if wy == 0.0 {
                continue;
            }
            let index = if let Some(index) = rows.iter().position(|(row, _)| *row == sy) {
                index
            } else {
                let start = sy as usize * width as usize * 4;
                let end = start + width as usize * 4;
                let row = match source {
                    PixelBuffer::Rgba8(v) => PixelBuffer::Rgba8(v[start..end].to_vec()),
                    PixelBuffer::Rgba16(v) => PixelBuffer::Rgba16(v[start..end].to_vec()),
                };
                let linear = strata_color::to_linear_p3(&row, width, 1, source_icc)
                    .map_err(DocumentError::from)?;
                let mut horizontal = vec![[0.0f64; 4]; out_width as usize];
                for x in 0..out_width {
                    for (sx, wx) in weights(x, width, out_width) {
                        let pixel = linear[sx as usize].map(f64::from);
                        for c in 0..3 {
                            horizontal[x as usize][c] += pixel[c] * pixel[3] * wx;
                        }
                        horizontal[x as usize][3] += pixel[3] * wx;
                    }
                }
                if rows.len() == 2 {
                    rows.pop_front();
                }
                rows.push_back((sy, horizontal));
                rows.len() - 1
            };
            for (pixel, row) in sum.iter_mut().zip(&rows[index].1) {
                for c in 0..4 {
                    pixel[c] += row[c] * wy;
                }
            }
        }
        let linear: Vec<[f32; 4]> = sum
            .into_iter()
            .map(|mut p| {
                if p[3] > 0.0 {
                    for c in 0..3 {
                        p[c] /= p[3];
                    }
                }
                p.map(|v| v as f32)
            })
            .collect();
        let converted = strata_color::from_linear_p3(&linear, format, target_icc)
            .map_err(DocumentError::from)?;
        match (&mut out, converted) {
            (PixelBuffer::Rgba8(v), PixelBuffer::Rgba8(row)) => v.extend(row),
            (PixelBuffer::Rgba16(v), PixelBuffer::Rgba16(row)) => v.extend(row),
            _ => unreachable!(),
        }
    }
    Ok(out)
}
