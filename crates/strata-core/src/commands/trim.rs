//! Alpha-only visible-composite bounds. Hidden RGB never counts as visible
//! content; source layers remain stored and editable outside the trimmed frame.
use super::*;
use crate::{Document, DocumentError, composite::Compositor};
use strata_pixels::TileCoord;
pub(super) fn mutate(
    doc: &mut Document,
    id: &str,
    params: Value,
    cancel: &dyn Fn() -> bool,
) -> Result<Option<Value>, CommandError> {
    if id != "image.trim" {
        return Ok(None);
    }
    let p: TrimCanvas = parse(params)?;
    if ![p.top, p.bottom, p.left, p.right].iter().any(|v| *v) {
        return Err(CommandError::Invalid(
            "choose at least one transparent edge to trim".into(),
        ));
    }
    if cancel() {
        return Err(CommandError::Expired);
    }
    let old = doc.frame.clone();
    if u64::from(old.width) * u64::from(old.height) > 64_000_000 {
        return Err(DocumentError::ResourceLimit.into());
    }
    let right = old.x + i64::from(old.width);
    let bottom = old.y + i64::from(old.height);
    let (first, _) = TileCoord::locate(old.x, old.y);
    let (last, _) = TileCoord::locate(right - 1, bottom - 1);
    let mut compositor = Compositor::new(1);
    let mut extent: Option<[i64; 4]> = None;
    for ty in first.y()..=last.y() {
        for tx in first.x()..=last.x() {
            if cancel() {
                return Err(CommandError::Expired);
            }
            let coordinate = TileCoord::new(tx, ty).map_err(DocumentError::from)?;
            let [ox, oy] = coordinate.origin();
            let tile =
                compositor.tile_prefix_cancellable(doc, coordinate, doc.layers.len(), cancel)?;
            let y0 = old.y.max(oy);
            let y1 = bottom.min(oy.saturating_add(256));
            let x0 = old.x.max(ox);
            let x1 = right.min(ox.saturating_add(256));
            for y in y0..y1 {
                if cancel() {
                    return Err(CommandError::Expired);
                }
                for x in x0..x1 {
                    let alpha = tile[(y - oy) as usize * 256 + (x - ox) as usize][3];
                    if alpha <= 0. {
                        continue;
                    }
                    extent = Some(extent.map_or([x, y, x + 1, y + 1], |[l, t, r, b]| {
                        [l.min(x), t.min(y), r.max(x + 1), b.max(y + 1)]
                    }));
                }
            }
        }
    }
    let Some([l, t, r, b]) = extent else {
        return Err(CommandError::Invalid(
            "visible composite is fully transparent; no nonempty trim rectangle exists".into(),
        ));
    };
    let x = if p.left { l } else { old.x };
    let y = if p.top { t } else { old.y };
    let r = if p.right { r } else { right };
    let b = if p.bottom { b } else { bottom };
    let frame = crate::Frame {
        name: old.name.clone(),
        x,
        y,
        width: (r - x) as u32,
        height: (b - y) as u32,
    };
    if frame == old {
        return Ok(Some(
            serde_json::json!({"previous_frame":old,"frame":frame,"changed":false,"preserved_external_content":true,"selection_rebound":false}),
        ));
    }
    let mut result = super::canvas::publish_frame(doc, frame, cancel)?;
    result["changed"] = serde_json::json!(true);
    Ok(Some(result))
}
