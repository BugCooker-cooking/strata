use super::*;
use crate::geometry_pixels::union;
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
        && p.snap_layers.is_none()
        && p.ruler_unit.is_none()
        && p.ruler_origin.is_none()
    {
        return Err(invalid("provide at least one presentation setting"));
    }
    let mut value = old.clone();
    if let Some(unit) = p.ruler_unit {
        value.ruler_unit = unit
    }
    if let Some(origin) = p.ruler_origin {
        if origin.iter().any(|v| !v.is_finite() || v.abs() > 1e12) {
            return Err(invalid(
                "ruler origin requires finite canvas pixels inside +/-1e12",
            ));
        }
        value.ruler_origin = origin;
    }
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
    if let Some(v) = p.snap_layers {
        value.snap_layers = v;
    }
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
pub(super) fn ruler_coordinates(
    doc: &Document,
    presentation: &Presentation,
    params: Value,
) -> Result<Value, CommandError> {
    let p: ConvertRuler = parse(params)?;
    if p.expected_version
        .is_some_and(|v| v != presentation.version)
    {
        return Err(CommandError::Stale);
    }
    if p.coordinates
        .iter()
        .any(|v| !v.is_finite() || v.abs() > 1e15)
    {
        return Err(invalid(
            "ruler coordinates require finite values inside +/-1e15",
        ));
    }
    let metrics = presentation.ruler_metrics(doc);
    let (canvas, ruler) = match p.direction {
        RulerDirection::CanvasToRuler => (
            p.coordinates,
            std::array::from_fn(|i| {
                (p.coordinates[i] - metrics.origin[i]) / metrics.pixels_per_unit[i]
            }),
        ),
        RulerDirection::RulerToCanvas => (
            std::array::from_fn(|i| {
                metrics.origin[i] + p.coordinates[i] * metrics.pixels_per_unit[i]
            }),
            p.coordinates,
        ),
    };
    if canvas.iter().any(|v| !v.is_finite() || v.abs() > 1e12)
        || ruler.iter().any(|v| !v.is_finite() || v.abs() > 1e15)
    {
        return Err(invalid(
            "converted canvas must lie inside +/-1e12 pixels and ruler inside +/-1e15 units",
        ));
    }
    Ok(serde_json::to_value(RulerCoordinatesResult {
        canvas,
        ruler,
        metrics,
    })
    .unwrap())
}
#[derive(Clone)]
pub(super) struct SnapContext {
    _allocation: std::sync::Arc<strata_pixels::memory::Reservation>,
    state: StateId,
    layer: LayerId,
    origin: [i64; 2],
    bounds: CoverageRect,
    layer_snap: bool,
    references: Vec<(LayerId, CoverageRect)>,
}
impl std::fmt::Debug for SnapContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SnapContext")
            .field("state", &self.state)
            .field("layer", &self.layer)
            .field("bounds", &self.bounds)
            .field("references", &self.references.len())
            .finish_non_exhaustive()
    }
}
impl SnapContext {
    pub(super) fn includes_layers(&self) -> bool {
        self.layer_snap
    }
}
fn checked_bounds(b: CoverageRect) -> Result<CoverageRect, CommandError> {
    if [
        i128::from(b.x),
        i128::from(b.y),
        i128::from(b.x) + i128::from(b.width),
        i128::from(b.y) + i128::from(b.height),
    ]
    .iter()
    .any(|v| v.abs() > 1_000_000_000_000)
    {
        return Err(invalid(
            "snap source/reference bounds exceed canvas range +/-1e12",
        ));
    }
    Ok(b)
}
/// Scan each visible primitive once, sharing one visit/extent budget with both
/// moving content and reference candidates. Groups only union cached bounds.
pub(super) fn snap_context(
    doc: &Document,
    target: LayerId,
    layer_snap: bool,
    cancel: &dyn Fn() -> bool,
) -> Result<Option<SnapContext>, CommandError> {
    let allocation = std::sync::Arc::new(
        doc.memory_pool()
            .reserve(strata_pixels::memory::MemoryKind::Transient, 64 * 1024)
            .map_err(DocumentError::from)?,
    );
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
    let mut primitive = std::collections::HashMap::new();
    let mut visits = 0u64;
    let mut areas = 0u64;
    for layer in &doc.layers {
        if cancel() {
            return Err(CommandError::Expired);
        }
        let moving = layer.id() == target || doc.is_descendant(layer.id(), target);
        if !doc.effective_visibility(layer.id()) || (!moving && !layer_snap) {
            continue;
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
            _ => None,
        };
        if let Some(b) = content {
            let b = checked_bounds(b)?;
            areas += u64::from(b.width) * u64::from(b.height);
            if areas > 64_000_000 {
                return Err(DocumentError::ResourceLimit.into());
            }
            primitive.insert(layer.id(), b);
        }
    }
    let cached_bounds = |id| -> Result<Option<CoverageRect>, CommandError> {
        if let Some(b) = primitive.get(&id) {
            return Ok(Some(*b));
        }
        let mut bounds = None;
        for (child, b) in &primitive {
            if doc.is_descendant(*child, id) {
                bounds = Some(match bounds {
                    None => *b,
                    Some(old) => union(old, *b)?,
                });
            }
        }
        bounds.map(checked_bounds).transpose()
    };
    let bounds = cached_bounds(target)?;
    let mut references = Vec::new();
    if layer_snap {
        for layer in &doc.layers {
            if cancel() {
                return Err(CommandError::Expired);
            }
            // An ancestor containing the target must not snap it against itself.
            if layer.id() == target
                || doc.is_descendant(layer.id(), target)
                || doc.is_descendant(target, layer.id())
                || !doc.effective_visibility(layer.id())
            {
                continue;
            }
            if matches!(
                layer.info.kind,
                LayerKind::Group
                    | LayerKind::Pixel
                    | LayerKind::Text { .. }
                    | LayerKind::Solid { .. }
                    | LayerKind::Gradient { .. }
            ) && let Some(b) = cached_bounds(layer.id())?
            {
                references.push((layer.id(), b));
            }
        }
    }
    Ok(bounds.map(|bounds| SnapContext {
        _allocation: allocation,
        state: doc.state,
        layer: target,
        origin: [root.info.x, root.info.y],
        bounds,
        layer_snap,
        references,
    }))
}
#[derive(Clone)]
struct Candidate {
    ideal: f64,
    anchor: f64,
    coordinate: f64,
    guide_id: Option<GuideId>,
    reference_layer: Option<LayerId>,
    reference_anchor: Option<SnapAnchor>,
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
                targets.push((g.position, Some(g.id), SnapTarget::Guide, None, None))
            }
        }
    }
    if p.artboard {
        let a = [doc.frame.x, doc.frame.y][axis] as f64;
        let length = f64::from([doc.frame.width, doc.frame.height][axis]);
        targets.extend([
            (a, None, SnapTarget::ArtboardEdge, None, None),
            (a + length, None, SnapTarget::ArtboardEdge, None, None),
        ])
    }
    if p.layers {
        for (id, b) in &context.references {
            let start = [b.x, b.y][axis] as f64;
            let size = f64::from([b.width, b.height][axis]);
            for (coordinate, anchor) in [
                (start, SnapAnchor::Minimum),
                (start + size, SnapAnchor::Maximum),
                (start + size / 2., SnapAnchor::Center),
            ] {
                targets.push((
                    coordinate,
                    None,
                    SnapTarget::LayerAnchor,
                    Some(*id),
                    Some(anchor),
                ))
            }
        }
    }
    let mut candidates = Vec::new();
    for (coordinate, guide_id, kind, reference_layer, reference_anchor) in targets {
        for (order, anchor) in anchors.iter().copied().enumerate() {
            let ideal = coordinate - anchor;
            let resolved = (proposed as f64 + ideal).round();
            if ideal.abs() <= p.tolerance && resolved.is_finite() && resolved.abs() <= 1e12 {
                candidates.push(Candidate {
                    ideal,
                    anchor,
                    coordinate,
                    guide_id,
                    reference_layer,
                    reference_anchor,
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
            .then_with(|| target_order(a.kind).cmp(&target_order(b.kind)))
            .then_with(|| a.coordinate.total_cmp(&b.coordinate))
            .then_with(|| a.order.cmp(&b.order))
            .then_with(|| a.guide_id.cmp(&b.guide_id))
            .then_with(|| a.reference_layer.cmp(&b.reference_layer))
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
            reference_layer: candidate.reference_layer,
            reference_anchor: candidate.reference_anchor,
            source_anchor: match p.anchors {
                SnapAnchors::Center => SnapAnchor::Center,
                _ => match candidate.order {
                    0 => SnapAnchor::Minimum,
                    1 => SnapAnchor::Maximum,
                    _ => SnapAnchor::Center,
                },
            },
            coordinate: candidate.coordinate,
            anchor,
            residual: candidate.coordinate - anchor,
        },
    ))
}
fn target_order(target: SnapTarget) -> u8 {
    match target {
        SnapTarget::Guide => 0,
        SnapTarget::ArtboardEdge => 1,
        SnapTarget::LayerAnchor => 2,
    }
}
pub(super) fn snap(
    doc: &mut Document,
    params: Value,
    cached: Option<&SnapContext>,
    cancel: &dyn Fn() -> bool,
) -> Result<Value, CommandError> {
    let _scratch = doc
        .memory_pool()
        .reserve(strata_pixels::memory::MemoryKind::Transient, 1024 * 1024)
        .map_err(DocumentError::from)?;
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
    let context = if let Some(c) =
        cached.filter(|c| c.state == doc.state && c.layer == target && c.layer_snap == p.layers)
    {
        c.clone()
    } else {
        snap_context(doc, target, p.layers, cancel)?
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
