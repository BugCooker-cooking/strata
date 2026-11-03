use crate::StrataError;
use strata_core::Document;
use strata_pixels::{PixelBuffer, PixelValue};

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct LayerThumbnail {
    pub layer_id: String,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

pub fn make(doc: &Document) -> Result<Vec<LayerThumbnail>, StrataError> {
    let destination =
        strata_color::working_profile("display_p3").map_err(|e| StrataError::Color {
            message: e.to_string(),
        })?;
    doc.layers()
        .iter()
        .map(|layer| {
            // Fit occupied source tiles, independent of opacity/visibility/position.
            // Sparse layers remain sparse: at most 48×48 samples per layer.
            let mut bounds: Option<(i64, i64, i64, i64)> = None;
            for (coord, _) in layer.pixels().tiles() {
                let [x, y] = coord.origin();
                bounds = Some(match bounds {
                    Some((a, b, c, d)) => (
                        a.min(x),
                        b.min(y),
                        c.max(x.saturating_add(256)),
                        d.max(y.saturating_add(256)),
                    ),
                    None => (x, y, x.saturating_add(256), y.saturating_add(256)),
                });
            }
            let (left, top, right, bottom) = bounds.unwrap_or((0, 0, 1, 1));
            let w = (right as f64 - left as f64).max(1.0);
            let h = (bottom as f64 - top as f64).max(1.0);
            let scale = 48.0 / w.max(h);
            let width = (w * scale).round().clamp(1.0, 48.0) as u32;
            let height = (h * scale).round().clamp(1.0, 48.0) as u32;
            let mut samples = Vec::with_capacity((width * height * 4) as usize);
            for y in 0..height {
                for x in 0..width {
                    let sx = left as f64 + (f64::from(x) + 0.5) * w / f64::from(width);
                    let sy = top as f64 + (f64::from(y) + 0.5) * h / f64::from(height);
                    match layer.pixels().pixel(sx.floor() as i64, sy.floor() as i64) {
                        PixelValue::Rgba8(v) => samples.extend(v.map(|v| u16::from(v) * 257)),
                        PixelValue::Rgba16(v) => samples.extend(v),
                    }
                }
            }
            let pixels = strata_color::convert_to_profile(
                &PixelBuffer::Rgba16(samples),
                width,
                height,
                doc.working_icc(),
                &destination,
            )
            .map_err(|e| StrataError::Color {
                message: e.to_string(),
            })?;
            let PixelBuffer::Rgba16(pixels) = pixels else {
                unreachable!()
            };
            Ok(LayerThumbnail {
                layer_id: layer.id().to_string(),
                width,
                height,
                rgba: pixels
                    .into_iter()
                    .map(|v| ((u32::from(v) + 128) / 257) as u8)
                    .collect(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use strata_core::DocumentCandidate;
    #[test]
    fn thumbnails_are_bounded_color_managed_copies_with_alpha() {
        for depth in [8, 16] {
            let source = if depth == 8 {
                PixelBuffer::Rgba8([180, 80, 20, 127].repeat(256 * 512))
            } else {
                PixelBuffer::Rgba16([46260, 20560, 5140, 32639].repeat(256 * 512))
            };
            let doc = DocumentCandidate::from_image(256, 512, source.clone(), None, "photo".into())
                .unwrap()
                .into_document();
            let previews = make(&doc).unwrap();
            let p = &previews[0];
            assert_eq!((p.width, p.height, p.rgba.len()), (24, 48, 24 * 48 * 4));
            assert!(p.rgba.chunks_exact(4).all(|v| v[3] == 127));
            assert_eq!(doc.export_pixels().unwrap(), source);
            assert_eq!(make(&doc).unwrap(), previews);
        }
    }
}
