//! One staged group operation keeps children editable and never flattens them.
use super::*;
use crate::{
    Document, DocumentError, GeneratedGeometry, LayerId, LayerKind,
    geometry::Homography,
    geometry_pixels::{self, Sampling, union},
};
use strata_pixels::CoverageRect;
fn invalid(message: &str) -> CommandError {
    CommandError::Invalid(message.into())
}
fn extend(
    current: &mut Option<CoverageRect>,
    next: Option<CoverageRect>,
) -> Result<(), DocumentError> {
    if let Some(next) = next {
        *current = Some(match *current {
            Some(old) => union(old, next)?,
            None => next,
        })
    }
    Ok(())
}
fn subtree<'a>(doc: &'a Document, roots: &'a [LayerId]) -> impl Iterator<Item = usize> + 'a {
    doc.layers
        .iter()
        .enumerate()
        .filter(move |(_, l)| {
            roots
                .iter()
                .any(|&root| l.id() == root || doc.is_descendant(l.id(), root))
        })
        .map(|(i, _)| i)
}
pub(super) fn validate(doc: &Document, id: LayerId) -> Result<(), CommandError> {
    for i in subtree(doc, &[id]) {
        let info = doc.layers[i].info();
        let locks = info.locks;
        if locks.all || locks.pixels || locks.position || locks.transparency {
            return Err(invalid(
                "unlock all affected subtree pixel/position/transparency/all locks before group transform",
            ));
        }
    }
    Ok(())
}
/// Query only visible child content, so a group bound never discloses hidden
/// descendant geometry. All editable leaf kinds have an explicit policy here.
pub(super) fn visible_bounds(
    doc: &Document,
    id: LayerId,
    cancel: &dyn Fn() -> bool,
) -> Result<Option<CoverageRect>, DocumentError> {
    visible_bounds_of(doc, &[id], cancel)
}

pub(super) fn visible_bounds_of(
    doc: &Document,
    roots: &[LayerId],
    cancel: &dyn Fn() -> bool,
) -> Result<Option<CoverageRect>, DocumentError> {
    let mut result = None;
    let mut visits = 0u64;
    for i in subtree(doc, roots) {
        if cancel() {
            return Err(DocumentError::Cancelled);
        }
        let layer = &doc.layers[i];
        if !doc.effective_visibility(layer.id()) {
            continue;
        }
        visits += layer.pixels().tile_count() as u64 * 256 * 256;
        if visits > 64_000_000 {
            return Err(DocumentError::ResourceLimit);
        }
        let bounds = match layer.info.kind {
            LayerKind::Pixel | LayerKind::Text { .. } => {
                super::transform::bounds(doc, layer, cancel)?
            }
            LayerKind::Solid { .. } | LayerKind::Gradient { .. } => {
                doc.generated_canvas_bounds(layer)?
            }
            LayerKind::Group
            | LayerKind::Exposure { .. }
            | LayerKind::HueSaturation { .. }
            | LayerKind::Adjustment { .. } => None,
        };
        extend(&mut result, bounds)?;
    }
    Ok(result)
}
struct Entry {
    index: usize,
    bounds: Option<CoverageRect>,
    origin: [i64; 2],
}
fn mask_offset(layer: &crate::Layer) -> Result<[i64; 2], DocumentError> {
    let meta = layer.info.mask.as_ref().unwrap();
    Ok([
        layer
            .info
            .x
            .checked_sub(meta.anchor_x)
            .ok_or(DocumentError::ResourceLimit)?,
        layer
            .info
            .y
            .checked_sub(meta.anchor_y)
            .ok_or(DocumentError::ResourceLimit)?,
    ])
}
fn checked_position(
    transform: &Homography,
    layer: &crate::Layer,
) -> Result<[i64; 2], CommandError> {
    let p = transform
        .map([layer.info.x as f64, layer.info.y as f64])
        .map_err(|e| CommandError::Invalid(e.to_string()))?;
    Ok([p[0].floor() as i64, p[1].floor() as i64])
}
pub(super) fn apply(
    doc: &mut Document,
    target: LayerId,
    transform: &Homography,
    interpolation: TransformInterpolation,
    cancel: &dyn Fn() -> bool,
) -> Result<Value, CommandError> {
    validate(doc, target)?;
    if !doc.effective_visibility(target) {
        return Err(invalid(
            "group transform needs a visible group and ancestors",
        ));
    }
    let result = apply_roots(doc, &[target], transform, interpolation, cancel)?;
    Ok(serde_json::json!({
        "layer_id":target,"scope":"whole_layer","output_bounds":result.output_bounds,
        "source_artboard_clipped":false,"mask_transformed":result.mask_transformed,
        "selection_transformed":false,"changed":result.changed
    }))
}

