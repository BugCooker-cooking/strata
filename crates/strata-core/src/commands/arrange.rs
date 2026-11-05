use super::*;
use crate::{Document, DocumentError, LayerId, LayerKind};
use serde_json::json;
use std::collections::HashSet;
use strata_pixels::{CoverageRect, PixelBuffer};
fn targets(doc: &Document, refs: &[LayerRef]) -> Result<Vec<LayerId>, CommandError> {
    if refs.is_empty() || refs.len() > crate::MAX_LAYERS {
        return Err(CommandError::Invalid("select 1..128 layers".into()));
    }
    let ids = refs
        .iter()
        .map(|r| doc.resolve(r))
        .collect::<Result<Vec<_>, _>>()?;
    if ids.iter().collect::<HashSet<_>>().len() != ids.len() {
        return Err(CommandError::Invalid(
            "duplicate arrangement targets".into(),
        ));
    }
    for id in &ids {
        if ids.iter().any(|parent| doc.is_descendant(*id, *parent)) {
            return Err(CommandError::Invalid(
                "select either a group or its descendants for arrangement".into(),
            ));
        }
        if doc.layers.iter().any(|l| {
            (l.id() == *id || doc.is_descendant(*id, l.id()))
                && (l.info.locks.all || l.info.locks.position)
        }) {
            return Err(CommandError::Invalid(
                "target or parent position is locked".into(),
            ));
        }
    }
    Ok(ids)
}
fn bounds(doc: &Document, id: LayerId) -> Result<CoverageRect, CommandError> {
    let root = doc.layers.iter().find(|l| l.id() == id).unwrap();
    let mut extent: Option<(i64, i64, i64, i64)> = None;
    let mut add = |x: i64, y: i64, right: i64, bottom: i64| {
        extent = Some(match extent {
            Some((a, b, c, d)) => (a.min(x), b.min(y), c.max(right), d.max(bottom)),
            None => (x, y, right, bottom),
        });
    };
    for layer in doc
        .layers
        .iter()
        .filter(|l| l.id() == id || doc.is_descendant(l.id(), id))
    {
        if matches!(
            layer.info.kind,
            LayerKind::Solid { .. } | LayerKind::Gradient { .. }
        ) {
            let [w, h] = layer
                .info
                .generated_size
                .unwrap_or([doc.frame.width, doc.frame.height]);
            add(
                layer.info.x,
                layer.info.y,
                layer.info.x + i64::from(w),
                layer.info.y + i64::from(h),
            );
        }
        for (coord, tile) in layer.pixels.tiles() {
            let tile = tile.map_err(DocumentError::from)?;
            let [ox, oy] = coord.origin();
            let ox = ox + layer.info.x;
            let oy = oy + layer.info.y;
            for y in 0..256usize {
                for x in 0..256usize {
                    let i = (y * 256 + x) * 4;
                    let occupied = match &*tile {
                        PixelBuffer::Rgba8(v) => v[i..i + 4].iter().any(|c| *c != 0),
                        PixelBuffer::Rgba16(v) => v[i..i + 4].iter().any(|c| *c != 0),
                    };
                    if occupied {
                        add(
                            ox + x as i64,
                            oy + y as i64,
                            ox + x as i64 + 1,
                            oy + y as i64 + 1,
                        );
                    }
                }
            }
        }
    }
    let (x, y, right, bottom) = extent.unwrap_or((
        root.info.x,
        root.info.y,
        root.info.x + i64::from(doc.frame.width),
        root.info.y + i64::from(doc.frame.height),
    ));
    Ok(CoverageRect {
        x,
        y,
        width: u32::try_from(i128::from(right) - i128::from(x))
            .map_err(|_| DocumentError::ResourceLimit)?,
        height: u32::try_from(i128::from(bottom) - i128::from(y))
            .map_err(|_| DocumentError::ResourceLimit)?,
    })
}
fn shifted(doc: &mut Document, id: LayerId, dx: i128, dy: i128) -> Result<(), CommandError> {
    let root = doc.layers.iter().find(|l| l.id() == id).unwrap();
    let x = i64::try_from(i128::from(root.info.x) + dx)
        .map_err(|_| CommandError::Invalid("alignment coordinate overflow".into()))?;
    let y = i64::try_from(i128::from(root.info.y) + dy)
        .map_err(|_| CommandError::Invalid("alignment coordinate overflow".into()))?;
    super::tree::move_position(doc, id, x, y)?;
    Ok(())
}
pub(super) fn mutate(
    doc: &mut Document,
    id: &str,
    params: Value,
) -> Result<Option<Value>, CommandError> {
    if id == "layer.align" {
        let p: AlignLayers = parse(params)?;
        let ids = targets(doc, &p.layers)?;
        let boxes = ids
            .iter()
            .map(|id| bounds(doc, *id))
            .collect::<Result<Vec<_>, _>>()?;
        let reference = if matches!(p.reference, AlignReference::Artboard) {
            CoverageRect {
                x: doc.frame.x,
                y: doc.frame.y,
                width: doc.frame.width,
                height: doc.frame.height,
            }
        } else {
            let x = boxes.iter().map(|b| b.x).min().unwrap();
            let y = boxes.iter().map(|b| b.y).min().unwrap();
            let right = boxes
                .iter()
                .map(|b| b.x + i64::from(b.width))
                .max()
                .unwrap();
            let bottom = boxes
                .iter()
                .map(|b| b.y + i64::from(b.height))
                .max()
                .unwrap();
            CoverageRect {
                x,
                y,
                width: u32::try_from(i128::from(right) - i128::from(x))
                    .map_err(|_| DocumentError::ResourceLimit)?,
                height: u32::try_from(i128::from(bottom) - i128::from(y))
                    .map_err(|_| DocumentError::ResourceLimit)?,
            }
        };
        for (target, b) in ids.iter().zip(boxes) {
            let (dx, dy) = match p.mode {
                AlignMode::Left => (i128::from(reference.x) - i128::from(b.x), 0),
                AlignMode::Right => (
                    i128::from(reference.x) + i128::from(reference.width)
                        - i128::from(b.x)
                        - i128::from(b.width),
                    0,
                ),
                AlignMode::HorizontalCenter => (
                    (i128::from(reference.x) * 2 + i128::from(reference.width)
                        - i128::from(b.x) * 2
                        - i128::from(b.width))
                    .div_euclid(2),
                    0,
                ),
                AlignMode::Top => (0, i128::from(reference.y) - i128::from(b.y)),
                AlignMode::Bottom => (
                    0,
                    i128::from(reference.y) + i128::from(reference.height)
                        - i128::from(b.y)
                        - i128::from(b.height),
                ),
                AlignMode::VerticalCenter => (
                    0,
                    (i128::from(reference.y) * 2 + i128::from(reference.height)
                        - i128::from(b.y) * 2
                        - i128::from(b.height))
                    .div_euclid(2),
                ),
            };
            shifted(doc, *target, dx, dy)?;
        }
        return Ok(Some(json!({"layer_ids":ids})));
    }
    if id != "layer.distribute" {
        return Ok(None);
    }
    let p: DistributeLayers = parse(params)?;
    let ids = targets(doc, &p.layers)?;
    if ids.len() < 3 {
        return Err(CommandError::Invalid(
            "distribution requires at least three layers".into(),
        ));
    }
    let horizontal = matches!(
        p.mode,
        Distribution::HorizontalCenters | Distribution::HorizontalSpacing
    );
    let spacing = matches!(
        p.mode,
        Distribution::HorizontalSpacing | Distribution::VerticalSpacing
    );
    let mut boxes = ids
        .iter()
        .map(|id| bounds(doc, *id).map(|b| (*id, b)))
        .collect::<Result<Vec<_>, _>>()?;
    let start = |b: &CoverageRect| {
        if horizontal {
            i128::from(b.x)
        } else {
            i128::from(b.y)
        }
    };
    let size = |b: &CoverageRect| {
        if horizontal {
            i128::from(b.width)
        } else {
            i128::from(b.height)
        }
    };
    boxes.sort_by_key(|(_, b)| {
        if spacing {
            start(b)
        } else {
            start(b) * 2 + size(b)
        }
    });
    let first = boxes.first().unwrap().1;
    let last = boxes.last().unwrap().1;
    let n = boxes.len() as i128 - 1;
    let low = if spacing {
        start(&first)
    } else {
        start(&first) * 2 + size(&first)
    };
    let high = if spacing {
        start(&last) + size(&last)
    } else {
        start(&last) * 2 + size(&last)
    };
    let gap = if spacing {
        high - low - boxes.iter().map(|(_, b)| size(b)).sum::<i128>()
    } else {
        high - low
    };
    let mut cumulative = 0;
    for (i, (id, b)) in boxes.iter().enumerate() {
        let desired = if spacing {
            low + cumulative + (gap * i as i128).div_euclid(n)
        } else {
            (low + (gap * i as i128).div_euclid(n) - size(b)).div_euclid(2)
        };
        let delta = desired - start(b);
        if horizontal {
            shifted(doc, *id, delta, 0)?
        } else {
            shifted(doc, *id, 0, delta)?
        }
        cumulative += size(b);
    }
    Ok(Some(json!({"layer_ids":ids})))
}
