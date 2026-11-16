//! Gradient-domain healing with an immutable source and target boundary.
//! The finite footprint and solver residual are explicit resource/quality gates.
use super::*;
use crate::{Document, DocumentError, LayerId, LayerKind};
use serde_json::json;
use strata_pixels::memory::MemoryKind;
fn check(cancel: &dyn Fn() -> bool) -> Result<(), CommandError> {
    if cancel() {
        Err(CommandError::Expired)
    } else {
        Ok(())
    }
}
fn dot(a: &[[f64; 3]], b: &[[f64; 3]]) -> f64 {
    a.iter()
        .zip(b)
        .map(|(a, b)| a.iter().zip(b).map(|(a, b)| a * b).sum::<f64>())
        .sum()
}
fn apply_matrix(
    values: &[[f64; 3]],
    unknown: &[usize],
    ids: &[i32],
    width: usize,
    out: &mut [[f64; 3]],
    cancel: &dyn Fn() -> bool,
) -> Result<(), CommandError> {
    for (n, &i) in unknown.iter().enumerate() {
        if n.is_multiple_of(1024) {
            check(cancel)?;
        }
        out[n] = values[n].map(|v| v * 4.0);
        for j in [i - 1, i + 1, i - width, i + width] {
            if ids[j] >= 0 {
                for c in 0..3 {
                    out[n][c] -= values[ids[j] as usize][c];
                }
            }
        }
    }
    Ok(())
}
pub(super) struct SolveLimits {
    pub iterations: u32,
    pub tolerance: f64,
}
pub(super) enum Guide {
    Sampled(CloneSource),
    Synthesized {
        mode: SpotHealMode,
        sample: SelectionSample,
        radius: u32,
    },
}

