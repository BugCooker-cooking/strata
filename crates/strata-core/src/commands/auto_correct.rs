//! Deterministic automatic RGB corrections from alpha/selection weighted,
//! exact 16-bit code histograms. Full coverage maps codes directly; only soft
//! coverage blends in premultiplied linear light through the real working ICC.
use super::*;
use crate::{Document, DocumentError};
use strata_pixels::{CoverageRect, PixelBuffer, TileCoord, memory::MemoryKind};
const BINS: usize = 65536;
const MAX_VISITS: u64 = 64_000_000;
fn check(cancel: &dyn Fn() -> bool) -> Result<(), CommandError> {
    if cancel() {
        Err(CommandError::Expired)
    } else {
        Ok(())
    }
}
fn invalid(message: &str) -> CommandError {
    CommandError::Invalid(message.into())
}
fn codes(p: &PixelBuffer, i: usize) -> [u16; 4] {
    match p {
        PixelBuffer::Rgba8(v) => std::array::from_fn(|c| u16::from(v[4 * i + c]) * 257),
        PixelBuffer::Rgba16(v) => std::array::from_fn(|c| v[4 * i + c]),
    }
}
fn write_rgb(p: &mut PixelBuffer, i: usize, rgb: [f64; 3]) {
    let code = |value: f64, max: f64| {
        let scaled = value.clamp(0., 1.) * max;
        // Quantize mathematical half-code ties toward the upper code even if
        // pow/log roundoff lands infinitesimally below the tie on one channel.
        let doubled = scaled * 2.;
        let snapped = if (doubled - doubled.round()).abs() <= 1e-9 {
            doubled.round() / 2.
        } else {
            scaled
        };
        snapped.round() as u16
    };
    match p {
        PixelBuffer::Rgba8(v) => {
            for c in 0..3 {
                v[4 * i + c] = code(rgb[c], 255.) as u8;
            }
        }
        PixelBuffer::Rgba16(v) => {
            for c in 0..3 {
                v[4 * i + c] = code(rgb[c], 65535.);
            }
        }
    }
}
fn tile_rect(
    coord: TileCoord,
    offset: [i64; 2],
    bounds: CoverageRect,
) -> Result<Option<[i64; 4]>, CommandError> {
    let [x, y] = coord.origin();
    let gx = x
        .checked_add(offset[0])
        .ok_or(DocumentError::ResourceLimit)?;
    let gy = y
        .checked_add(offset[1])
        .ok_or(DocumentError::ResourceLimit)?;
    let r = gx
        .checked_add(256)
        .ok_or(DocumentError::ResourceLimit)?
        .min(bounds.x + i64::from(bounds.width));
    let b = gy
        .checked_add(256)
        .ok_or(DocumentError::ResourceLimit)?
        .min(bounds.y + i64::from(bounds.height));
    let l = gx.max(bounds.x);
    let t = gy.max(bounds.y);
    Ok((l < r && t < b).then_some([l, t, r, b]))
}
/// Exact floor(total * percent / 100) for the supplied binary f64. The
/// u16 alpha*coverage sum can exceed 2^53, so never round that total via f64.
fn percentile_weight(total: u64, percent: f64) -> u64 {
    if total == 0 || percent == 0. {
        return 0;
    }
    let bits = percent.to_bits();
    let exponent = ((bits >> 52) & 2047) as i32;
    let fraction = bits & ((1u64 << 52) - 1);
    let (mantissa, power) = if exponent == 0 {
        (fraction, -1074)
    } else {
        (fraction | (1u64 << 52), exponent - 1023 - 52)
    };
    let product = u128::from(total) * u128::from(mantissa);
    let shift = (-power) as u32;
    if shift > 120 {
        return 0;
    }
    let divisor = 100u128 << shift;
    (product / divisor) as u64
}
/// Smallest occupied code with cumulative weight > threshold. Endpoints are
/// exact min/max; integer histogram accumulation is traversal-independent.
fn quantile(threshold: u64, mut weight: impl FnMut(usize) -> u64) -> u16 {
    let mut cumulative = 0;
    let mut last = 0;
    for i in 0..BINS {
        let w = weight(i);
        if w > 0 {
            last = i;
        }
        cumulative += w;
        if cumulative > threshold {
            return i as u16;
        }
    }
    last as u16
}
pub(super) fn mutate(
    doc: &mut Document,
    id: &str,
    params: Value,
    cancel: &dyn Fn() -> bool,
) -> Result<Option<Value>, CommandError> {
    if id != "image.auto" {
        return Ok(None);
    }
    check(cancel)?;
    let p: AutoCorrect = parse(params)?;
    let (method, clip, neutralize) = match p.settings {
        AutoCorrectionSettings::Contrast { clip_percent } => {
            (AutoCorrectionMode::Contrast, clip_percent, 0.)
        }
        AutoCorrectionSettings::Tone { clip_percent } => {
            (AutoCorrectionMode::Tone, clip_percent, 0.)
        }
        AutoCorrectionSettings::Color {
            clip_percent,
            neutralize_strength,
        } => (AutoCorrectionMode::Color, clip_percent, neutralize_strength),
    };
    if !clip.is_finite()
        || !(0.0..=5.0).contains(&clip)
        || !neutralize.is_finite()
        || !(0.0..=1.0).contains(&neutralize)
    {
        return Err(invalid(
            "automatic correction needs finite clip_percent 0..5 per tail and color neutralize_strength 0..1",
        ));
    }
    let (target, index) = super::pixel_transform::target(doc, &p.layer)?;
    if !doc.effective_visibility(target) {
        return Err(invalid(
            "automatic correction statistics require a visible pixel layer and visible ancestors",
        ));
    }
    let mut result = AutoCorrectionResult {
        layer_id: target,
        method,
        clip_percent: clip,
        neutralize_strength: neutralize,
        input_black: [0.; 3],
        input_white: [1.; 3],
        gamma: [1.; 3],
        sampled_pixels: 0,
        weighted_samples: 0.,
        touched_tiles: 0,
        changed: false,
    };
    let coverage = doc
        .selection()
        .cloned()
        .map(Ok)
        .unwrap_or_else(|| super::local::uniform(doc, 65535))?;
    let Some(selection) = crate::geometry_masks::coverage_bounds(&coverage, cancel)? else {
        return Ok(Some(serde_json::to_value(result).unwrap()));
    };
    let frame = doc.frame();
    let l = frame.x.max(selection.x);
    let t = frame.y.max(selection.y);
    let r = (frame.x + i64::from(frame.width)).min(selection.x + i64::from(selection.width));
    let b = (frame.y + i64::from(frame.height)).min(selection.y + i64::from(selection.height));
    if l >= r || t >= b || doc.layers[index].pixels().tile_count() == 0 {
        return Ok(Some(serde_json::to_value(result).unwrap()));
    }
    let bounds = CoverageRect {
        x: l,
        y: t,
        width: (r - l) as u32,
        height: (b - t) as u32,
    };
    let info = doc.layers[index].info();
    let offset = [info.x, info.y];
    let pool = doc.memory_pool();
    let _hist_budget = pool
        .reserve(MemoryKind::Transient, (3 * BINS * 8 + 8192) as u64)
        .map_err(DocumentError::from)?;
    let _index_budget = pool
        .reserve(
            MemoryKind::Transient,
            doc.layers[index].pixels().tile_count() as u64 * 128
                + doc.working_icc().len() as u64 * 8
                + 8192,
        )
        .map_err(DocumentError::from)?;
    let source = doc.layers[index].pixels().clone();
    let mut hist = vec![vec![0u64; BINS]; 3];
    let mut total = 0u64;
    let mut visits = 0u64;
    for coord in source.tile_coords().copied() {
        check(cancel)?;
        let Some([l, t, r, b]) = tile_rect(coord, offset, bounds)? else {
            continue;
        };
        visits = visits
            .checked_add(((r - l) * (b - t)) as u64)
            .ok_or(DocumentError::ResourceLimit)?;
        if visits > MAX_VISITS {
            return Err(DocumentError::ResourceLimit.into());
        }
        let _scratch = pool
            .reserve(MemoryKind::Transient, 256 * 256 * 2)
            .map_err(DocumentError::from)?;
        let raw = source
            .tile(coord)
            .map_err(DocumentError::from)?
            .expect("stored source coordinate");
        let [ox, oy] = coord.origin();
        let (gx, gy) = (ox + offset[0], oy + offset[1]);
        let weights = coverage.region_tile(gx, gy).map_err(DocumentError::from)?;
        for y in t..b {
            check(cancel)?;
            for x in l..r {
                let i = (y - gy) as usize * 256 + (x - gx) as usize;
                let p = codes(&raw, i);
                let weight = u64::from(weights[i]) * u64::from(p[3]);
                if weight == 0 {
                    continue;
                }
                total = total
                    .checked_add(weight)
                    .ok_or(DocumentError::ResourceLimit)?;
                result.sampled_pixels += 1;
                for c in 0..3 {
                    hist[c][usize::from(p[c])] += weight;
                }
            }
        }
    }
    if total == 0 {
        return Ok(Some(serde_json::to_value(result).unwrap()));
    }
    result.weighted_samples = total as f64 / (65535. * 65535.);
    let mut black = [0u16; 3];
    let mut white = [65535u16; 3];
    for c in 0..3 {
        check(cancel)?;
        let (low, high) = if matches!(method, AutoCorrectionMode::Contrast) {
            let pooled = |i| hist.iter().map(|h| h[i]).sum::<u64>();
            let tail = percentile_weight(total * 3, clip);
            (
                quantile(tail, pooled),
                quantile(total * 3 - tail - 1, pooled),
            )
        } else {
            let tail = percentile_weight(total, clip);
            (
                quantile(tail, |i| hist[c][i]),
                quantile(total - tail - 1, |i| hist[c][i]),
            )
        };
        black[c] = low;
        white[c] = high;
        result.input_black[c] = f64::from(low) / 65535.;
        result.input_white[c] = f64::from(high) / 65535.;
        if matches!(method, AutoCorrectionMode::Color) && neutralize > 0. && high > low {
            let median = quantile(total / 2, |i| hist[c][i]);
            let midpoint =
                (f64::from(median) - f64::from(low)) / (f64::from(high) - f64::from(low));
            if midpoint > 0. && midpoint < 1. {
                let target_gamma = (midpoint.ln() / 0.5f64.ln()).clamp(0.1, 10.);
                result.gamma[c] = (target_gamma.ln() * neutralize).exp();
            }
        }
    }
    if (0..3).all(|c| {
        white[c] <= black[c] || (black[c] == 0 && white[c] == 65535 && result.gamma[c] == 1.)
    }) {
        check(cancel)?;
        return Ok(Some(serde_json::to_value(result).unwrap()));
    }
    let _lut_budget = pool
        .reserve(MemoryKind::Transient, (3 * BINS * 8) as u64)
        .map_err(DocumentError::from)?;
    let mut lut = vec![vec![0.; BINS]; 3];
    for c in 0..3 {
        for (i, value) in lut[c].iter_mut().enumerate() {
            if i % 4096 == 0 {
                check(cancel)?;
            }
            *value = if white[c] <= black[c] {
                i as f64 / 65535.
            } else {
                ((i as f64 - f64::from(black[c])) / f64::from(white[c] - black[c]))
                    .clamp(0., 1.)
                    .powf(1. / result.gamma[c])
            };
        }
    }
    drop(hist);
    drop(_hist_budget);
    for coord in source.tile_coords().copied() {
        check(cancel)?;
        let Some([l, t, r, b]) = tile_rect(coord, offset, bounds)? else {
            continue;
        };
        let _scratch = pool
            .reserve(MemoryKind::Transient, 256 * 256 * 128)
            .map_err(DocumentError::from)?;
        let raw = source
            .tile(coord)
            .map_err(DocumentError::from)?
            .expect("stored source coordinate");
        let mut output = (*raw).clone();
        let [ox, oy] = coord.origin();
        let (gx, gy) = (ox + offset[0], oy + offset[1]);
        let weights = coverage.region_tile(gx, gy).map_err(DocumentError::from)?;
        let mut front = vec![[0.0f32; 4]; 256 * 256];
        let mut soft = vec![false; 256 * 256];
        for y in t..b {
            check(cancel)?;
            for x in l..r {
                let i = (y - gy) as usize * 256 + (x - gx) as usize;
                let p = codes(&raw, i);
                if weights[i] == 0 || p[3] == 0 {
                    continue;
                }
                let rgb = std::array::from_fn(|c| lut[c][usize::from(p[c])]);
                if (0..3).all(|c| rgb[c] == f64::from(p[c]) / 65535.) {
                    continue;
                }
                if weights[i] == 65535 {
                    write_rgb(&mut output, i, rgb);
                } else {
                    front[i] = [
                        rgb[0] as f32,
                        rgb[1] as f32,
                        rgb[2] as f32,
                        f32::from(p[3]) / 65535.,
                    ];
                    soft[i] = true;
                }
            }
        }
        if soft.iter().any(|value| *value) {
            check(cancel)?;
            let mut back = strata_color::to_linear_p3(&raw, 256, 256, doc.working_icc())
                .map_err(DocumentError::from)?;
            let front = strata_color::working_to_linear_p3(&front, doc.working_icc())
                .map_err(DocumentError::from)?;
            for row in 0..256 {
                check(cancel)?;
                for col in 0..256 {
                    let i = row * 256 + col;
                    if soft[i] {
                        let w = f64::from(weights[i]) / 65535.;
                        for c in 0..3 {
                            back[i][c] = (f64::from(back[i][c]) * (1. - w)
                                + f64::from(front[i][c]) * w)
                                as f32;
                        }
                    }
                }
            }
            let converted = strata_color::from_linear_p3(&back, doc.format, doc.working_icc())
                .map_err(DocumentError::from)?;
            for (i, changed) in soft.into_iter().enumerate() {
                if changed {
                    super::brush::copy_color(&mut output, &converted, i);
                }
            }
        }
        if output != *raw {
            check(cancel)?;
            doc.layers[index]
                .pixels
                .insert_tile(coord, output)
                .map_err(DocumentError::from)?;
            result.touched_tiles += 1;
        }
    }
    check(cancel)?;
    result.changed = result.touched_tiles > 0;
    Ok(Some(serde_json::to_value(result).unwrap()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn percentile_rank_keeps_low_weight_units_beyond_f64_integer_precision() {
        for total in [
            9_007_199_254_740_993u64,
            (64_000_000u64 * 65535 * 65535) * 3,
        ] {
            assert_eq!(percentile_weight(total, 0.), 0);
            assert_eq!(percentile_weight(total, 0.5), total / 200);
            assert_eq!(percentile_weight(total, 5.), total / 20);
            assert_eq!(percentile_weight(total, f64::from_bits(1)), 0);
        }
        let threshold = 9_007_199_254_740_993u64;
        let found = quantile(threshold, |index| match index {
            100 => threshold,
            200 => 1,
            _ => 0,
        });
        assert_eq!(found, 200);
    }
}
