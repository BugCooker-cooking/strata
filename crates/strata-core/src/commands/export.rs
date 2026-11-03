use super::*;
use crate::{Document, DocumentError};
use strata_io::export::ExportFormat;
use strata_pixels::{PixelBuffer, PixelFormat};

pub(super) struct ExportPlan {
    pub format: ExportFormat,
    pub icc: Vec<u8>,
    background: Option<[u8; 3]>,
}

impl ExportPlan {
    pub fn new(doc: &Document, p: &ExportFile) -> Result<Self, CommandError> {
        let format = match p.format.as_str() {
            "png" => ExportFormat::Png,
            "tiff" => ExportFormat::Tiff,
            "webp" => ExportFormat::WebP,
            "jpeg" => ExportFormat::Jpeg {
                quality: p.quality.unwrap_or(90),
            },
            _ => {
                return Err(CommandError::Invalid(
                    "format must be png, tiff, jpeg or webp".into(),
                ));
            }
        };
        if p.format != "jpeg" && (p.quality.is_some() || p.background.is_some()) {
            return Err(CommandError::Invalid(
                "quality/background apply only to JPEG; WebP is lossless".into(),
            ));
        }
        if p.format == "jpeg"
            && (p.background.is_none() || !(1..=100).contains(&p.quality.unwrap_or(90)))
        {
            return Err(CommandError::Invalid(
                "JPEG requires background RGB and quality 1..100".into(),
            ));
        }
        if matches!(format, ExportFormat::Jpeg { .. } | ExportFormat::WebP)
            && doc.bit_depth() == 16
            && !p.allow_quantization
        {
            return Err(CommandError::Invalid(
                "JPEG/WebP require explicit allow_quantization=true for 16-bit documents".into(),
            ));
        }
        let icc = match p.target_space.as_deref() {
            None => doc.working_icc().to_vec(),
            Some(space) => strata_color::working_profile(space).map_err(DocumentError::from)?,
        };
        Ok(Self {
            format,
            icc,
            background: p.background,
        })
    }

    pub fn pixels(&self, doc: &Document) -> Result<PixelBuffer, CommandError> {
        let source = doc.export_pixels()?;
        let source = if doc.working_icc() == self.icc {
            source
        } else {
            strata_color::convert_to_profile(
                &source,
                doc.frame().width,
                doc.frame().height,
                doc.working_icc(),
                &self.icc,
            )
            .map_err(DocumentError::from)?
        };
        let transparent = match &source {
            PixelBuffer::Rgba8(v) => v.chunks_exact(4).any(|p| p[3] != 255),
            PixelBuffer::Rgba16(v) => v.chunks_exact(4).any(|p| p[3] != 65535),
        };
        if let Some(background) = self.background.filter(|_| transparent) {
            let matte = strata_color::to_linear_p3(
                &PixelBuffer::Rgba8(vec![background[0], background[1], background[2], 255]),
                1,
                1,
                &self.icc,
            )
            .map_err(DocumentError::from)?[0];
            // Bounded strips avoid an additional full-image floating-point copy.
            let count = doc.frame().width as usize * doc.frame().height as usize * 4;
            let mut result = Vec::with_capacity(count);
            for start in (0..count).step_by(65536 * 4) {
                let end = (start + 65536 * 4).min(count);
                let strip = match &source {
                    PixelBuffer::Rgba8(v) => PixelBuffer::Rgba8(v[start..end].to_vec()),
                    PixelBuffer::Rgba16(v) => PixelBuffer::Rgba16(v[start..end].to_vec()),
                };
                let mut linear =
                    strata_color::to_linear_p3(&strip, ((end - start) / 4) as u32, 1, &self.icc)
                        .map_err(DocumentError::from)?;
                for pixel in &mut linear {
                    for channel in 0..3 {
                        pixel[channel] =
                            pixel[channel] * pixel[3] + matte[channel] * (1.0 - pixel[3]);
                    }
                    pixel[3] = 1.0;
                }
                let PixelBuffer::Rgba8(converted) =
                    strata_color::from_linear_p3(&linear, PixelFormat::Rgba8, &self.icc)
                        .map_err(DocumentError::from)?
                else {
                    unreachable!()
                };
                result.extend(converted);
            }
            return Ok(PixelBuffer::Rgba8(result));
        }
        Ok(match (source, self.format) {
            (PixelBuffer::Rgba16(v), ExportFormat::Jpeg { .. } | ExportFormat::WebP) => {
                PixelBuffer::Rgba8(
                    v.into_iter()
                        .map(|v| ((u32::from(v) + 128) / 257) as u8)
                        .collect(),
                )
            }
            (pixels, _) => pixels,
        })
    }
}
