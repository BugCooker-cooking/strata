use super::*;
use crate::{Document, DocumentError};
use strata_io::export::ExportFormat;
use strata_pixels::{PixelBuffer, PixelFormat};

pub(super) struct ExportPlan {
    pub format: ExportFormat,
    pub icc: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub allocation: strata_pixels::memory::Reservation,
    background: Option<[u8; 3]>,
}

impl ExportPlan {
    pub fn new(doc: &Document, p: &ExportFile) -> Result<Self, CommandError> {
        let (width, height) = match (p.width, p.height) {
            (None, None) => (doc.frame().width, doc.frame().height),
            (Some(w), Some(h)) if w > 0 && h > 0 => (w, h),
            _ => {
                return Err(CommandError::Invalid(
                    "width and height must both be positive integers".into(),
                ));
            }
        };
        if doc
            .format()
            .byte_count(width, height)
            .map_err(DocumentError::from)? as u64
            > crate::MAX_FLAT_IMAGE_BYTES
        {
            return Err(DocumentError::ResourceLimit.into());
        }
        let format = validate_format(p)?;
        if p.format == "webp" && (width > 16383 || height > 16383) {
            return Err(CommandError::Invalid(
                "WebP dimensions must be at most 16383 pixels".into(),
            ));
        }
        if matches!(
            format,
            ExportFormat::Jpeg { .. } | ExportFormat::WebP | ExportFormat::WebPLossy { .. }
        ) && doc.bit_depth() == 16
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
        let source_bytes = doc
            .format()
            .byte_count(doc.frame().width, doc.frame().height)
            .map_err(DocumentError::from)? as u64;
        let output_bytes = doc
            .format()
            .byte_count(width, height)
            .map_err(DocumentError::from)? as u64;
        let scratch = source_bytes * 2
            + output_bytes * 3
            + if matches!(format, ExportFormat::WebPLossy { .. }) {
                output_bytes * 3
            } else {
                0
            }
            + (u64::from(width) + u64::from(doc.frame().width)) * 96
            + 16 * 1024 * 1024;
        let allocation = doc
            .memory_pool()
            .reserve(strata_pixels::memory::MemoryKind::Transient, scratch)
            .map_err(DocumentError::from)?;
        Ok(Self {
            format,
            icc,
            width,
            height,
            allocation,
            background: p.background,
        })
    }

    pub fn pixels(
        &self,
        doc: &Document,
        cancel: &dyn Fn() -> bool,
    ) -> Result<PixelBuffer, CommandError> {
        let source = doc.export_pixels_cancellable(cancel)?;
        let source = if (self.width, self.height) != (doc.frame().width, doc.frame().height) {
            super::resample::resize(
                &source,
                doc.frame().width,
                doc.frame().height,
                self.width,
                self.height,
                doc.working_icc(),
                &self.icc,
            )?
        } else if doc.working_icc() == self.icc {
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
            let count = self.width as usize * self.height as usize * 4;
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
            (
                PixelBuffer::Rgba16(v),
                ExportFormat::Jpeg { .. } | ExportFormat::WebP | ExportFormat::WebPLossy { .. },
            ) => PixelBuffer::Rgba8(
                v.into_iter()
                    .map(|v| ((u32::from(v) + 128) / 257) as u8)
                    .collect(),
            ),
            (pixels, _) => pixels,
        })
    }
}

pub(super) fn validate_format(p: &ExportFile) -> Result<ExportFormat, CommandError> {
    let format = match p.format.as_str() {
        "png" => ExportFormat::Png,
        "tiff" => ExportFormat::Tiff,
        "webp" if p.webp_lossless == Some(false) => ExportFormat::WebPLossy {
            quality: p.quality.unwrap_or(80),
        },
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
    if p.format != "webp" && p.webp_lossless.is_some() {
        return Err(CommandError::Invalid(
            "webp_lossless applies only to WebP".into(),
        ));
    }
    if p.format != "jpeg" && p.background.is_some() {
        return Err(CommandError::Invalid(
            "background applies only to JPEG".into(),
        ));
    }
    if p.format != "jpeg"
        && !matches!(format, ExportFormat::WebPLossy { .. })
        && p.quality.is_some()
    {
        return Err(CommandError::Invalid(
            "quality applies only to JPEG or lossy WebP".into(),
        ));
    }
    if p.format == "jpeg"
        && (p.background.is_none() || !(1..=100).contains(&p.quality.unwrap_or(90)))
    {
        return Err(CommandError::Invalid(
            "JPEG requires background RGB and quality 1..100".into(),
        ));
    }
    if let ExportFormat::WebPLossy { quality } = format
        && !(1..=100).contains(&quality)
    {
        return Err(CommandError::Invalid("WebP quality must be 1..100".into()));
    }
    Ok(format)
}
