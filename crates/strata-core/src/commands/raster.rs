use super::*;
use crate::{Document, DocumentError, Layer, LayerId, LayerKind, document::tile_limit};
use serde_json::json;
use std::collections::HashSet;
use strata_pixels::{
    PixelBuffer, TiledPixels,
    memory::{MemoryKind, Reservation},
};

struct Raster {
    pixels: PixelBuffer,
    x: i64,
    y: i64,
    width: u32,
    height: u32,
    _allocation: Reservation,
}
fn subtree(doc: &Document, id: LayerId) -> HashSet<LayerId> {
    doc.layers
        .iter()
        .filter(|l| l.id() == id || doc.is_descendant(l.id(), id))
        .map(Layer::id)
        .collect()
}
fn ensure_unlocked(doc: &Document, ids: &HashSet<LayerId>) -> Result<(), CommandError> {
    if doc.layers.iter().any(|l| {
        ids.contains(&l.id())
            && (l.info.locks.all || l.info.locks.pixels || l.info.locks.transparency)
    }) {
        return Err(CommandError::Invalid("operation would bake or remove locked pixels/transparency; unlock affected layers first".into()));
    }
    Ok(())
}
fn raster(doc: &Document) -> Result<Raster, CommandError> {
    let mut extent: Option<(i64, i64, i64, i64)> = None;
    let mut include = |x: i64, y: i64, w: u32, h: u32| -> Result<(), CommandError> {
        let right = x
            .checked_add(i64::from(w))
            .ok_or_else(|| CommandError::Invalid("content bounds overflow".into()))?;
        let bottom = y
            .checked_add(i64::from(h))
            .ok_or_else(|| CommandError::Invalid("content bounds overflow".into()))?;
        extent = Some(match extent {
            Some((a, b, c, d)) => (a.min(x), b.min(y), c.max(right), d.max(bottom)),
            None => (x, y, right, bottom),
        });
        Ok(())
    };
    for layer in &doc.layers {
        if !doc.effective_visibility(layer.id()) {
            continue;
        }
        if matches!(
            layer.info.kind,
            LayerKind::Solid { .. } | LayerKind::Gradient { .. }
        ) {
            let [w, h] = layer
                .info
                .generated_size
                .unwrap_or([doc.frame.width, doc.frame.height]);
            let pad = doc.style_outset(layer.id());
            let x = layer
                .info
                .x
                .checked_sub(pad[0])
                .ok_or_else(|| CommandError::Invalid("style bounds overflow".into()))?;
            let y = layer
                .info
                .y
                .checked_sub(pad[1])
                .ok_or_else(|| CommandError::Invalid("style bounds overflow".into()))?;
            let w = u32::try_from(i64::from(w) + pad[0] + pad[2])
                .map_err(|_| DocumentError::ResourceLimit)?;
            let h = u32::try_from(i64::from(h) + pad[1] + pad[3])
                .map_err(|_| DocumentError::ResourceLimit)?;
            include(x, y, w, h)?;
        }
        for coord in layer.pixels.tile_coords() {
            let [x, y] = coord.origin();
            let pad = doc.style_outset(layer.id());
            let x = x
                .checked_add(layer.info.x)
                .and_then(|v| v.checked_sub(pad[0]))
                .ok_or_else(|| CommandError::Invalid("style bounds overflow".into()))?;
            let y = y
                .checked_add(layer.info.y)
                .and_then(|v| v.checked_sub(pad[1]))
                .ok_or_else(|| CommandError::Invalid("style bounds overflow".into()))?;
            include(
                x,
                y,
                (256 + pad[0] + pad[2]) as u32,
                (256 + pad[1] + pad[3]) as u32,
            )?;
        }
    }
    let (x, y, right, bottom) =
        extent.unwrap_or((doc.frame.x, doc.frame.y, doc.frame.x + 1, doc.frame.y + 1));
    let width = u32::try_from(i128::from(right) - i128::from(x))
        .map_err(|_| DocumentError::ResourceLimit)?;
    let height = u32::try_from(i128::from(bottom) - i128::from(y))
        .map_err(|_| DocumentError::ResourceLimit)?;
    let bytes = doc
        .format
        .byte_count(width, height)
        .map_err(DocumentError::from)? as u64;
    if bytes > crate::MAX_FLAT_IMAGE_BYTES {
        return Err(DocumentError::ResourceLimit.into());
    }
    let allocation = doc
        .memory_pool()
        .reserve(MemoryKind::Transient, bytes)
        .map_err(DocumentError::from)?;
    let pixels = doc.export_region_pixels(
        strata_pixels::CoverageRect {
            x,
            y,
            width,
            height,
        },
        &|| false,
    )?;
    Ok(Raster {
        pixels,
        x,
        y,
        width,
        height,
        _allocation: allocation,
    })
}
fn pixel_layer(doc: &Document, image: &Raster, name: String) -> Result<Layer, CommandError> {
    let pixels = TiledPixels::from_dense_with_pool(
        image.width,
        image.height,
        &image.pixels,
        tile_limit(doc.format),
        doc.memory_pool(),
    )
    .map_err(DocumentError::from)?;
    let mut layer = Layer::new(name, pixels);
    layer.info.x = image.x;
    layer.info.y = image.y;
    Ok(layer)
}
pub(super) fn mutate(
    doc: &mut Document,
    id: &str,
    params: Value,
) -> Result<Option<Value>, CommandError> {
    if id == "layer.rasterize" {
        let p: RasterizeTarget = parse(params)?;
        let target = doc.resolve(&p.layer)?;
        let original = doc
            .layers
            .iter()
            .find(|l| l.id() == target)
            .unwrap()
            .info
            .clone();
        if matches!(original.kind, LayerKind::Group) && original.pass_through && p.bake_backdrop {
            return bake_adjustment(doc, target).map(Some);
        }
        if matches!(original.kind, LayerKind::Pixel) {
            return Ok(Some(json!({"layer_id":target})));
        }
        if original.kind.is_adjustment() {
            if !p.bake_backdrop {
                return Err(CommandError::Invalid("an adjustment needs its lower input stack. Explicitly approve bake_backdrop=true to consolidate that input and this adjustment into one pixel layer, preserving undo.".into()));
            }
            return bake_adjustment(doc, target).map(Some);
        }
        let ids = subtree(doc, target);
        ensure_unlocked(doc, &ids)?;
        let mut isolated = doc.clone();
        isolated.layers.retain(|l| ids.contains(&l.id()));
        let root = isolated
            .layers
            .iter_mut()
            .find(|l| l.id() == target)
            .unwrap();
        root.info.parent = None;
        root.info.opacity = 1.0;
        root.info.blend_mode = crate::BlendMode::Normal;
        root.info.clipped = false;
        root.info.visible = true;
        let image = raster(&isolated)?;
        let mut layer = pixel_layer(doc, &image, original.name.clone())?;
        layer.info.id = target;
        layer.info.parent = original.parent;
        layer.info.opacity = original.opacity;
        layer.info.fill_opacity = 1.0;
        layer.info.blend_mode = original.blend_mode;
        layer.info.clipped = original.clipped;
        layer.info.visible = original.visible;
        layer.info.color_label = original.color_label;
        layer.info.locks = original.locks;
        let at = doc.layers.iter().position(|l| l.id() == target).unwrap();
        let insertion = doc.layers[..at]
            .iter()
            .filter(|l| !ids.contains(&l.id()))
            .count();
        doc.layers.retain(|l| !ids.contains(&l.id()));
        doc.layers.insert(insertion, layer);
        doc.active = Some(target);
        return Ok(Some(json!({"layer_id":target})));
    }
    if id == "layer.mask.apply" {
        let p: RasterizeTarget = parse(params)?;
        let target = doc.resolve(&p.layer)?;
        let ids = HashSet::from([target]);
        ensure_unlocked(doc, &ids)?;
        let layer = doc.layers.iter().find(|l| l.id() == target).unwrap();
        if layer.info.mask.as_ref().is_none_or(|m| !m.enabled) {
            return Err(CommandError::Invalid("an enabled mask is required".into()));
        }
        if !matches!(layer.info.kind, LayerKind::Pixel) {
            return mutate(
                doc,
                "layer.rasterize",
                json!({"layer":{"id":target},"bake_backdrop":p.bake_backdrop}),
            );
        }
        let layer = doc.layers.iter_mut().find(|l| l.id() == target).unwrap();
        let coords: Vec<_> = layer.pixels.tile_coords().copied().collect();
        for coord in coords {
            let _scratch = layer
                .pixels
                .memory_pool()
                .reserve(MemoryKind::Transient, 256 * 256 * 10)
                .map_err(DocumentError::from)?;
            let [ox, oy] = coord.origin();
            let coverage =
                crate::composite::layer_mask_tile(layer, ox + layer.info.x, oy + layer.info.y)?
                    .unwrap_or_else(|| vec![65535; 256 * 256]);
            let mut tile = (*layer
                .pixels
                .tile(coord)
                .map_err(DocumentError::from)?
                .unwrap())
            .clone();
            match &mut tile {
                PixelBuffer::Rgba8(values) => {
                    for (pixel, mask) in values.chunks_exact_mut(4).zip(&coverage) {
                        pixel[3] = ((u32::from(pixel[3]) * u32::from(*mask) + 32767) / 65535) as u8;
                    }
                }
                PixelBuffer::Rgba16(values) => {
                    for (pixel, mask) in values.chunks_exact_mut(4).zip(&coverage) {
                        pixel[3] =
                            ((u64::from(pixel[3]) * u64::from(*mask) + 32767) / 65535) as u16;
                    }
                }
            }
            layer
                .pixels
                .insert_tile(coord, tile)
                .map_err(DocumentError::from)?;
        }
        layer.mask = None;
        layer.info.mask = None;
        return Ok(Some(json!({"layer_id":target})));
    }
    if !matches!(
        id,
        "layer.merge_down" | "layer.merge_visible" | "layer.flatten" | "layer.stamp_visible"
    ) {
        return Ok(None);
    }
    let (ids, name, preserve, parent, replacement, mut render_doc) = if id == "layer.merge_down" {
        let p: Target = parse(params)?;
        let target = doc.resolve(&p.layer)?;
        let upper = doc.layers.iter().find(|l| l.id() == target).unwrap();
        let siblings: Vec<_> = doc
            .layers
            .iter()
            .filter(|l| l.info.parent == upper.info.parent)
            .collect();
        let index = siblings.iter().position(|l| l.id() == target).unwrap();
        if index == 0 {
            return Err(CommandError::Invalid("no lower sibling".into()));
        }
        let lower = siblings[index - 1];
        if lower.info.clipped {
            return Err(CommandError::Invalid(
                "merge the full clipping chain with merge_visible instead".into(),
            ));
        }
        let mut ids = subtree(doc, target);
        ids.extend(subtree(doc, lower.id()));
        ensure_unlocked(doc, &ids)?;
        let mut snapshot = doc.clone();
        snapshot.layers.retain(|l| ids.contains(&l.id()));
        for layer in &mut snapshot.layers {
            if layer.id() == target || layer.id() == lower.id() {
                layer.info.parent = None;
            }
        }
        (
            ids,
            lower.info.name.clone(),
            false,
            upper.info.parent,
            Some(lower.id()),
            snapshot,
        )
    } else {
        let _: Empty = parse(params)?;
        let ids = doc
            .layers
            .iter()
            .filter(|l| id == "layer.flatten" || doc.effective_visibility(l.id()))
            .map(Layer::id)
            .collect::<HashSet<_>>();
        if id != "layer.stamp_visible" {
            ensure_unlocked(doc, &ids)?;
        }
        (
            ids,
            if id == "layer.flatten" {
                "Background"
            } else {
                "Merged"
            }
            .into(),
            id == "layer.stamp_visible",
            None,
            None,
            doc.clone(),
        )
    };
    render_doc.selection = None;
    let image = raster(&render_doc)?;
    let mut layer = pixel_layer(doc, &image, name)?;
    if let Some(replacement) = replacement {
        layer.info.id = replacement;
    }
    layer.info.parent = parent;
    let new = layer.id();
    let mut candidate = doc.clone();
    if preserve {
        if candidate.layers.len() >= crate::MAX_LAYERS {
            return Err(DocumentError::ResourceLimit.into());
        }
        candidate.layers.push(layer);
    } else {
        let insertion = candidate
            .layers
            .iter()
            .rposition(|l| ids.contains(&l.id()))
            .map(|i| {
                candidate.layers[..i]
                    .iter()
                    .filter(|l| !ids.contains(&l.id()))
                    .count()
            })
            .unwrap_or(candidate.layers.len());
        // Hidden descendants whose visible container is merged remain hidden,
        // reparented to the nearest surviving ancestor at the same canvas coords.
        let parents: std::collections::HashMap<_, _> = candidate
            .layers
            .iter()
            .map(|l| (l.id(), l.info.parent))
            .collect();
        candidate.layers.retain(|l| !ids.contains(&l.id()));
        for child in &mut candidate.layers {
            while child.info.parent.is_some_and(|id| ids.contains(&id)) {
                child.info.parent = child.info.parent.and_then(|id| parents[&id]);
            }
        }
        candidate
            .layers
            .insert(insertion.min(candidate.layers.len()), layer);
    }
    candidate.active = Some(new);
    candidate.validate()?;
    if !preserve {
        let bytes = doc
            .format
            .byte_count(doc.frame.width, doc.frame.height)
            .map_err(DocumentError::from)? as u64;
        let _compare = doc
            .memory_pool()
            .reserve(MemoryKind::Transient, bytes * 2)
            .map_err(DocumentError::from)?;
        let before = doc.export_pixels()?;
        let after = candidate.export_pixels()?;
        let within = match (&before, &after) {
            (PixelBuffer::Rgba8(a), PixelBuffer::Rgba8(b)) => {
                a.iter().zip(b).all(|(a, b)| a.abs_diff(*b) <= 1)
            }
            (PixelBuffer::Rgba16(a), PixelBuffer::Rgba16(b)) => {
                a.iter().zip(b).all(|(a, b)| a.abs_diff(*b) <= 3)
            }
            _ => false,
        };
        if !within {
            return Err(CommandError::Invalid("merge would change the composed image beyond the bit-depth tolerance; merge the complete visible stack instead".into()));
        }
    }
    *doc = candidate;
    Ok(Some(json!({"layer_id":new})))
}

