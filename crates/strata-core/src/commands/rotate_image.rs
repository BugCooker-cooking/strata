//! Arbitrary rotation for pixel layers and unstyled groups. Unsupported editable
//! types are rejected before sampling; quarter turns reuse full-type isometries.
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
pub(super) fn layout(
    old: &crate::Frame,
    angle: f64,
) -> Result<(Homography, crate::Frame), CommandError> {
    let theta = angle.to_radians();
    let (c, s) = (theta.cos(), theta.sin());
    let cx = old.x as f64 + f64::from(old.width) / 2.;
    let cy = old.y as f64 + f64::from(old.height) / 2.;
    let tx = cx - c * cx + s * cy;
    let ty = cy - s * cx - c * cy;
    let base = Homography::affine([c, s, -s, c, tx, ty]).map_err(invalid)?;
    // A conservative half-pixel reconstruction envelope, identical for the
    // nearest and bilinear choices, encloses antialias support after rotation.
    let extent = geometry_pixels::transformed_bounds(
        CoverageRect {
            x: old.x,
            y: old.y,
            width: old.width,
            height: old.height,
        },
        &base,
        Sampling::Bilinear,
    )?;
    let transform = Homography::affine([
        c,
        s,
        -s,
        c,
        tx + (old.x as f64 - extent.x as f64),
        ty + (old.y as f64 - extent.y as f64),
    ])
    .map_err(invalid)?;
    let frame = crate::Frame {
        name: old.name.clone(),
        x: old.x,
        y: old.y,
        width: extent.width,
        height: extent.height,
    };
    if [
        frame.x as f64,
        frame.y as f64,
        frame.x as f64 + f64::from(frame.width),
        frame.y as f64 + f64::from(frame.height),
    ]
    .iter()
    .any(|v| v.abs() > 1e12)
    {
        return Err(invalid(
            "expanded image frame exceeds canvas coordinates +/-1e12",
        ));
    }
    Ok((transform, frame))
}
pub(super) fn mutate(
    doc: &mut Document,
    id: &str,
    params: Value,
    cancel: &dyn Fn() -> bool,
) -> Result<Option<Value>, CommandError> {
    if id != "image.rotate" {
        return Ok(None);
    }
    check(cancel)?;
    let p: RotateImage = parse(params)?;
    if !p.angle_degrees.is_finite() || p.angle_degrees.abs() > 360000. {
        return Err(invalid("image angle needs finite degrees inside +/-360000"));
    }
    let angle = p.angle_degrees.rem_euclid(360.);
    let old = doc.frame.clone();
    let result = |frame: &crate::Frame, changed| serde_json::json!({"angle_degrees":angle,"previous_frame":old,"frame":frame,"changed":changed,"source_artboard_clipped":false,"preserved_layer_structure":true});
    if angle == 0. {
        return Ok(Some(result(&old, false)));
    }
    let operation = match angle {
        90. => Some("rotate_90_cw"),
        180. => Some("rotate_180"),
        270. => Some("rotate_90_ccw"),
        _ => None,
    };
    if let Some(operation) = operation {
        super::orthogonal::mutate(
            doc,
            "image.orient",
            serde_json::json!({"operation":operation}),
            cancel,
        )?;
        return Ok(Some(result(&doc.frame, true)));
    }
    for layer in &doc.layers {
        let info = layer.info();
        match &info.kind {
            LayerKind::Pixel | LayerKind::Group => {}
            LayerKind::Solid { .. }
            | LayerKind::Text { .. }
            | LayerKind::Gradient { .. }
            | LayerKind::Exposure { .. }
            | LayerKind::HueSaturation { .. }
            | LayerKind::Adjustment { .. } => {
                return Err(invalid(
                    "arbitrary image rotation currently supports pixel layers and unstyled groups; editable fills/adjustments require later geometry support and are never implicitly rasterized",
                ));
            }
        }
        if !info.styles.is_empty() || info.generated_size.is_some() {
            return Err(invalid(
                "arbitrary image rotation does not yet support layer styles or frozen style references",
            ));
        }
        let l = info.locks;
        if l.all || l.pixels || l.position || l.transparency {
            return Err(invalid(
                "unlock every affected layer/ancestor pixel, position, transparency and all lock before image rotation",
            ));
        }
    }
    let (transform, frame) = layout(&old, angle)?;
    if frame.width > 100000
        || frame.height > 100000
        || doc
            .format
            .byte_count(frame.width, frame.height)
            .map_err(DocumentError::from)? as u64
            > crate::MAX_FLAT_IMAGE_BYTES
    {
        return Err(DocumentError::ResourceLimit.into());
    }
    let sampling = match p.interpolation {
        TransformInterpolation::Nearest => Sampling::Nearest,
        TransformInterpolation::Bilinear => Sampling::Bilinear,
    };
    let source = doc.clone();
    let pool = doc.memory_pool();
    let warp_mask = |mask: &strata_pixels::TiledMask, offset| {
        crate::geometry_masks::transform_mask(
            mask,
            offset,
            &transform,
            sampling,
            pool.clone(),
            cancel,
        )
    };
    let document_mask = |mask: &strata_pixels::TiledMask| {
        let transformed = warp_mask(mask, [0, 0])?;
        crate::geometry_masks::rebind(
            &transformed,
            CoverageRect {
                x: frame.x,
                y: frame.y,
                width: frame.width,
                height: frame.height,
            },
            pool.clone(),
            cancel,
        )
    };
    let selection = source.selection.as_ref().map(document_mask).transpose()?;
    let previous = source
        .previous_selection
        .as_ref()
        .map(document_mask)
        .transpose()?;
    for (index, layer) in source.layers.iter().enumerate() {
        check(cancel)?;
        let info = layer.info();
        let offset = [info.x, info.y];
        let bounds = geometry_pixels::content_bounds(layer.pixels(), offset, cancel)?;
        let origin = if let Some(b) = bounds {
            let b = geometry_pixels::transformed_bounds(b, &transform, Sampling::Nearest)?;
            [b.x, b.y]
        } else {
            let p = transform
                .map([info.x as f64, info.y as f64])
                .map_err(invalid)?;
            [p[0].floor() as i64, p[1].floor() as i64]
        };
        let output = bounds
            .map(|bounds| {
                geometry_pixels::transform_pixels_at(
                    PixelSource {
                        pixels: layer.pixels(),
                        offset,
                        bounds,
                        icc: source.working_icc(),
                    },
                    &transform,
                    sampling,
                    origin,
                    pool.clone(),
                    cancel,
                )
            })
            .transpose()?;
        let mask = if let (Some(mask), Some(meta)) = (&layer.mask, &info.mask) {
            Some(warp_mask(
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
        check(cancel)?;
        let target = &mut doc.layers[index];
        if let Some(output) = output {
            target.pixels = output;
        }
        target.info.x = origin[0];
        target.info.y = origin[1];
        if let Some(mask) = mask {
            let enabled = info.mask.as_ref().unwrap().enabled;
            super::local::attach(target, mask);
            target.info.mask.as_mut().unwrap().enabled = enabled;
        }
    }
    check(cancel)?;
    doc.selection = selection;
    doc.previous_selection = previous;
    doc.frame = frame;
    Ok(Some(result(&doc.frame, true)))
}
