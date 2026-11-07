//! Whole-document signed-axis permutations keep source codes and editable
//! layers. This is not flattening and never clips content to the old artboard.
use super::*;
use crate::{
    Document, DocumentError, LayerKind,
    geometry::Homography,
    geometry_pixels::{self, PixelSource, Sampling},
};
use strata_pixels::CoverageRect;
fn check(cancel: &dyn Fn() -> bool) -> Result<(), CommandError> {
    if cancel() {
        Err(CommandError::Expired)
    } else {
        Ok(())
    }
}
fn invalid(e: impl std::fmt::Display) -> CommandError {
    CommandError::Invalid(e.to_string())
}
fn angle(value: f64, coefficients: [f64; 4]) -> f64 {
    let (s, c) = value.to_radians().sin_cos();
    let [a, b, cc, d] = coefficients;
    (b * c + d * s).atan2(a * c + cc * s).to_degrees()
}
pub(super) fn mutate(
    doc: &mut Document,
    id: &str,
    params: Value,
    cancel: &dyn Fn() -> bool,
) -> Result<Option<Value>, CommandError> {
    if id != "image.orient" {
        return Ok(None);
    }
    check(cancel)?;
    let p: OrientImage = parse(params)?;
    if doc.layers.iter().any(|l| {
        let lock = l.info().locks;
        lock.all || lock.pixels || lock.position || lock.transparency
    }) {
        return Err(invalid(
            "unlock all affected layer pixel/position/transparency/all locks before orienting the whole document",
        ));
    }
    let before = doc.frame.clone();
    let x = before.x as f64;
    let y = before.y as f64;
    let w = f64::from(before.width);
    let h = f64::from(before.height);
    let (m, quarter) = match p.operation {
        ImageOrientation::Rotate90Cw => ([0., 1., -1., 0., x + y + h, y - x], true),
        ImageOrientation::Rotate90Ccw => ([0., -1., 1., 0., x - y, x + y + w], true),
        ImageOrientation::Rotate180 => ([-1., 0., 0., -1., 2. * x + w, 2. * y + h], false),
        ImageOrientation::FlipHorizontal => ([-1., 0., 0., 1., 2. * x + w, 0.], false),
        ImageOrientation::FlipVertical => ([1., 0., 0., -1., 0., 2. * y + h], false),
    };
    let transform = Homography::affine(m).map_err(invalid)?;
    if !transform.preserves_lattice() {
        return Err(invalid(
            "image orientation requires an exactly representable integer canvas inside ±1e12",
        ));
    }
    geometry_pixels::transformed_bounds(
        CoverageRect {
            x: before.x,
            y: before.y,
            width: before.width,
            height: before.height,
        },
        &transform,
        Sampling::Nearest,
    )?;
    let coefficients = [m[0], m[1], m[2], m[3]];
    super::canvas::freeze_references(doc);
    let source = doc.clone();
    let pool = doc.memory_pool();
    let transform_mask = |mask: &strata_pixels::TiledMask, offset| {
        crate::geometry_masks::transform_mask(
            mask,
            offset,
            &transform,
            Sampling::Nearest,
            pool.clone(),
            cancel,
        )
    };
    let selection = source
        .selection
        .as_ref()
        .map(|mask| transform_mask(mask, [0, 0]))
        .transpose()?;
    let previous = source
        .previous_selection
        .as_ref()
        .map(|mask| transform_mask(mask, [0, 0]))
        .transpose()?;
    for (index, old) in source.layers.iter().enumerate() {
        check(cancel)?;
        let info = old.info();
        let offset = [info.x, info.y];
        // Exhaustive on purpose: a new editable layer kind must explicitly
        // implement orientation instead of silently keeping its old payload.
        let has_pixels = match &info.kind {
            LayerKind::Text { .. } => false,
            LayerKind::Pixel => true,
            LayerKind::Group
            | LayerKind::Solid { .. }
            | LayerKind::Gradient { .. }
            | LayerKind::Exposure { .. }
            | LayerKind::HueSaturation { .. }
            | LayerKind::Adjustment { .. } => false,
        };
        let pixels = if has_pixels {
            geometry_pixels::content_bounds(old.pixels(), offset, cancel)?
        } else {
            None
        };
        let reference = info.generated_size.unwrap_or([before.width, before.height]);
        let reference_bound = CoverageRect {
            x: info.x,
            y: info.y,
            width: reference[0],
            height: reference[1],
        };
        let reference_origin = if matches!(
            info.kind,
            LayerKind::Solid { .. } | LayerKind::Gradient { .. }
        ) || info.generated_size.is_some()
        {
            let b = geometry_pixels::transformed_bounds(
                reference_bound,
                &transform,
                Sampling::Nearest,
            )?;
            [b.x, b.y]
        } else if let Some(b) = pixels {
            let b = geometry_pixels::transformed_bounds(b, &transform, Sampling::Nearest)?;
            [b.x, b.y]
        } else {
            let position = transform
                .map([info.x as f64, info.y as f64])
                .map_err(invalid)?;
            [position[0].round() as i64, position[1].round() as i64]
        };
        let output = pixels
            .map(|bounds| {
                geometry_pixels::transform_pixels_at(
                    PixelSource {
                        pixels: old.pixels(),
                        offset,
                        bounds,
                        icc: source.working_icc(),
                    },
                    &transform,
                    Sampling::Nearest,
                    reference_origin,
                    doc.memory_pool(),
                    cancel,
                )
            })
            .transpose()?;
        let mask = if let (Some(mask), Some(meta)) = (&old.mask, &info.mask) {
            Some(transform_mask(
                mask,
                [
                    info.x
                        .checked_sub(meta.anchor_x)
                        .ok_or(DocumentError::ResourceLimit)?,
                    info.y
                        .checked_sub(meta.anchor_y)
                        .ok_or(DocumentError::ResourceLimit)?,
                ],
            )?)
        } else {
            None
        };
        let spatial = info
            .spatial_bounds
            .map(|b| geometry_pixels::transformed_bounds(b, &transform, Sampling::Nearest))
            .transpose()?;
        let generated_crop = info
            .generated_crop
            .as_ref()
            .map(|crop| {
                let bounds = crop
                    .bounds
                    .map(|mut b| {
                        b.x =
                            b.x.checked_add(info.x)
                                .ok_or(DocumentError::ResourceLimit)?;
                        b.y =
                            b.y.checked_add(info.y)
                                .ok_or(DocumentError::ResourceLimit)?;
                        let mut transformed =
                            geometry_pixels::transformed_bounds(b, &transform, Sampling::Nearest)?;
                        transformed.x = transformed
                            .x
                            .checked_sub(reference_origin[0])
                            .ok_or(DocumentError::ResourceLimit)?;
                        transformed.y = transformed
                            .y
                            .checked_sub(reference_origin[1])
                            .ok_or(DocumentError::ResourceLimit)?;
                        Ok::<_, DocumentError>(transformed)
                    })
                    .transpose()?;
                Ok::<_, DocumentError>(crate::GeneratedCrop { bounds })
            })
            .transpose()?;
        check(cancel)?;
        let text = if matches!(info.kind, LayerKind::Text { .. }) {
            Some(crate::typography::transformed_layer(
                old,
                &transform,
                TransformInterpolation::Nearest,
                reference_origin,
                doc.format,
                pool.clone(),
                cancel,
            )?)
        } else {
            None
        };
        let layer = &mut doc.layers[index];
        if let Some(output) = output {
            layer.pixels = output;
        }
        if let Some(text) = text {
            layer.pixels = text.pixels;
            layer.info.kind = text.info.kind;
        }
        layer.info.x = reference_origin[0];
        layer.info.y = reference_origin[1];
        if info.generated_size.is_some()
            || matches!(
                info.kind,
                LayerKind::Solid { .. } | LayerKind::Gradient { .. }
            )
        {
            layer.info.generated_size = Some(if quarter {
                [reference[1], reference[0]]
            } else {
                reference
            });
        }
        layer.info.spatial_bounds = spatial;
        layer.info.generated_crop = if matches!(info.kind, LayerKind::Text { .. }) {
            None
        } else {
            generated_crop
        };
        if let LayerKind::Gradient {
            angle: direction, ..
        } = &mut layer.info.kind
        {
            *direction = angle(*direction, coefficients);
        }
        if let Some(overlay) = &mut layer.info.styles.gradient_overlay {
            overlay.angle = angle(overlay.angle, coefficients);
        }
        if let Some(shadow) = &mut layer.info.styles.drop_shadow {
            let dx = f64::from(shadow.offset_x);
            let dy = f64::from(shadow.offset_y);
            shadow.offset_x = (m[0] * dx + m[2] * dy) as i32;
            shadow.offset_y = (m[1] * dx + m[3] * dy) as i32;
        }
        if let Some(mask) = mask {
            let enabled = info.mask.as_ref().unwrap().enabled;
            super::local::attach(layer, mask);
            layer.info.mask.as_mut().unwrap().enabled = enabled;
        }
    }
    check(cancel)?;
    doc.selection = selection;
    doc.previous_selection = previous;
    if quarter {
        doc.frame.width = before.height;
        doc.frame.height = before.width;
    }
    Ok(Some(
        serde_json::json!({"operation":p.operation,"previous_frame":before,"frame":doc.frame,"all_stored_content_transformed":true,"preserved_layer_structure":true}),
    ))
}