fn solve(
    rhs: &[[f64; 3]],
    initial: Vec<[f64; 3]>,
    unknown: &[usize],
    ids: &[i32],
    width: usize,
    limits: SolveLimits,
    cancel: &dyn Fn() -> bool,
) -> Result<(Vec<[f64; 3]>, u32, f64), CommandError> {
    let mut value = initial;
    let mut product = vec![[0.0; 3]; value.len()];
    apply_matrix(&value, unknown, ids, width, &mut product, cancel)?;
    let mut residual = rhs
        .iter()
        .zip(&product)
        .map(|(a, b)| std::array::from_fn(|c| a[c] - b[c]))
        .collect::<Vec<[f64; 3]>>();
    let mut direction = residual.clone();
    let mut norm = dot(&residual, &residual);
    let threshold = limits.tolerance * limits.tolerance * (value.len() * 3) as f64;
    for step in 0..=limits.iterations {
        check(cancel)?;
        let rms = (norm / (value.len() * 3) as f64).sqrt();
        if norm <= threshold {
            return Ok((value, step, rms));
        }
        if step == limits.iterations {
            break;
        }
        apply_matrix(&direction, unknown, ids, width, &mut product, cancel)?;
        let denominator = dot(&direction, &product);
        if !denominator.is_finite() || denominator <= 0.0 {
            return Err(CommandError::Invalid(
                "healing solver could not preserve a finite positive domain".into(),
            ));
        }
        let alpha = norm / denominator;
        for i in 0..value.len() {
            for c in 0..3 {
                value[i][c] += alpha * direction[i][c];
                residual[i][c] -= alpha * product[i][c];
            }
        }
        let next = dot(&residual, &residual);
        let beta = next / norm;
        for i in 0..value.len() {
            for c in 0..3 {
                direction[i][c] = residual[i][c] + beta * direction[i][c];
            }
        }
        norm = next;
    }
    Err(CommandError::Invalid(format!(
        "healing did not converge within {} iterations (RMS {:.6}); shorten the stroke or increase max_iterations",
        limits.iterations,
        (norm / (value.len() * 3) as f64).sqrt()
    )))
}
pub(super) fn validate_limits(iterations: u32, tolerance: f64) -> Result<(), CommandError> {
    if !(1..=2048).contains(&iterations)
        || !tolerance.is_finite()
        || !(1e-7..=0.001).contains(&tolerance)
    {
        return Err(CommandError::Invalid(
            "healing max_iterations 1..2048 and tolerance 1e-7..0.001 required".into(),
        ));
    }
    Ok(())
}
pub(super) fn footprint(
    doc: &Document,
    stroke: &BrushStroke,
    cancel: &dyn Fn() -> bool,
) -> Result<(LayerId, strata_pixels::TiledMask), CommandError> {
    check(cancel)?;
    if !matches!(stroke.target, BrushTarget::Pixels) {
        return Err(CommandError::Invalid(
            "healing requires a pixel layer target".into(),
        ));
    }
    let target = doc.resolve(&stroke.layer)?;
    let index = doc.layers().iter().position(|l| l.id() == target).unwrap();
    if !matches!(doc.layers()[index].info().kind, LayerKind::Pixel) {
        return Err(CommandError::Invalid("healing needs a pixel target".into()));
    }
    // Reuse the exact brush rasterizer on a temporary coverage plane. The
    // original target pixels and existing layer mask remain immutable.
    let mut footprint = doc.clone();
    let mask = super::local::uniform(doc, 0)?;
    super::local::attach(&mut footprint.layers[index], mask);
    let mut stroke = stroke.clone();
    stroke.target = BrushTarget::Mask;
    stroke.value = 1.0;
    stroke.clone_source = None;
    super::brush::mutate_cancellable(
        &mut footprint,
        "paint.stroke",
        serde_json::to_value(&stroke).map_err(|e| CommandError::Invalid(e.to_string()))?,
        cancel,
    )?;
    Ok((
        target,
        footprint.layers[index]
            .mask
            .take()
            .expect("temporary coverage"),
    ))
}
pub(super) fn mutate(
    doc: &mut Document,
    id: &str,
    params: Value,
    cancel: &dyn Fn() -> bool,
) -> Result<Option<Value>, CommandError> {
    if id != "retouch.heal" {
        return Ok(None);
    }
    check(cancel)?;
    let mut p: HealStroke = parse(params)?;
    validate_limits(p.max_iterations, p.tolerance)?;
    let clone =
        p.stroke.clone_source.take().ok_or_else(|| {
            CommandError::Invalid("healing needs an explicit clone_source".into())
        })?;
    let (target, coverage) = footprint(doc, &p.stroke, cancel)?;
    apply(
        doc,
        &p.stroke,
        (target, coverage),
        Guide::Sampled(clone),
        SolveLimits {
            iterations: p.max_iterations,
            tolerance: p.tolerance,
        },
        cancel,
    )
    .map(Some)
}

