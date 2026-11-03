use super::*;
use crate::{Document, DocumentCandidate, DocumentError};
use serde_json::json;
use strata_pixels::{PixelBuffer, PixelFormat, memory::MemoryPool};

pub(super) fn document(p: NewDocument, pool: MemoryPool) -> Result<Document, CommandError> {
    let preset = match p.preset.as_deref() {
        None => None,
        Some("web_1080p") => Some((1920, 1080, 72)),
        Some("a4_300dpi") => Some((2480, 3508, 300)),
        Some("photo_4x6_300dpi") => Some((1200, 1800, 300)),
        Some("square_2048") => Some((2048, 2048, 72)),
        Some(_) => {
            return Err(CommandError::Invalid(
                "unknown preset: use web_1080p, a4_300dpi, photo_4x6_300dpi or square_2048".into(),
            ));
        }
    };
    let (width, height) = match (p.width, p.height, preset) {
        (None, None, Some((w, h, _))) => (w, h),
        (Some(w), Some(h), None) => (w, h),
        (Some(w), Some(h), Some((pw, ph, _))) if (w, h) == (pw, ph) => (w, h),
        _ => {
            return Err(CommandError::Invalid(
                "provide width/height together or an exact named preset".into(),
            ));
        }
    };
    let resolution = p
        .resolution_dpi
        .unwrap_or(preset.map_or(72, |(_, _, dpi)| dpi));
    if !(1..=2400).contains(&resolution) {
        return Err(CommandError::Invalid(
            "resolution_dpi must be an integer in 1..2400".into(),
        ));
    }
    let format = match p.bit_depth {
        8 => PixelFormat::Rgba8,
        16 => PixelFormat::Rgba16,
        _ => return Err(CommandError::Invalid("bit_depth must be 8 or 16".into())),
    };
    if format
        .byte_count(width, height)
        .map_err(DocumentError::from)? as u64
        > crate::MAX_FLAT_IMAGE_BYTES
    {
        return Err(DocumentError::ResourceLimit.into());
    }
    let space = p.working_space.as_deref().unwrap_or("srgb");
    let icc = strata_color::working_profile(space).map_err(DocumentError::from)?;
    let empty = match format {
        PixelFormat::Rgba8 => PixelBuffer::Rgba8(vec![0; 4]),
        PixelFormat::Rgba16 => PixelBuffer::Rgba16(vec![0; 4]),
    };
    let mut doc =
        DocumentCandidate::from_image_with_pool(1, 1, empty, Some(icc), "Background".into(), pool)?
            .into_document();
    doc.source_icc = None;
    doc.frame.width = width;
    doc.frame.height = height;
    doc.resolution_dpi = resolution;
    doc.preset = p
        .preset
        .filter(|_| preset.is_some_and(|(_, _, dpi)| dpi == resolution));
    if let Some(fill) = p.background {
        super::executor::mutate(
            &mut doc,
            "layer.create",
            json!({"name":"Background","fill":fill}),
        )?;
        doc.layers.remove(0);
    }
    doc.validate()?;
    Ok(doc)
}
