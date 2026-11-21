//! Projective transforms of editable layers, finite groups and selected pixels.
use super::*;
use crate::{
    Document, DocumentError, GeneratedGeometry, LayerKind,
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
    if matches!(doc.layers()[index].info.kind, LayerKind::Group) {
        super::transform_group::validate(doc, target)?;
    }
    Ok((target, index))
}

/// Pick the outermost selected roots in stable document order. An explicitly
/// selected descendant travels with its group and is transformed only once.
pub(super) fn roots(
    doc: &Document,
    layers: &[LayerRef],
) -> Result<Vec<crate::LayerId>, CommandError> {
    if !(2..=64).contains(&layers.len()) {
        return Err(invalid("select between 2 and 64 layers"));
    }
    let mut selected = std::collections::HashSet::with_capacity(layers.len());
    for layer in layers {
        if !selected.insert(doc.resolve(layer)?) {
            return Err(invalid("transform layer references must be unique"));
        }
    }
    let roots: Vec<_> = doc
        .layers()
        .iter()
        .map(crate::Layer::id)
        .filter(|&id| {
            selected.contains(&id)
                && !selected
                    .iter()
                    .any(|&other| other != id && doc.is_descendant(id, other))
        })
        .collect();
    for &id in &roots {
        target(doc, &LayerRef::Id(id))?;
    }
    Ok(roots)
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
            bounds(doc, layer, cancel)?
        }
        TransformScope::Selection => {
            if !matches!(doc.layers()[index].info().kind, LayerKind::Pixel) {
                return Err(invalid(
                    "selection transform requires pixels; explicitly rasterize text/group or choose whole_layer",
                ));
            }
            let selection = doc
                .selection()
                .ok_or_else(|| invalid("select pixels before canvas transform"))?;
            crate::geometry_masks::coverage_bounds(selection, cancel)?
        }
    };
    Ok(LayerBoundsResult { layer_id, bounds })
}
pub(super) fn bounds(
    doc: &Document,
    layer: &crate::Layer,
    cancel: &dyn Fn() -> bool,
) -> Result<Option<strata_pixels::CoverageRect>, DocumentError> {
    let info = layer.info();
    match info.kind {
        LayerKind::Group => return super::transform_group::visible_bounds(doc, layer.id(), cancel),
        LayerKind::Solid { .. } | LayerKind::Gradient { .. } => {
            return doc.generated_canvas_bounds(layer);
        }
        LayerKind::Exposure { .. }
        | LayerKind::HueSaturation { .. }
        | LayerKind::Adjustment { .. } => {
            if let Some((mask, metadata)) = layer.mask.as_ref().zip(info.mask.as_ref()) {
                let Some(mut bounds) = crate::geometry_masks::coverage_bounds(mask, cancel)? else {
                    return Ok(None);
                };
                bounds.x = i64::try_from(
                    i128::from(bounds.x) + i128::from(info.x) - i128::from(metadata.anchor_x),
                )
                .map_err(|_| DocumentError::ResourceLimit)?;
                bounds.y = i64::try_from(
                    i128::from(bounds.y) + i128::from(info.y) - i128::from(metadata.anchor_y),
                )
                .map_err(|_| DocumentError::ResourceLimit)?;
                bounds.validate()?;
                return Ok(Some(bounds));
            }
            if let Some(mask) = &info.vector_mask {
                if mask.inverted {
                    let f = doc.frame();
                    return Ok(Some(strata_pixels::CoverageRect {
                        x: f.x,
                        y: f.y,
                        width: f.width,
                        height: f.height,
                    }));
                }
                let contours = mask.contours(doc, info)?;
                let mut min = [f64::INFINITY; 2];
                let mut max = [f64::NEG_INFINITY; 2];
                for p in contours.iter().flatten() {
                    for k in 0..2 {
                        min[k] = min[k].min(p[k]);
                        max[k] = max[k].max(p[k]);
                    }
                }
                let x = min[0].floor() as i64;
                let y = min[1].floor() as i64;
                let w = u32::try_from(max[0].ceil() as i64 - x)
                    .map_err(|_| DocumentError::ResourceLimit)?;
                let h = u32::try_from(max[1].ceil() as i64 - y)
                    .map_err(|_| DocumentError::ResourceLimit)?;
                return Ok((w > 0 && h > 0).then_some(strata_pixels::CoverageRect {
                    x,
                    y,
                    width: w,
                    height: h,
                }));
            }

            return Ok(None);
        }
        LayerKind::Pixel | LayerKind::Text { .. } => {}
    }
    let bounds =
        geometry_pixels::content_bounds(layer.pixels(), [layer.info().x, layer.info().y], cancel)?;
    if bounds.is_some() {
        return Ok(bounds);
    }
    let LayerKind::Text { text } = &layer.info().kind else {
        return Ok(None);
    };
    crate::typography::ink_bounds(
        text,
        doc.format,
        layer.info().generated_crop.as_ref(),
        doc.memory_pool(),
        cancel,
    )?
    .map(|mut b| {
        b.x =
            b.x.checked_add(layer.info().x)
                .ok_or(DocumentError::ResourceLimit)?;
        b.y =
            b.y.checked_add(layer.info().y)
                .ok_or(DocumentError::ResourceLimit)?;
        Ok(b)
    })
    .transpose()
}
pub(super) fn mutate(
    doc: &mut Document,
    id: &str,
    params: Value,
    cancel: &dyn Fn() -> bool,
) -> Result<Option<Value>, CommandError> {
    if id == "transform.apply_layers" {
        if cancel() {
            return Err(CommandError::Expired);
        }
        let p: TransformLayers = parse(params)?;
        let transform = matrix(&p.geometry)?;
        let roots = roots(doc, &p.layers)?;
        let result =
            super::transform_group::apply_roots(doc, &roots, &transform, p.interpolation, cancel)?;
        return Ok(Some(serde_json::json!(result)));
    }
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
    if matches!(p.scope, TransformScope::Selection) {
        if !matches!(layer.info().kind, LayerKind::Pixel) {
            return Err(invalid(
                "selection transform requires pixels; explicitly rasterize text/group or choose whole_layer",
            ));
        }
        return super::transform_selection::apply(
            doc,
            index,
            &transform,
            match p.interpolation {
                TransformInterpolation::Nearest => Sampling::Nearest,
                TransformInterpolation::Bilinear => Sampling::Bilinear,
            },
            cancel,
        )
        .map(Some)
        .map_err(CommandError::from);
    }
    if matches!(layer.info.kind, LayerKind::Group) {
        return super::transform_group::apply(doc, target, &transform, p.interpolation, cancel)
            .map(Some);
    }
    let bounds = bounds(doc, &layer, cancel)?
        .ok_or_else(|| invalid("layer has no finite content or mask coverage to transform"))?;
    apply_leaf(doc, index, &transform, p.interpolation, bounds, cancel).map(Some)
}
pub(super) fn apply_leaf(
    doc: &mut Document,
    index: usize,
    transform: &Homography,
    interpolation: TransformInterpolation,
    bounds: strata_pixels::CoverageRect,
    cancel: &dyn Fn() -> bool,
) -> Result<Value, CommandError> {
    let layer = doc.layers()[index].clone();
    let target = layer.id();
    let offset = [layer.info().x, layer.info().y];
    let sampling = match interpolation {
        TransformInterpolation::Nearest => Sampling::Nearest,
        TransformInterpolation::Bilinear => Sampling::Bilinear,
    };
    let output_bounds = geometry_pixels::transformed_bounds(bounds, transform, sampling)?;
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
        return Ok(
            serde_json::json!({"layer_id":target,"scope":"whole_layer","output_bounds":output_bounds,"source_artboard_clipped":false,"mask_transformed":layer.mask.is_some() || layer.info().vector_mask.as_ref().is_some_and(|m|m.linked),"selection_transformed":false,"changed":dx!=0 || dy!=0}),
        );
    }
    let logical = geometry_pixels::transformed_bounds(bounds, transform, Sampling::Nearest)?;
    let origin = [logical.x, logical.y];
    let text = if matches!(layer.info().kind, LayerKind::Text { .. }) {
        Some(crate::typography::transformed_layer(
            &layer,
            transform,
            interpolation,
            origin,
            doc.format,
            doc.memory_pool(),
            cancel,
        )?)
    } else {
        None
    };
    let output = if matches!(layer.info.kind, LayerKind::Pixel) {
        Some(geometry_pixels::transform_pixels_at(
            PixelSource {
                pixels: layer.pixels(),
                offset,
                bounds,
                icc: doc.working_icc(),
            },
            transform,
            sampling,
            origin,
            doc.memory_pool(),
            cancel,
        )?)
    } else {
        None
    };
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
            transform,
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
    if let Some(output) = output {
        doc.layers[index].pixels = output;
    }
    if let Some(text) = text {
        doc.layers[index].pixels = text.pixels;
        doc.layers[index].info.kind = text.info.kind;
        doc.layers[index].info.generated_crop = None;
    }
    doc.layers[index].info.x = origin[0];
    doc.layers[index].info.y = origin[1];
    doc.layers[index]
        .info
        .transform_bindings(&layer.info, transform, false)?;
    GeneratedGeometry::update_reference(
        &mut doc.layers[index].info,
        &layer.info,
        &doc.frame,
        transform,
    )?;
    if let Some(mask) = mask {
        let enabled = layer.info().mask.as_ref().is_some_and(|m| m.enabled);
        super::local::attach(&mut doc.layers[index], mask);
        doc.layers[index].info.mask.as_mut().unwrap().enabled = enabled;
    }
    let output_bounds = if matches!(layer.info().kind, LayerKind::Text { .. }) {
        geometry_pixels::content_bounds(
            doc.layers[index].pixels(),
            [doc.layers[index].info.x, doc.layers[index].info.y],
            cancel,
        )?
        .unwrap_or(output_bounds)
    } else {
        output_bounds
    };
    Ok(
        serde_json::json!({"layer_id":target,"scope":"whole_layer","output_bounds":output_bounds,"source_artboard_clipped":false,"mask_transformed":layer.mask.is_some() || layer.info().vector_mask.as_ref().is_some_and(|m|m.linked),"selection_transformed":false,"changed":true}),
    )
}
