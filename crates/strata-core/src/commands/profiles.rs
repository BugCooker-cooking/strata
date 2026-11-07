//! Assignment changes interpretation only. Conversion changes working samples
//! while preserving appearance, with explicit source locks and cancellation.
use super::*;
use crate::{Document, DocumentError, document::tile_limit};
use strata_pixels::{PixelBuffer, PixelFormat, TiledPixels, memory::MemoryKind};
fn check(cancel: &dyn Fn() -> bool) -> Result<(), CommandError> {
    if cancel() {
        Err(CommandError::Expired)
    } else {
        Ok(())
    }
}
pub(super) fn mutate(
    doc: &mut Document,
    id: &str,
    params: Value,
    cancel: &dyn Fn() -> bool,
) -> Result<Option<Value>, CommandError> {
    if !matches!(id, "color.assign" | "color.convert") {
        return Ok(None);
    }
    check(cancel)?;
    let space = if id == "color.assign" {
        let p: AssignProfile = parse(params)?;
        match p.space {
            ProfileSpace::Srgb => "srgb".to_owned(),
            ProfileSpace::DisplayP3 => "display_p3".to_owned(),
        }
    } else {
        let p: ConvertColor = parse(params)?;
        p.space
    };
    if doc
        .layers()
        .iter()
        .any(|l| l.info().locks.all || (id == "color.convert" && l.info().locks.pixels))
    {
        return Err(CommandError::Invalid("unlock all-locked layers before changing the document profile; conversion also requires unlocked pixels".into()));
    }
    let icc = strata_color::working_profile(&space).map_err(DocumentError::from)?;
    let profile = strata_color::resolve_profile(Some(&icc)).map_err(DocumentError::from)?;
    let pool = doc.memory_pool();
    let _profile_budget = pool
        .reserve(
            MemoryKind::Transient,
            (icc.len() as u64 + doc.working_icc.len() as u64) * 8 + 8192,
        )
        .map_err(DocumentError::from)?;
    if id == "color.convert" {
        for layer in &mut doc.layers {
            check(cancel)?;
            let convert_sample = |color: [u16; 4]| -> Result<[u16; 4], CommandError> {
                let source = if doc.format == PixelFormat::Rgba8 {
                    PixelBuffer::Rgba8(color.map(|v| v as u8).to_vec())
                } else {
                    PixelBuffer::Rgba16(color.to_vec())
                };
                let converted =
                    strata_color::convert_to_profile(&source, 1, 1, &doc.working_icc, &icc)
                        .map_err(DocumentError::from)?;
                Ok(match converted {
                    PixelBuffer::Rgba8(v) => [
                        u16::from(v[0]),
                        u16::from(v[1]),
                        u16::from(v[2]),
                        u16::from(v[3]),
                    ],
                    PixelBuffer::Rgba16(v) => [v[0], v[1], v[2], v[3]],
                })
            };
            match &mut layer.info.kind {
                crate::LayerKind::Text { text } => text.color = convert_sample(text.color)?,
                crate::LayerKind::Solid { color } => *color = convert_sample(*color)?,
                crate::LayerKind::Gradient { start, end, .. } => {
                    *start = convert_sample(*start)?;
                    *end = convert_sample(*end)?;
                }
                _ => {}
            }
            if let Some(s) = &mut layer.info.styles.drop_shadow {
                s.color = convert_sample(s.color)?;
            }
            if let Some(s) = &mut layer.info.styles.stroke {
                s.color = convert_sample(s.color)?;
            }
            if let Some(s) = &mut layer.info.styles.outer_glow {
                s.color = convert_sample(s.color)?;
            }
            if let Some(s) = &mut layer.info.styles.color_overlay {
                s.color = convert_sample(s.color)?;
            }
            if let Some(s) = &mut layer.info.styles.gradient_overlay {
                s.start = convert_sample(s.start)?;
                s.end = convert_sample(s.end)?;
            }
            let mut converted =
                TiledPixels::with_pool(doc.format, tile_limit(doc.format), pool.clone());
            for (coord, tile) in layer.pixels.tiles() {
                check(cancel)?;
                let _scratch = pool
                    .reserve(MemoryKind::Transient, 256 * 256 * 32)
                    .map_err(DocumentError::from)?;
                let tile = tile.map_err(DocumentError::from)?;
                let pixels =
                    strata_color::convert_to_profile(&tile, 256, 256, &doc.working_icc, &icc)
                        .map_err(DocumentError::from)?;
                converted
                    .insert_tile(*coord, pixels)
                    .map_err(DocumentError::from)?;
            }
            layer.pixels = converted;
        }
    }
    check(cancel)?;
    doc.working_icc = icc;
    doc.profile_name = profile.name;
    doc.profile_fallback = false;
    Ok(Some(serde_json::json!({"working_space":space})))
}
