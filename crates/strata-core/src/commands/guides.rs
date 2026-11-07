use super::*;
use crate::{
    Document, DocumentError, Guide, GuideId, GuideRef, LayerId, LayerKind, Presentation, StateId,
};
use strata_pixels::CoverageRect;

fn invalid(message: impl Into<String>) -> CommandError {
    CommandError::Invalid(message.into())
}
pub(super) fn resolve(doc: &Document, reference: &GuideRef) -> Result<usize, CommandError> {
    let candidates: Vec<_> = doc
        .guides
        .iter()
        .enumerate()
        .filter(|(_, g)| match reference {
            GuideRef::Id(id) => g.id == *id,
            GuideRef::Name(name) => g.name == *name,
        })
        .collect();
    match candidates.as_slice() {
        [(i, _)] => Ok(*i),
        [] => Err(invalid("guide not found")),
        _ => Err(invalid(format!(
            "ambiguous guide name; use one of {:?}",
            candidates.iter().map(|(_, g)| g.id).collect::<Vec<_>>()
        ))),
    }
}
pub(super) fn list(doc: &Document, params: Value) -> Result<Value, CommandError> {
    let p: ListGuides = parse(params)?;
    if p.offset > crate::MAX_GUIDES || !(1..=64).contains(&p.limit) {
        return Err(invalid("guide offset0..1024 and limit1..64 required"));
    }
    let end = p.offset.saturating_add(p.limit).min(doc.guides.len());
    let guides = doc.guides.get(p.offset..end).unwrap_or(&[]).to_vec();
    Ok(serde_json::to_value(GuidesResult {
        guides,
        total: doc.guides.len(),
        next_offset: (end < doc.guides.len()).then_some(end),
    })
    .unwrap())
}
pub(super) fn mutate(
    doc: &mut Document,
    id: &str,
    params: Value,
    locked: bool,
) -> Result<Option<Value>, CommandError> {
    if !matches!(
        id,
        "view.guides.add" | "view.guides.update" | "view.guides.delete" | "view.guides.clear"
    ) {
        return Ok(None);
    }
    if locked {
        return Err(invalid(
            "guides are locked; unlock reference lines before editing",
        ));
    }
    if id == "view.guides.clear" {
        let _: Empty = parse(params)?;
        let removed = doc.guides.len();
        doc.guides.clear();
        return Ok(Some(
            serde_json::to_value(ClearGuidesResult {
                removed,
                changed: removed > 0,
            })
            .unwrap(),
        ));
    }
    let (guide, changed) = if id == "view.guides.add" {
        let p: AddGuide = parse(params)?;
        if doc.guides.len() >= crate::MAX_GUIDES {
            return Err(DocumentError::ResourceLimit.into());
        }
        let guide = Guide {
            id: GuideId::fresh(),
            name: p.name,
            axis: p.axis,
            position: p.position,
        };
        guide.validate()?;
        doc.guides.push(guide.clone());
        (guide, true)
    } else if id == "view.guides.update" {
        let p: UpdateGuide = parse(params)?;
        if p.axis.is_none() && p.position.is_none() && p.name.is_none() {
            return Err(invalid("provide guide axis, position and/or name"));
        }
        let at = resolve(doc, &p.guide)?;
        let mut guide = doc.guides[at].clone();
        if let Some(axis) = p.axis {
            guide.axis = axis
        };
        if let Some(position) = p.position {
            guide.position = position
        };
        if let Some(name) = p.name {
            guide.name = name
        };
        guide.validate()?;
        let changed = guide != doc.guides[at];
        if changed {
            doc.guides[at] = guide.clone()
        };
        (guide, changed)
    } else {
        let p: TargetGuide = parse(params)?;
        let at = resolve(doc, &p.guide)?;
        (doc.guides.remove(at), true)
    };
    Ok(Some(
        serde_json::to_value(GuideResult { guide, changed }).unwrap(),
    ))
}
pub(super) fn presentation(
    old: &Presentation,
    params: Value,
) -> Result<Presentation, CommandError> {
    let p: SetPresentation = parse(params)?;
    if p.expected_version.is_some_and(|v| v != old.version) {
        return Err(CommandError::Stale);
    }
    if p.rulers_visible.is_none()
        && p.guides_visible.is_none()
        && p.guides_locked.is_none()
        && p.snap_enabled.is_none()
        && p.snap_tolerance.is_none()
        && p.snap_artboard.is_none()
    {
        return Err(invalid("provide at least one presentation setting"));
    }
    let mut value = old.clone();
    if let Some(v) = p.rulers_visible {
        value.rulers_visible = v
    };
    if let Some(v) = p.guides_visible {
        value.guides_visible = v
    };
    if let Some(v) = p.guides_locked {
        value.guides_locked = v
    };
    if let Some(v) = p.snap_enabled {
        value.snap_enabled = v
    };
    if let Some(v) = p.snap_artboard {
        value.snap_artboard = v
    };
    if let Some(v) = p.snap_tolerance {
        if !v.is_finite() || !(0.0..=64.).contains(&v) {
            return Err(invalid("screen-point snap tolerance must be finite0..64"));
        };
        value.snap_tolerance = v
    };
    if value != *old {
        value.version = old
            .version
            .checked_add(1)
            .ok_or_else(|| invalid("presentation version exhausted"))?
    };
    Ok(value)
}
#[derive(Clone, Debug)]
pub(super) struct SnapContext {
    state: StateId,
    layer: LayerId,
    origin: [i64; 2],
    bounds: CoverageRect,
}
fn union(a: CoverageRect, b: CoverageRect) -> Result<CoverageRect, DocumentError> {
    let x = a.x.min(b.x);
    let y = a.y.min(b.y);
    let r = (i128::from(a.x) + i128::from(a.width)).max(i128::from(b.x) + i128::from(b.width));
    let bottom =
        (i128::from(a.y) + i128::from(a.height)).max(i128::from(b.y) + i128::from(b.height));
    Ok(CoverageRect {
        x,
        y,
        width: u32::try_from(r - i128::from(x)).map_err(|_| DocumentError::ResourceLimit)?,
        height: u32::try_from(bottom - i128::from(y)).map_err(|_| DocumentError::ResourceLimit)?,
    })
}
pub(super) fn snap_context(
    doc: &Document,
    target: LayerId,
    cancel: &dyn Fn() -> bool,
) -> Result<Option<SnapContext>, CommandError> {
    if !doc.effective_visibility(target) {
        return Err(invalid("snapping requires a visible target and ancestors"));
    }
    let root = doc
        .layers
        .iter()
        .find(|l| l.id() == target)
        .ok_or(DocumentError::LayerNotFound)?;
    if [root.info.x, root.info.y]
        .iter()
        .any(|v| v.unsigned_abs() > 1_000_000_000_000)
    {
        return Err(invalid("snap source origin exceeds canvas range +/-1e12"));
    }
    let mut bounds = None;
    let mut visits = 0u64;
    let mut areas = 0u64;
    for layer in doc.layers.iter().filter(|l| {
        (l.id() == target || doc.is_descendant(l.id(), target)) && doc.effective_visibility(l.id())
    }) {
        if cancel() {
            return Err(CommandError::Expired);
        }
        visits += layer.pixels().tile_count() as u64 * 256 * 256;
        if visits > 64_000_000 {
            return Err(DocumentError::ResourceLimit.into());
        }
        let content = match &layer.info.kind {
            LayerKind::Pixel | LayerKind::Text { .. } => {
                super::transform::bounds(doc, layer, cancel)?
            }
            LayerKind::Solid { .. } | LayerKind::Gradient { .. } => {
                doc.generated_canvas_bounds(layer)?
            }
            LayerKind::Group => None,
            LayerKind::Exposure { .. }
            | LayerKind::HueSaturation { .. }
            | LayerKind::Adjustment { .. } => None,
        };
        if let Some(b) = content {
            if [
                i128::from(b.x),
                i128::from(b.y),
                i128::from(b.x) + i128::from(b.width),
                i128::from(b.y) + i128::from(b.height),
            ]
            .iter()
            .any(|v| v.abs() > 1_000_000_000_000)
            {
                return Err(invalid("snap source bounds exceed canvas range +/-1e12"));
            }
            areas += u64::from(b.width) * u64::from(b.height);
            if areas > 64_000_000 {
                return Err(DocumentError::ResourceLimit.into());
            };
            bounds = Some(match bounds {
                None => b,
                Some(old) => union(old, b)?,
            })
        }
    }
    Ok(bounds.map(|bounds| SnapContext {
        state: doc.state,
        layer: target,
        origin: [root.info.x, root.info.y],
        bounds,
    }))
}
#[derive(Clone)]
struct Candidate {
    ideal: f64,
    anchor: f64,
    coordinate: f64,
    guide_id: Option<GuideId>,
    kind: SnapTarget,
    order: usize,
}
fn best(
    doc: &Document,
    p: &SnapPosition,
    context: &SnapContext,
    axis: usize,
) -> Option<(i64, SnapMatch)> {
    let proposed = [p.x, p.y][axis];
    let origin = context.origin[axis];
    let min = [context.bounds.x, context.bounds.y][axis] as f64
        + (i128::from(proposed) - i128::from(origin)) as f64;
    let length = f64::from([context.bounds.width, context.bounds.height][axis]);
    let anchors = match p.anchors {
        SnapAnchors::Edges => vec![min, min + length],
        SnapAnchors::Center => vec![min + length / 2.],
        SnapAnchors::EdgesAndCenter => vec![min, min + length, min + length / 2.],
    };
    let mut targets = Vec::new();
    if p.guides {
        for g in &doc.guides {
            if g.axis.component() == axis {
                targets.push((g.position, Some(g.id), SnapTarget::Guide))
            }
        }
    }
    if p.artboard {
        let a = [doc.frame.x, doc.frame.y][axis] as f64;
        let length = f64::from([doc.frame.width, doc.frame.height][axis]);
        targets.extend([
            (a, None, SnapTarget::ArtboardEdge),
            (a + length, None, SnapTarget::ArtboardEdge),
        ])
    }
    let mut candidates = Vec::new();
    for (coordinate, guide_id, kind) in targets {
        for (order, anchor) in anchors.iter().copied().enumerate() {
            let ideal = coordinate - anchor;
            let resolved = (proposed as f64 + ideal).round();
            if ideal.abs() <= p.tolerance && resolved.is_finite() && resolved.abs() <= 1e12 {
                candidates.push(Candidate {
                    ideal,
                    anchor,
                    coordinate,
                    guide_id,
                    kind,
                    order,
                })
            }
        }
    }
    let candidate = candidates.into_iter().min_by(|a, b| {
        a.ideal
            .abs()
            .total_cmp(&b.ideal.abs())
            .then_with(|| a.guide_id.is_none().cmp(&b.guide_id.is_none()))
            .then_with(|| a.coordinate.total_cmp(&b.coordinate))
            .then_with(|| a.order.cmp(&b.order))
            .then_with(|| a.guide_id.cmp(&b.guide_id))
    })?;
    let resolved = (proposed as f64 + candidate.ideal).round();
    if !resolved.is_finite() || resolved.abs() > 1e12 {
        return None;
    }
    let actual = resolved as i64;
    let anchor = candidate.anchor + (i128::from(actual) - i128::from(proposed)) as f64;
    Some((
        actual,
        SnapMatch {
            target: candidate.kind,
            guide_id: candidate.guide_id,
            coordinate: candidate.coordinate,
            anchor,
            residual: candidate.coordinate - anchor,
        },
    ))
}
pub(super) fn snap(
    doc: &mut Document,
    params: Value,
    cached: Option<&SnapContext>,
    cancel: &dyn Fn() -> bool,
) -> Result<Value, CommandError> {
    let p: SnapPosition = parse(params)?;
    if !p.tolerance.is_finite()
        || !(0.0..=1e6).contains(&p.tolerance)
        || [p.x, p.y]
            .iter()
            .any(|v| v.unsigned_abs() > 1_000_000_000_000)
    {
        return Err(invalid(
            "snap positions within +/-1e12 and tolerance0..1e6 required",
        ));
    }
    let target = doc.resolve(&p.layer)?;
    check_snap_target(doc, target)?;
    let context = if let Some(c) = cached.filter(|c| c.state == doc.state && c.layer == target) {
        c.clone()
    } else {
        snap_context(doc, target, cancel)?
            .ok_or_else(|| invalid("no finite pixel/text/fill content to snap"))?
    };
    let x = best(doc, &p, &context, 0);
    let y = best(doc, &p, &context, 1);
    let position = [
        x.as_ref().map_or(p.x, |v| v.0),
        y.as_ref().map_or(p.y, |v| v.0),
    ];
    let changed = position != context.origin;
    if cancel() {
        return Err(CommandError::Expired);
    }
    if changed {
        super::tree::move_position(doc, target, position[0], position[1])?;
    }
    Ok(serde_json::to_value(SnapPositionResult {
        layer_id: target,
        x: position[0],
        y: position[1],
        snapped_x: x.map(|v| v.1),
        snapped_y: y.map(|v| v.1),
        changed,
    })
    .unwrap())
}
pub(super) fn check_snap_target(doc: &Document, target: LayerId) -> Result<(), CommandError> {
    if !doc.effective_visibility(target) {
        return Err(invalid("snapping requires visible content"));
    }
    if doc.layers.iter().any(|l| {
        (l.id() == target || doc.is_descendant(target, l.id()) || doc.is_descendant(l.id(), target))
            && (l.info.locks.all || l.info.locks.position)
    }) {
        return Err(invalid(
            "unlock target, ancestor and subtree positions before snapping",
        ));
    }
    Ok(())
}
