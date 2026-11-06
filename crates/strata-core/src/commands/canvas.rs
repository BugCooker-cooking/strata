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
    if id != "image.canvas_size" {
        return Ok(None);
    }
    if cancel() {
        return Err(CommandError::Expired);
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
