//! Bounded proximity texture matching followed by sampled Poisson healing.
//! This deliberately remains separate from content-aware PatchMatch synthesis.
use super::*;
use crate::{Document, DocumentError};
use serde_json::json;
use std::collections::HashSet;
use strata_pixels::{CoverageRect, TiledMask, memory::MemoryKind};

fn check(cancel: &dyn Fn() -> bool) -> Result<(), CommandError> {
    if cancel() {
        Err(CommandError::Expired)
    } else {
        Ok(())
    }
}
fn reduce(points: &[[i64; 2]], limit: usize) -> Vec<[i64; 2]> {
    let count = points.len().min(limit);
    (0..count)
        .map(|i| points[i * points.len() / count])
        .collect()
}
struct FootprintSamples {
    boundary: Vec<[i64; 2]>,
    interior: Vec<[i64; 2]>,
}
fn samples(
    mask: &TiledMask,
    b: CoverageRect,
    cancel: &dyn Fn() -> bool,
) -> Result<FootprintSamples, CommandError> {
    let mut boundary = Vec::new();
    let mut interior = Vec::new();
    for y in b.y - 1..=b.y + i64::from(b.height) {
        check(cancel)?;
        for x in b.x - 1..=b.x + i64::from(b.width) {
            if mask.sample(x, y).map_err(DocumentError::from)? > 0 {
                interior.push([x, y]);
            } else {
                let mut edge = false;
                for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                    edge |= mask.sample(x + dx, y + dy).map_err(DocumentError::from)? > 0;
                }
                if edge {
                    boundary.push([x, y]);
                }
            }
        }
    }
    Ok(FootprintSamples {
        boundary: reduce(&boundary, 128),
        interior: reduce(&interior, 32),
    })
}
struct Search<'a> {
    sampler: super::clone_source::CloneSampler,
    border: Vec<[i64; 2]>,
    interior: Vec<[i64; 2]>,
    reference: Vec<[f64; 4]>,
    bounds: [i64; 4],
    artboard: [i64; 4],
    radius: i64,
    visited: HashSet<[i64; 2]>,
    best: Option<([i64; 2], f64, f64)>,
    cancel: &'a dyn Fn() -> bool,
}
impl Search<'_> {
    fn candidate(&mut self, offset: [i64; 2]) -> Result<(), CommandError> {
        check(self.cancel)?;
        if offset.iter().any(|v| v.abs() > self.radius) || !self.visited.insert(offset) {
            return Ok(());
        }
        if self.visited.len() > 2048 {
            return Err(DocumentError::ResourceLimit.into());
        }
        let [dx, dy] = offset;
        let [l, t, r, b] = self.bounds;
        let [al, at, ar, ab] = self.artboard;
        if l + dx < al
            || t + dy < at
            || r + dx > ar
            || b + dy > ab
            || (l + dx < r && r + dx > l && t + dy < b && b + dy > t)
        {
            return Ok(());
        }
        let mut sum = [0.0; 3];
        let mut square = [0.0; 3];
        for (position, target) in self.border.iter().zip(&self.reference) {
            let source = self.sampler.destination_color(
                (position[0] + dx) as f64 + 0.5,
                (position[1] + dy) as f64 + 0.5,
                self.cancel,
            )?;
            if source[3] <= 1e-6 {
                return Ok(());
            }
            for c in 0..3 {
                let d = source[c] - target[c];
                sum[c] += d;
                square[c] += d * d;
            }
        }
        for position in &self.interior {
            if self.sampler.destination_color(
                (position[0] + dx) as f64 + 0.5,
                (position[1] + dy) as f64 + 0.5,
                self.cancel,
            )?[3]
                <= 1e-6
            {
                return Ok(());
            }
        }
        let n = self.border.len() as f64;
        let error = (0..3)
            .map(|c| {
                let mean = sum[c] / n;
                (square[c] / n - mean * mean).max(0.0) + 0.05 * mean * mean
            })
            .sum::<f64>()
            / 3.0;
        let distance = (dx * dx + dy * dy) as f64 / (self.radius * self.radius) as f64;
        let score = error + distance * 1e-9;
        if score.is_finite() && self.best.is_none_or(|(_, old, _)| score < old) {
            self.best = Some((offset, score, error.sqrt()));
        }
        Ok(())
    }
}
pub(super) fn mutate(
    doc: &mut Document,
    id: &str,
    params: Value,
    cancel: &dyn Fn() -> bool,
) -> Result<Option<Value>, CommandError> {
    if id != "retouch.spot_heal" {
        return Ok(None);
    }
    check(cancel)?;
    let p: SpotHealStroke = parse(params)?;
    super::heal::validate_limits(p.max_iterations, p.tolerance)?;
    if !(1..=512).contains(&p.search_radius)
        || p.stroke.clone_source.is_some()
        || !matches!(p.stroke.target, BrushTarget::Pixels)
    {
        return Err(CommandError::Invalid(
            "spot healing requires pixel target, search_radius 1..512 and no explicit clone_source"
                .into(),
        ));
    }
    let (target, coverage) = super::heal::footprint(doc, &p.stroke, cancel)?;
    let Some(bounds) = coverage.content_bounds().map_err(DocumentError::from)? else {
        return Ok(Some(
            json!({"layer_id":target,"source":null,"candidates":0,"boundary_error":0.0,"healing":null}),
        ));
    };
    let area = (u64::from(bounds.width) + 2) * (u64::from(bounds.height) + 2);
    if area > 262144 {
        return Err(CommandError::Invalid(
            "spot healing footprint plus boundary exceeds 262144 pixels; split the stroke".into(),
        ));
    }
    let _budget = doc
        .memory_pool()
        .reserve(MemoryKind::Transient, area * 32 + 8 * 1024 * 1024)
        .map_err(DocumentError::from)?;
    let FootprintSamples {
        boundary: border,
        interior,
    } = samples(&coverage, bounds, cancel)?;
    let anchor = [
        bounds.x as f64 + f64::from(bounds.width) / 2.0,
        bounds.y as f64 + f64::from(bounds.height) / 2.0,
    ];
    let source = CloneSource {
        source: anchor,
        destination: Some(anchor),
        sample: p.sample,
    };
    let first = BrushPoint {
        x: anchor[0],
        y: anchor[1],
        pressure: 1.0,
    };
    let mut sampler = super::clone_source::CloneSampler::new(doc, target, &source, &first)?;
    sampler.limit_tile_loads(1024);
    let f = doc.frame();
    let artboard = [
        f.x,
        f.y,
        f.x + i64::from(f.width),
        f.y + i64::from(f.height),
    ];
    let mut valid_border = Vec::new();
    let mut reference = Vec::new();
    for position in border {
        check(cancel)?;
        if position[0] < artboard[0]
            || position[1] < artboard[1]
            || position[0] >= artboard[2]
            || position[1] >= artboard[3]
        {
            continue;
        }
        let value = sampler.destination_color(
            position[0] as f64 + 0.5,
            position[1] as f64 + 0.5,
            cancel,
        )?;
        if value[3] > 1e-6 {
            valid_border.push(position);
            reference.push(value);
        }
    }
    if valid_border.len() < 4 {
        return Err(CommandError::Invalid("spot healing needs at least four nontransparent boundary samples; use merged sampling or a smaller stroke".into()));
    }
    let radius = i64::from(p.search_radius);
    let step = (radius + 15) / 16;
    let mut search = Search {
        sampler,
        border: valid_border,
        interior,
        reference,
        bounds: [
            bounds.x - 1,
            bounds.y - 1,
            bounds.x + i64::from(bounds.width) + 1,
            bounds.y + i64::from(bounds.height) + 1,
        ],
        artboard,
        radius,
        visited: HashSet::new(),
        best: None,
        cancel,
    };
    let mut coordinates = (-16..=16)
        .map(|v| v * step)
        .filter(|v| v.abs() <= radius)
        .collect::<Vec<_>>();
    coordinates.extend([-radius, 0, radius]);
    coordinates.sort_unstable();
    coordinates.dedup();
    for &dy in &coordinates {
        for &dx in &coordinates {
            search.candidate([dx, dy])?;
        }
    }
    let mut refinement = step / 2;
    while refinement > 0 {
        if let Some(([x, y], _, _)) = search.best {
            for dy in [-refinement, 0, refinement] {
                for dx in [-refinement, 0, refinement] {
                    search.candidate([x + dx, y + dy])?;
                }
            }
        }
        refinement /= 2;
    }
    let Some(([dx, dy], _, error)) = search.best else {
        return Err(CommandError::Invalid("no non-overlapping nontransparent source patch in search radius; increase search_radius, reduce brush size or use sampled healing".into()));
    };
    let candidates = search.visited.len();
    drop(search);
    drop(coverage);
    drop(_budget);
    let source = [anchor[0] + dx as f64, anchor[1] + dy as f64];
    let mut stroke = p.stroke;
    stroke.clone_source = Some(CloneSource {
        source,
        destination: Some(anchor),
        sample: p.sample,
    });
    let healing = super::heal::mutate(
        doc,
        "retouch.heal",
        serde_json::to_value(HealStroke {
            stroke,
            max_iterations: p.max_iterations,
            tolerance: p.tolerance,
        })
        .map_err(|e| CommandError::Invalid(e.to_string()))?,
        cancel,
    )?
    .expect("healing command");
    Ok(Some(
        json!({"layer_id":target,"source":source,"candidates":candidates,"boundary_error":error,"healing":healing}),
    ))
}