pub(super) fn apply(
    doc: &mut Document,
    stroke: &BrushStroke,
    (target, coverage): (LayerId, strata_pixels::TiledMask),
    guide: Guide,
    limits: SolveLimits,
    cancel: &dyn Fn() -> bool,
) -> Result<Value, CommandError> {
    let method = match guide {
        Guide::Sampled(_) => {
            "linear P3 Poisson source gradients with immutable target Dirichlet boundary"
        }
        Guide::Synthesized {
            mode: SpotHealMode::CreateTexture,
            ..
        } => "boundary texture synthesis with linear P3 Poisson blending",
        Guide::Synthesized { .. } => {
            "directional content reconstruction with linear P3 Poisson blending"
        }
    };
    let result = |pixels, steps, rms| json!({"layer_id":target,"domain_pixels":pixels,"iterations":steps,"residual_rms":rms,"method":method});
    let Some(bounds) = coverage.content_bounds().map_err(DocumentError::from)? else {
        return Ok(result(0, 0, 0.0));
    };
    let width = bounds.width as usize + 2;
    let height = bounds.height as usize + 2;
    let area = width
        .checked_mul(height)
        .ok_or(DocumentError::ResourceLimit)?;
    if area > 262144 {
        return Err(CommandError::Invalid(
            "healing footprint plus boundary exceeds 262144 pixels; split into shorter strokes"
                .into(),
        ));
    }
    let _budget = doc
        .memory_pool()
        .reserve(MemoryKind::Transient, area as u64 * 256 + 8 * 1024 * 1024)
        .map_err(DocumentError::from)?;
    let first = stroke
        .points
        .first()
        .ok_or_else(|| CommandError::Invalid("empty healing path".into()))?;
    let clone = match &guide {
        Guide::Sampled(clone) => clone.clone(),
        Guide::Synthesized { sample, .. } => CloneSource {
            source: [first.x, first.y],
            destination: None,
            sample: *sample,
        },
    };
    let mut sampler = super::clone_source::CloneSampler::new(doc, target, &clone, first)?;
    let left = bounds
        .x
        .checked_sub(1)
        .ok_or(DocumentError::ResourceLimit)?;
    let top = bounds
        .y
        .checked_sub(1)
        .ok_or(DocumentError::ResourceLimit)?;
    let f = doc.frame();
    let right = f.x + i64::from(f.width) - 1;
    let bottom = f.y + i64::from(f.height) - 1;
    let mut source = vec![[0.0; 4]; area];
    let mut destination = vec![[0.0; 4]; area];
    let mut ids = vec![-1i32; area];
    let mut unknown = Vec::new();
    for y in 0..height {
        check(cancel)?;
        for x in 0..width {
            let i = y * width + x;
            let gx = left + x as i64;
            let gy = top + y as i64;
            destination[i] = sampler.destination_color(
                gx.clamp(f.x, right) as f64 + 0.5,
                gy.clamp(f.y, bottom) as f64 + 0.5,
                cancel,
            )?;
            source[i] = if matches!(guide, Guide::Sampled(_)) {
                sampler.color(gx as f64 + 0.5, gy as f64 + 0.5, cancel)?
            } else {
                destination[i]
            };
            if x > 0
                && x + 1 < width
                && y > 0
                && y + 1 < height
                && source[i][3] > 1e-6
                && destination[i][3] > 1e-6
                && coverage.sample(gx, gy).map_err(DocumentError::from)? > 0
            {
                ids[i] = unknown.len() as i32;
                unknown.push(i);
            }
        }
    }
    if unknown.is_empty() {
        return Ok(result(0, 0, 0.0));
    }
    drop(sampler);
    if let Guide::Synthesized { mode, radius, .. } = guide {
        super::spot_synthesis::guide(mode, &destination, &ids, width, radius, &mut source, cancel)?;
    }
    let mut rhs = vec![[0.0; 3]; unknown.len()];
    let mut initial = Vec::with_capacity(unknown.len());
    for (n, &i) in unknown.iter().enumerate() {
        if n.is_multiple_of(1024) {
            check(cancel)?;
        }
        initial.push([destination[i][0], destination[i][1], destination[i][2]]);
        for j in [i - 1, i + 1, i - width, i + width] {
            for c in 0..3 {
                rhs[n][c] += if source[j][3] > 1e-6 {
                    source[i][c] - source[j][c]
                } else {
                    0.0
                };
                if ids[j] < 0 {
                    rhs[n][c] += destination[j][c];
                }
            }
        }
    }
    let (solved, iterations, residual) =
        solve(&rhs, initial, &unknown, &ids, width, limits, cancel)?;
    let mut linear = vec![[0.0f32; 4]; area];
    for (n, &i) in unknown.iter().enumerate() {
        linear[i] = [
            solved[n][0] as f32,
            solved[n][1] as f32,
            solved[n][2] as f32,
            source[i][3] as f32,
        ];
    }
    let colors = strata_color::linear_p3_to_working(&linear, doc.working_icc())
        .map_err(DocumentError::from)?;
    let color = |x: i64, y: i64| {
        let x = i128::from(x) - i128::from(left);
        let y = i128::from(y) - i128::from(top);
        if x < 0 || y < 0 || x >= width as i128 || y >= height as i128 {
            [0.0; 4]
        } else {
            colors[y as usize * width + x as usize].map(|v| f64::from(v).clamp(0.0, 1.0))
        }
    };
    // Coverage already includes the current selection, pressure and whole-
    // stroke opacity. Do not multiply soft selection a second time at publish.
    let selection = doc.selection.take();
    let applied = super::paint::fill(
        doc,
        stroke.layer.clone(),
        1.0,
        Some(&coverage),
        color,
        cancel,
    );
    doc.selection = selection;
    applied?;
    Ok(result(unknown.len(), iterations, residual))
}
