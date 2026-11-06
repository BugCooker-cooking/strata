//! Canvas-only geometry keeps editable source content outside the artboard.
//! Linked masks stay anchored; document selections are rebound to the new frame.
use super::*;
use crate::{Document, DocumentError, Frame, LayerKind};
use strata_pixels::CoverageRect;

fn rect(f: &Frame) -> CoverageRect {
    CoverageRect {
        x: f.x,
        y: f.y,
        width: f.width,
        height: f.height,
    }
}
fn checked_frame(doc: &Document, frame: &Frame) -> Result<(), CommandError> {
    if frame.width > 100000 || frame.height > 100000 {
        return Err(CommandError::Invalid(
            "canvas dimensions must be 1..100000".into(),
        ));
    }
    rect(frame).validate().map_err(DocumentError::from)?;
    if doc
        .format
        .byte_count(frame.width, frame.height)
        .map_err(DocumentError::from)? as u64
        > crate::MAX_FLAT_IMAGE_BYTES
    {
        return Err(DocumentError::ResourceLimit.into());
    }
    Ok(())
}
fn freeze_references(doc: &mut Document) {
    let size = [doc.frame.width, doc.frame.height];
    let bounds = rect(&doc.frame);
    for layer in &mut doc.layers {
        if matches!(
            layer.info.kind,
            LayerKind::Solid { .. } | LayerKind::Gradient { .. }
        ) || layer.info.styles.gradient_overlay.is_some()
        {
            layer.info.generated_size.get_or_insert(size);
        }
        if matches!(
            layer.info.kind,
            LayerKind::Adjustment {
                adjustment: crate::TonalAdjustment::ShadowsHighlights { .. }
            }
        ) {
            layer.info.spatial_bounds.get_or_insert(bounds);
        }
    }
}
fn publish_frame(
    doc: &mut Document,
    frame: Frame,
    cancel: &dyn Fn() -> bool,
) -> Result<Value, CommandError> {
    checked_frame(doc, &frame)?;
    let before = doc.frame.clone();
    let bounds = rect(&frame);
    let selection = doc
        .selection
        .as_ref()
        .map(|m| crate::geometry_masks::rebind(m, bounds, doc.memory_pool(), cancel))
        .transpose()?;
    let previous = doc
        .previous_selection
        .as_ref()
        .map(|m| crate::geometry_masks::rebind(m, bounds, doc.memory_pool(), cancel))
        .transpose()?;
    if cancel() {
        return Err(CommandError::Expired);
    }
    freeze_references(doc);
    doc.selection = selection;
    doc.previous_selection = previous;
    doc.frame = frame;
    Ok(
        serde_json::json!({"previous_frame":before,"frame":doc.frame,"preserved_external_content":true,"selection_rebound":doc.selection.is_some()||doc.previous_selection.is_some()}),
    )
}
pub(super) fn mutate(
    doc: &mut Document,
    id: &str,
    params: Value,
    cancel: &dyn Fn() -> bool,
) -> Result<Option<Value>, CommandError> {
    if !matches!(id, "image.canvas_size" | "image.crop" | "image.reveal_all") {
        return Ok(None);
    }
    if cancel() {
        return Err(CommandError::Expired);
    }
    if id == "image.crop" {
        let p: CropCanvas = parse(params)?;
        let mut bounds = p.rect;
        bounds.validate().map_err(DocumentError::from)?;
        if let Some([w, h]) = p.aspect_ratio {
            if w == 0 || h == 0 || w > 100000 || h > 100000 {
                return Err(CommandError::Invalid(
                    "crop aspect_ratio needs two integers 1..100000".into(),
                ));
            }
            // Largest integer rectangle inside the requested box, fixed at its
            // top-left. Reduce the ratio first so the result is exact.
            let (mut a, mut b) = (w, h);
            while b != 0 {
                (a, b) = (b, a % b);
            }
            let (w, h) = (w / a, h / a);
            let multiple = (bounds.width / w).min(bounds.height / h);
            if multiple == 0 {
                return Err(CommandError::Invalid(
                    "crop box cannot contain one integer unit of this aspect ratio".into(),
                ));
            }
            bounds.width = w * multiple;
            bounds.height = h * multiple;
        }
        let frame = Frame {
            name: doc.frame.name.clone(),
            x: bounds.x,
            y: bounds.y,
            width: bounds.width,
            height: bounds.height,
        };
        return publish_frame(doc, frame, cancel).map(Some);
    }
    if id == "image.reveal_all" {
        let _: Empty = parse(params)?;
        let old = &doc.frame;
        let mut extent = [
            i128::from(old.x),
            i128::from(old.y),
            i128::from(old.x) + i128::from(old.width),
            i128::from(old.y) + i128::from(old.height),
        ];
        for layer in &doc.layers {
            if cancel() {
                return Err(CommandError::Expired);
            }
            if !doc.effective_visibility(layer.id()) {
                continue;
            }
            let info = layer.info();
            let bounds = match info.kind {
                LayerKind::Pixel => crate::geometry_pixels::content_bounds(
                    layer.pixels(),
                    [info.x, info.y],
                    cancel,
                )?,
                LayerKind::Solid { .. } | LayerKind::Gradient { .. } => {
                    let [width, height] = info.generated_size.unwrap_or([old.width, old.height]);
                    Some(CoverageRect {
                        x: info.x,
                        y: info.y,
                        width,
                        height,
                    })
                }
                _ => None,
            };
            if let Some(b) = bounds {
                let pad = doc.style_outset(layer.id());
                extent[0] = extent[0].min(i128::from(b.x) - i128::from(pad[0]));
                extent[1] = extent[1].min(i128::from(b.y) - i128::from(pad[1]));
                extent[2] =
                    extent[2].max(i128::from(b.x) + i128::from(b.width) + i128::from(pad[2]));
                extent[3] =
                    extent[3].max(i128::from(b.y) + i128::from(b.height) + i128::from(pad[3]));
            }
        }
        let frame = Frame {
            name: old.name.clone(),
            x: i64::try_from(extent[0]).map_err(|_| DocumentError::ResourceLimit)?,
            y: i64::try_from(extent[1]).map_err(|_| DocumentError::ResourceLimit)?,
            width: u32::try_from(extent[2] - extent[0])
                .map_err(|_| DocumentError::ResourceLimit)?,
            height: u32::try_from(extent[3] - extent[1])
                .map_err(|_| DocumentError::ResourceLimit)?,
        };
        return publish_frame(doc, frame, cancel).map(Some);
    }
    let p: CanvasSize = parse(params)?;
    let (horizontal, vertical) = match p.anchor {
        CanvasAnchor::TopLeft => (0, 0),
        CanvasAnchor::Top => (1, 0),
        CanvasAnchor::TopRight => (2, 0),
        CanvasAnchor::Left => (0, 1),
        CanvasAnchor::Center => (1, 1),
        CanvasAnchor::Right => (2, 1),
        CanvasAnchor::BottomLeft => (0, 2),
        CanvasAnchor::Bottom => (1, 2),
        CanvasAnchor::BottomRight => (2, 2),
    };
    let origin = |old: i64, input: u32, output: u32, anchor: i128| {
        i64::try_from(
            i128::from(old) + ((i128::from(input) - i128::from(output)) * anchor).div_euclid(2),
        )
        .map_err(|_| DocumentError::ResourceLimit)
    };
    let frame = Frame {
        name: doc.frame.name.clone(),
        x: origin(doc.frame.x, doc.frame.width, p.width, horizontal)?,
        y: origin(doc.frame.y, doc.frame.height, p.height, vertical)?,
        width: p.width,
        height: p.height,
    };
    publish_frame(doc, frame, cancel).map(Some)
}