pub(super) fn apply_roots(
    doc: &mut Document,
    roots: &[LayerId],
    transform: &Homography,
    interpolation: TransformInterpolation,
    cancel: &dyn Fn() -> bool,
) -> Result<TransformLayersResult, CommandError> {
    let source = doc.clone();
    let translation = transform.integer_translation();
    let sampling = match interpolation {
        TransformInterpolation::Nearest => Sampling::Nearest,
        TransformInterpolation::Bilinear => Sampling::Bilinear,
    };
    let mut entries = Vec::new();
    let mut input_visits = 0u64;
    let mut output_visits = 0u64;
    let mut visible = None;
    let mut masks = false;
    for index in subtree(&source, roots) {
        if cancel() {
            return Err(CommandError::Expired);
        }
        let layer = &source.layers[index];
        input_visits += layer.pixels().tile_count() as u64 * 256 * 256;
        if input_visits > 64_000_000 {
            return Err(DocumentError::ResourceLimit.into());
        }
        let bounds =
            if matches!(layer.info.kind, LayerKind::Group) || layer.info.kind.is_adjustment() {
                None
            } else {
                super::transform::bounds(&source, layer, cancel)?
            };
        let origin = if let Some(bounds) = bounds {
            let output = geometry_pixels::transformed_bounds(bounds, transform, sampling)?;
            if translation.is_none()
                && matches!(layer.info.kind, LayerKind::Pixel | LayerKind::Text { .. })
            {
                output_visits += u64::from(output.width) * u64::from(output.height);
            }
            let logical =
                geometry_pixels::transformed_bounds(bounds, transform, Sampling::Nearest)?;
            [logical.x, logical.y]
        } else {
            [layer.info.x, layer.info.y]
        };
        if source.effective_visibility(layer.id()) {
            extend(&mut visible, bounds)?;
        }
        if layer.mask.is_some() || layer.info.vector_mask.as_ref().is_some_and(|m| m.linked) {
            masks |= source.effective_visibility(layer.id());
        }
        if let Some(mask) = layer.mask.as_ref().filter(|_| translation.is_none()) {
            let [dx, dy] = mask_offset(layer)?;
            let mut b = mask.layout().bounds;
            b.x = b.x.checked_add(dx).ok_or(DocumentError::ResourceLimit)?;
            b.y = b.y.checked_add(dy).ok_or(DocumentError::ResourceLimit)?;
            let output = geometry_pixels::transformed_bounds(b, transform, sampling)?;
            output_visits += u64::from(output.width) * u64::from(output.height);
        }
        if output_visits > 64_000_000 {
            return Err(DocumentError::ResourceLimit.into());
        }
        entries.push(Entry {
            index,
            bounds,
            origin,
        });
    }
    let visible = visible.ok_or_else(|| {
        invalid("layers have no visible finite pixel, text or fill content to transform")
    })?;
    let output_bounds = geometry_pixels::transformed_bounds(visible, transform, sampling)?;
    let result = |changed| TransformLayersResult {
        layer_ids: roots.to_vec(),
        output_bounds,
        mask_transformed: masks,
        changed,
    };
    if let Some([dx, dy]) = translation {
        if dx != 0 || dy != 0 {
            for &target in roots {
                let root = source.layers.iter().find(|l| l.id() == target).unwrap();
                super::tree::move_position(
                    doc,
                    target,
                    root.info
                        .x
                        .checked_add(dx)
                        .ok_or(DocumentError::ResourceLimit)?,
                    root.info
                        .y
                        .checked_add(dy)
                        .ok_or(DocumentError::ResourceLimit)?,
                )?;
            }
        }
        if cancel() {
            return Err(CommandError::Expired);
        }
        return Ok(result(dx != 0 || dy != 0));
    }
    // Work out all storage anchors before any output is sampled. Source boxes
    // are cached from the one bounded scan; groups do not rescan nested pixels.
    for i in 0..entries.len() {
        let layer = &source.layers[entries[i].index];
        if matches!(layer.info.kind, LayerKind::Group) {
            let mut extent = None;
            for child in &entries {
                let other = &source.layers[child.index];
                if source.is_descendant(other.id(), layer.id())
                    && (!source.effective_visibility(layer.id())
                        || source.effective_visibility(other.id()))
                {
                    extend(&mut extent, child.bounds)?
                }
            }
            entries[i].origin = if let Some(bounds) = extent {
                let logical =
                    geometry_pixels::transformed_bounds(bounds, transform, Sampling::Nearest)?;
                [logical.x, logical.y]
            } else {
                checked_position(transform, layer)?
            };
        } else if entries[i].bounds.is_none() {
            entries[i].origin = checked_position(transform, layer)?
        }
    }
    for entry in entries {
        if cancel() {
            return Err(CommandError::Expired);
        }
        let original = &source.layers[entry.index];
        if !matches!(original.info.kind, LayerKind::Group)
            && let Some(bounds) = entry.bounds
        {
            super::transform::apply_leaf(
                doc,
                entry.index,
                transform,
                interpolation,
                bounds,
                cancel,
            )?;
            continue;
        }
        let mask = original
            .mask
            .as_ref()
            .map(|mask| {
                crate::geometry_masks::transform_mask(
                    mask,
                    mask_offset(original)?,
                    transform,
                    sampling,
                    source.memory_pool(),
                    cancel,
                )
            })
            .transpose()?;
        let text = if matches!(original.info.kind, LayerKind::Text { .. }) {
            Some(crate::typography::transformed_layer(
                original,
                transform,
                interpolation,
                entry.origin,
                source.format,
                source.memory_pool(),
                cancel,
            )?)
        } else {
            None
        };
        let layer = &mut doc.layers[entry.index];
        layer.info.x = entry.origin[0];
        layer.info.y = entry.origin[1];
        if let Some(text) = text {
            layer.pixels = text.pixels;
            layer.info.kind = text.info.kind;
            layer.info.generated_crop = None;
        }
        if let Some(mask) = mask {
            super::local::attach(layer, mask);
            layer.info.mask.as_mut().unwrap().enabled =
                original.info.mask.as_ref().unwrap().enabled;
        }
        layer
            .info
            .transform_bindings(&original.info, transform, false)?;
        GeneratedGeometry::update_reference(
            &mut layer.info,
            &original.info,
            &source.frame,
            transform,
        )?;
    }
    if cancel() {
        return Err(CommandError::Expired);
    }
    Ok(result(true))
}