fn bake_adjustment(doc: &mut Document, target: LayerId) -> Result<Value, CommandError> {
    let root = doc.layers.iter().find(|l| l.id() == target).unwrap();
    let parent = root.info.parent;
    let name = root.info.name.clone();
    let label = root.info.color_label;
    let siblings: Vec<_> = doc
        .layers
        .iter()
        .filter(|l| l.info.parent == parent)
        .map(Layer::id)
        .collect();
    let end = siblings.iter().position(|id| *id == target).unwrap();
    let mut ids = HashSet::new();
    for id in &siblings[..=end] {
        ids.extend(subtree(doc, *id));
    }
    ensure_unlocked(doc, &ids)?;
    let mut render_doc = doc.clone();
    render_doc.layers.retain(|l| ids.contains(&l.id()));
    for layer in &mut render_doc.layers {
        if layer.info.parent == parent {
            layer.info.parent = None;
        }
    }
    let image = raster(&render_doc)?;
    let mut replacement = pixel_layer(doc, &image, name)?;
    replacement.info.id = target;
    replacement.info.parent = parent;
    replacement.info.color_label = label;
    let mut candidate = doc.clone();
    let at = candidate
        .layers
        .iter()
        .position(|l| l.id() == target)
        .unwrap();
    let insertion = candidate.layers[..at]
        .iter()
        .filter(|l| !ids.contains(&l.id()))
        .count();
    candidate.layers.retain(|l| !ids.contains(&l.id()));
    candidate.layers.insert(insertion, replacement);
    candidate.active = Some(target);
    candidate.validate()?;
    let bytes = doc
        .format
        .byte_count(doc.frame.width, doc.frame.height)
        .map_err(DocumentError::from)? as u64;
    let _comparison = doc
        .memory_pool()
        .reserve(MemoryKind::Transient, bytes * 2)
        .map_err(DocumentError::from)?;
    let before = doc.export_pixels()?;
    let after = candidate.export_pixels()?;
    let same = match (&before, &after) {
        (PixelBuffer::Rgba8(a), PixelBuffer::Rgba8(b)) => {
            a.iter().zip(b).all(|(a, b)| a.abs_diff(*b) <= 1)
        }
        (PixelBuffer::Rgba16(a), PixelBuffer::Rgba16(b)) => {
            a.iter().zip(b).all(|(a, b)| a.abs_diff(*b) <= 3)
        }
        _ => false,
    };
    if !same {
        return Err(CommandError::Invalid(
            "baking this adjustment would change the image; use a complete visible merge instead"
                .into(),
        ));
    }
    *doc = candidate;
    Ok(json!({"layer_id":target}))
}
