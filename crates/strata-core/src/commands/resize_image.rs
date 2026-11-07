//! Document-size resampling retains layer identity and never flattens. Editable
//! types without a defined sizing model are rejected before any pixels change.
use super::*;
use crate::{
    Document, DocumentError, LayerKind,
    geometry_pixels::{self, PixelSource},
    geometry_resize::{self, Geometry, Work},
};
use strata_pixels::CoverageRect;
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
    if id != "image.resize" {
        return Ok(None);
    }
    check(cancel)?;
    let p: ResizeImage = parse(params)?;
    if p.width == 0 || p.height == 0 || p.width > 100000 || p.height > 100000 {
        return Err(DocumentError::ResourceLimit.into());
    }
    let before = doc.frame.clone();
    let result = |frame: &crate::Frame, changed| serde_json::json!({"previous_frame":before,"frame":frame,"interpolation":p.interpolation,"changed":changed,"source_artboard_clipped":false,"preserved_layer_structure":true});
    if before.width == p.width && before.height == p.height {
        return Ok(Some(result(&before, false)));
    }
    if u64::from(p.width) * u64::from(p.height) > 64_000_000
        || doc
            .format
            .byte_count(p.width, p.height)
            .map_err(DocumentError::from)? as u64
            > crate::MAX_FLAT_IMAGE_BYTES
    {
        return Err(DocumentError::ResourceLimit.into());
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
                return Err(CommandError::Invalid(
                    "image resizing currently supports pixel layers and unstyled groups; editable fills/adjustments are not implicitly rasterized".into()
                ));
            }
        }
        if !info.styles.is_empty() || info.generated_size.is_some() {
            return Err(CommandError::Invalid(
                "image resizing does not yet support styles or frozen style references".into(),
            ));
        }
        let l = info.locks;
        if l.all || l.pixels || l.position || l.transparency {
            return Err(CommandError::Invalid("unlock every affected pixel/position/transparency/all layer or ancestor lock before resizing".into()));
        }
    }
    let source = doc.clone();
    let geometry = Geometry::new(&before, [p.width, p.height]);
    let pool = doc.memory_pool();
    let mut work = Work::default();
    let frame = crate::Frame {
        width: p.width,
        height: p.height,
        ..before.clone()
    };
    let frame_bounds = CoverageRect {
        x: frame.x,
        y: frame.y,
        width: frame.width,
        height: frame.height,
    };
    // Validate the new frame as well as each independently stored source bound.
    geometry.bounds(CoverageRect {
        x: before.x,
        y: before.y,
        width: before.width,
        height: before.height,
    })?;
    let mut document_mask = |mask: &strata_pixels::TiledMask| -> Result<_, CommandError> {
        let resized = geometry_resize::mask(
            mask,
            [0, 0],
            geometry,
            p.interpolation,
            pool.clone(),
            &mut work,
            cancel,
        )?;
        Ok(crate::geometry_masks::rebind(
            &resized,
            frame_bounds,
            pool.clone(),
            cancel,
        )?)
    };
    let selection = source
        .selection
        .as_ref()
        .map(&mut document_mask)
        .transpose()?;
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
        let output = bounds
            .map(|bounds| {
                geometry_resize::pixels(
                    PixelSource {
                        pixels: layer.pixels(),
                        offset,
                        bounds,
                        icc: source.working_icc(),
                    },
                    geometry,
                    p.interpolation,
                    pool.clone(),
                    &mut work,
                    cancel,
                )
            })
            .transpose()?;
        let origin = if let Some((_, origin)) = &output {
            *origin
        } else {
            geometry.position(offset)?
        };
        let mask = if let (Some(mask), Some(meta)) = (&layer.mask, &info.mask) {
            Some(geometry_resize::mask(
                mask,
                [
                    info.x
                        .checked_sub(meta.anchor_x)
                        .ok_or(DocumentError::ResourceLimit)?,
                    info.y
                        .checked_sub(meta.anchor_y)
                        .ok_or(DocumentError::ResourceLimit)?,
                ],
                geometry,
                p.interpolation,
                pool.clone(),
                &mut work,
                cancel,
            )?)
        } else {
            None
        };
        check(cancel)?;
        let target = &mut doc.layers[index];
        if let Some((output, _)) = output {
            target.pixels = output;
        }
        target.info.x = origin[0];
        target.info.y = origin[1];
        if let Some(mask) = mask {
            super::local::attach(target, mask);
            target.info.mask.as_mut().unwrap().enabled = info.mask.as_ref().unwrap().enabled;
        }
    }
    check(cancel)?;
    doc.selection = selection;
    doc.previous_selection = previous;
    doc.frame = frame;
    Ok(Some(result(&doc.frame, true)))
}
