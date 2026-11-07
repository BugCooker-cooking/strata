//! Pixel-layer/selected-pixel projective transforms. Editable generated/group
//! transforms remain separate work, never silent rasterization.
use super::*;
use crate::{
    Document, DocumentError, LayerKind,
    geometry::Homography,
    geometry_pixels::{self, PixelSource, Sampling},
};
fn invalid(e: impl std::fmt::Display) -> CommandError {
    CommandError::Invalid(e.to_string())
}
fn matrix(spec: &TransformGeometry) -> Result<Homography, CommandError> {
    let result = match spec {
        TransformGeometry::Matrix { values } => Homography::new(*values),
        TransformGeometry::Affine { values } => Homography::affine(*values),
        TransformGeometry::Corners {
            source,
            destination,
        } => Homography::between(*source, *destination).and_then(|value| {
            if source == destination {
                Homography::affine([1., 0., 0., 1., 0., 0.])
            } else {
                Ok(value)
            }
        }),
        TransformGeometry::Components {
            translation,
            scale,
            rotation_degrees,
            shear_degrees,
            pivot,
        } => {
            if translation
                .iter()
                .chain(scale)
                .chain(shear_degrees)
                .chain(pivot)
                .chain([rotation_degrees])
                .any(|v| !v.is_finite() || v.abs() > 1e12)
                || scale.iter().any(|v| v.abs() < 1e-6 || v.abs() > 1e4)
                || shear_degrees.iter().any(|v| v.abs() >= 89.0)
            {
                return Err(invalid(
                    "transform components need finite positions ±1e12, absolute scale 1e-6..1e4 and shear strictly inside ±89 degrees",
                ));
            }
            let angle = rotation_degrees.rem_euclid(360.0);
            let (c, s) = match angle {
                0.0 => (1.0, 0.0),
                90.0 => (0.0, 1.0),
                180.0 => (-1.0, 0.0),
                270.0 => (0.0, -1.0),
                _ => (angle.to_radians().cos(), angle.to_radians().sin()),
            };
            let sx = shear_degrees[0].to_radians().tan();
            let sy = shear_degrees[1].to_radians().tan();
            // Order: scale, shear [1,sx;sy,1], rotation, translation.
            let a = scale[0] * (c - s * sy);
            let b = scale[0] * (s + c * sy);
            let d = scale[1] * (c + s * sx);
            let cc = scale[1] * (c * sx - s);
            Homography::affine([
                a,
                b,
                cc,
                d,
                pivot[0] + translation[0] - a * pivot[0] - cc * pivot[1],
                pivot[1] + translation[1] - b * pivot[0] - d * pivot[1],
            ])
        }
    };
    result.map_err(invalid)
}
pub(super) fn target(
    doc: &Document,
    layer: &LayerRef,
) -> Result<(crate::LayerId, usize), CommandError> {
    let target = doc.resolve(layer)?;
    let index = doc.layers().iter().position(|l| l.id() == target).unwrap();
    if !matches!(doc.layers()[index].info().kind, LayerKind::Pixel) {
        return Err(invalid(
            "transform currently requires a pixel layer; generated/group/text layers are not implicitly rasterized",
        ));
    }
    if doc.layers().iter().any(|l| {
        (l.id() == target || doc.is_descendant(target, l.id()))
            && (l.info().locks.all
                || l.info().locks.pixels
                || l.info().locks.position
                || l.info().locks.transparency)
    }) {
        return Err(invalid(
            "unlock pixels, position, transparency and locked ancestors before transform",
        ));
    }
    Ok((target, index))
}
pub(super) fn canvas_bounds(
    doc: &Document,
    layer: &LayerRef,
    scope: TransformScope,
    cancel: &dyn Fn() -> bool,
) -> Result<LayerBoundsResult, CommandError> {
    let (layer_id, index) = target(doc, layer)?;
    if !doc.effective_visibility(layer_id) {
        return Err(invalid(
            "canvas transform needs a visible layer and visible ancestors",
        ));
    }
    let bounds = match scope {
        TransformScope::WholeLayer => {
            let layer = &doc.layers()[index];
            // Bound this interactive initialization independently of the later
            // resampling budget. Include stored alpha-zero RGB/offboard codes.
            if layer.pixels().tile_count() as u64 * 256 * 256 > 64_000_000 {
                return Err(DocumentError::ResourceLimit.into());
            }
            geometry_pixels::content_bounds(
                layer.pixels(),
                [layer.info().x, layer.info().y],
                cancel,
            )?
        }
        TransformScope::Selection => {
            let selection = doc
                .selection()
                .ok_or_else(|| invalid("select pixels before canvas transform"))?;
            crate::geometry_masks::coverage_bounds(selection, cancel)?
        }
    };
    Ok(LayerBoundsResult { layer_id, bounds })
}
pub(super) fn mutate(
    doc: &mut Document,
    id: &str,
    params: Value,
    cancel: &dyn Fn() -> bool,
) -> Result<Option<Value>, CommandError> {
    if id != "transform.apply" {
        return Ok(None);
    }
    if cancel() {
        return Err(CommandError::Expired);
    }
    let p: TransformApply = parse(params)?;
    let transform = matrix(&p.geometry)?;
    let (target, index) = target(doc, &p.layer)?;
    let layer = doc.layers()[index].clone();
    let sampling = match p.interpolation {
        TransformInterpolation::Nearest => Sampling::Nearest,
        TransformInterpolation::Bilinear => Sampling::Bilinear,
    };
    let offset = [layer.info().x, layer.info().y];
    if matches!(p.scope, TransformScope::Selection) {
        return super::transform_selection::apply(doc, index, &transform, sampling, cancel)
            .map(Some)
            .map_err(CommandError::from);
    }
    let bounds = geometry_pixels::content_bounds(layer.pixels(), offset, cancel)?
        .ok_or_else(|| invalid("pixel layer is empty; there is no geometry to transform"))?;
    let output_bounds = geometry_pixels::transformed_bounds(bounds, &transform, sampling)?;
    if let Some([dx, dy]) = transform.integer_translation() {
        let x = offset[0]
            .checked_add(dx)
            .ok_or(DocumentError::ResourceLimit)?;
        let y = offset[1]
            .checked_add(dy)
            .ok_or(DocumentError::ResourceLimit)?;
        if cancel() {
            return Err(CommandError::Expired);
        }
        super::tree::move_position(doc, target, x, y)?;
        return Ok(Some(
            serde_json::json!({"layer_id":target,"scope":"whole_layer","output_bounds":output_bounds,"source_artboard_clipped":false,"mask_transformed":layer.mask.is_some(),"selection_transformed":false,"changed":dx!=0 || dy!=0}),
        ));
    }
    let logical = geometry_pixels::transformed_bounds(bounds, &transform, Sampling::Nearest)?;
    let origin = [logical.x, logical.y];
    let output = geometry_pixels::transform_pixels_at(
        PixelSource {
            pixels: layer.pixels(),
            offset,
            bounds,
            icc: doc.working_icc(),
        },
        &transform,
        sampling,
        origin,
        doc.memory_pool(),
        cancel,
    )?;
    let mask = if let (Some(mask), Some(info)) = (&layer.mask, &layer.info().mask) {
        let offset = [
            layer
                .info()
                .x
                .checked_sub(info.anchor_x)
                .ok_or(DocumentError::ResourceLimit)?,
            layer
                .info()
                .y
                .checked_sub(info.anchor_y)
                .ok_or(DocumentError::ResourceLimit)?,
        ];
        Some(crate::geometry_masks::transform_mask(
            mask,
            offset,
            &transform,
            sampling,
            doc.memory_pool(),
            cancel,
        )?)
    } else {
        None
    };
    if cancel() {
        return Err(CommandError::Expired);
    }
    doc.layers[index].pixels = output;
    doc.layers[index].info.x = origin[0];
    doc.layers[index].info.y = origin[1];
    if let Some(mask) = mask {
        let enabled = layer.info().mask.as_ref().is_some_and(|m| m.enabled);
        super::local::attach(&mut doc.layers[index], mask);
        doc.layers[index].info.mask.as_mut().unwrap().enabled = enabled;
    }
    Ok(Some(
        serde_json::json!({"layer_id":target,"scope":"whole_layer","output_bounds":output_bounds,"source_artboard_clipped":false,"mask_transformed":layer.mask.is_some(),"selection_transformed":false,"changed":true}),
    ))
}
