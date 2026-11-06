//! Integer-code depth conversion keeps independent editable layers and hidden
//! RGBA samples. No ICC conversion, compositing or display textures participate.
use super::*;
use crate::{Document, DocumentError, LayerKind, document::tile_limit};
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
    if id != "image.convert_bit_depth" {
        return Ok(None);
    }
    check(cancel)?;
    let p: ConvertBitDepth = parse(params)?;
    let previous = match doc.format {
        PixelFormat::Rgba8 => ImageBitDepth::Eight,
        PixelFormat::Rgba16 => ImageBitDepth::Sixteen,
    };
    let result = |changed| serde_json::json!({"previous_bit_depth":previous,"bit_depth":p.bit_depth,"changed":changed,"preserved_layer_structure":true,"masks_unchanged":true,"profile_unchanged":true});
    if previous == p.bit_depth {
        return Ok(Some(result(false)));
    }
    if doc.layers.iter().any(|l| {
        let lock = l.info().locks;
        lock.all || lock.pixels || lock.transparency
    }) {
        return Err(CommandError::Invalid("unlock all affected layer/ancestor pixel, transparency and all locks before converting document bit depth".into()));
    }
    let target = match p.bit_depth {
        ImageBitDepth::Eight => PixelFormat::Rgba8,
        ImageBitDepth::Sixteen => PixelFormat::Rgba16,
    };
    let check_size = |w, h| -> Result<(), CommandError> {
        if target.byte_count(w, h).map_err(DocumentError::from)? as u64
            > crate::MAX_FLAT_IMAGE_BYTES
        {
            Err(DocumentError::ResourceLimit.into())
        } else {
            Ok(())
        }
    };
    check_size(doc.frame.width, doc.frame.height)?;
    let mut bytes = 0u64;
    for layer in &doc.layers {
        check(cancel)?;
        if let Some([w, h]) = layer.info.generated_size {
            check_size(w, h)?;
        }
        if let Some(bounds) = layer.info.spatial_bounds {
            check_size(bounds.width, bounds.height)?;
        }
        let target_bytes = (layer.pixels.tile_count() as u64)
            .checked_mul(256 * 256 * 4 * target.bytes_per_sample() as u64)
            .ok_or(DocumentError::ResourceLimit)?;
        bytes = bytes
            .checked_add(target_bytes)
            .and_then(|v| v.checked_add(layer.mask.as_ref().map_or(0, |m| m.referenced_bytes())))
            .ok_or(DocumentError::ResourceLimit)?;
    }
    for mask in [&doc.selection, &doc.previous_selection]
        .into_iter()
        .flatten()
    {
        bytes = bytes
            .checked_add(mask.referenced_bytes())
            .ok_or(DocumentError::ResourceLimit)?;
    }
    if bytes > crate::MAX_DOCUMENT_BYTES {
        return Err(DocumentError::ResourceLimit.into());
    }
    let pool = doc.memory_pool();
    let convert_color = |color: &mut [u16; 4]| {
        for value in color {
            *value = match previous {
                ImageBitDepth::Eight => *value * 257,
                ImageBitDepth::Sixteen => ((u32::from(*value) + 128) / 257) as u16,
            };
        }
    };
    for layer in &mut doc.layers {
        check(cancel)?;
        match &mut layer.info.kind {
            LayerKind::Solid { color } => convert_color(color),
            LayerKind::Gradient { start, end, .. } => {
                convert_color(start);
                convert_color(end);
            }
            LayerKind::Pixel
            | LayerKind::Group
            | LayerKind::Exposure { .. }
            | LayerKind::HueSaturation { .. }
            | LayerKind::Adjustment { .. } => {}
        }
        let crate::styles::LayerStyles {
            drop_shadow,
            stroke,
            outer_glow,
            color_overlay,
            gradient_overlay,
        } = &mut layer.info.styles;
        if let Some(style) = drop_shadow {
            convert_color(&mut style.color);
        }
        if let Some(style) = stroke {
            convert_color(&mut style.color);
        }
        if let Some(style) = outer_glow {
            convert_color(&mut style.color);
        }
        if let Some(style) = color_overlay {
            convert_color(&mut style.color);
        }
        if let Some(style) = gradient_overlay {
            convert_color(&mut style.start);
            convert_color(&mut style.end);
        }
        let mut output = TiledPixels::with_pool(target, tile_limit(target), pool.clone());
        for (coord, tile) in layer.pixels.tiles() {
            check(cancel)?;
            let _scratch = pool
                .reserve(MemoryKind::Transient, 256 * 256 * 32)
                .map_err(DocumentError::from)?;
            let tile = tile.map_err(DocumentError::from)?;
            let converted = match &*tile {
                PixelBuffer::Rgba8(source) => {
                    let mut values = Vec::with_capacity(source.len());
                    for row in source.chunks(256 * 4) {
                        check(cancel)?;
                        values.extend(row.iter().map(|v| u16::from(*v) * 257));
                    }
                    PixelBuffer::Rgba16(values)
                }
                PixelBuffer::Rgba16(source) => {
                    let mut values = Vec::with_capacity(source.len());
                    for row in source.chunks(256 * 4) {
                        check(cancel)?;
                        values.extend(row.iter().map(|v| ((u32::from(*v) + 128) / 257) as u8));
                    }
                    PixelBuffer::Rgba8(values)
                }
            };
            output
                .insert_tile(*coord, converted)
                .map_err(DocumentError::from)?;
        }
        layer.pixels = output;
    }
    check(cancel)?;
    doc.format = target;
    Ok(Some(result(true)))
}
