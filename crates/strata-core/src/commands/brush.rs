use super::*;
use crate::{Document, DocumentError, LayerKind};
use serde_json::json;
use strata_pixels::{CoverageRect, PixelBuffer, PixelFormat, TileCoord, memory::MemoryKind};

const MAX_DABS: usize = 1_000_000;
#[derive(Clone, Copy)]
struct Dab {
    x: f64,
    y: f64,
    radius: f64,
    pressure: f64,
}
fn check(cancel: &dyn Fn() -> bool) -> Result<(), CommandError> {
    if cancel() {
        Err(CommandError::Expired)
    } else {
        Ok(())
    }
}
fn validate(p: &BrushStroke) -> Result<usize, CommandError> {
    let unit = |v: f64| v.is_finite() && (0.0..=1.0).contains(&v);
    if !(1..=8192).contains(&p.points.len())
        || p.points.iter().any(|v| {
            !v.x.is_finite()
                || !v.y.is_finite()
                || v.x.abs() > 1e12
                || v.y.abs() > 1e12
                || !unit(v.pressure)
        })
        || !p.size.is_finite()
        || !(0.5..=4096.0).contains(&p.size)
        || !unit(p.hardness)
        || !unit(p.opacity)
        || !unit(p.flow)
        || !unit(p.value)
        || !p.spacing.is_finite()
        || !(0.01..=2.0).contains(&p.spacing)
        || p.color.into_iter().any(|v| !unit(v))
    {
        return Err(CommandError::Invalid("brush needs 1..8192 finite canvas points ±1e12, pressure/color/hardness/opacity/flow/value 0..1, size 0.5..4096 and spacing 0.01..2 diameters".into()));
    }
    let mut count = 1usize;
    for pair in p.points.windows(2) {
        let distance = (pair[1].x - pair[0].x).hypot(pair[1].y - pair[0].y);
        let minimum_size = if p.pressure_size {
            (p.size * pair[0].pressure.min(pair[1].pressure)).max(0.25)
        } else {
            p.size
        };
        let needed = (distance / (minimum_size * p.spacing)).ceil();
        if needed > MAX_DABS as f64 || count.saturating_add(needed as usize + 1) > MAX_DABS {
            return Err(DocumentError::ResourceLimit.into());
        }
        count += needed as usize + 1;
    }
    Ok(count)
}
fn dabs(
    p: &BrushStroke,
    capacity: usize,
    cancel: &dyn Fn() -> bool,
) -> Result<Vec<Dab>, CommandError> {
    let dab = |x: f64, y: f64, pressure: f64| Dab {
        x,
        y,
        radius: if p.pressure_size {
            (p.size * pressure).max(0.25) / 2.0
        } else {
            p.size / 2.0
        },
        pressure,
    };
    let mut out = Vec::with_capacity(capacity);
    out.push(dab(p.points[0].x, p.points[0].y, p.points[0].pressure));
    let mut remaining = out[0].radius * 2.0 * p.spacing;
    for pair in p.points.windows(2) {
        check(cancel)?;
        let a = &pair[0];
        let b = &pair[1];
        let distance = (b.x - a.x).hypot(b.y - a.y);
        if distance == 0.0 {
            continue;
        }
        let mut offset = remaining;
        while offset <= distance {
            if out.len() >= capacity {
                return Err(DocumentError::ResourceLimit.into());
            }
            if out.len().is_multiple_of(1024) {
                check(cancel)?;
            }
            let t = offset / distance;
            let pressure = a.pressure + (b.pressure - a.pressure) * t;
            let sample = dab(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t, pressure);
            offset += sample.radius * 2.0 * p.spacing;
            out.push(sample);
        }
        remaining = offset - distance;
    }
    Ok(out)
}
fn intersection(a: CoverageRect, b: CoverageRect) -> Option<CoverageRect> {
    let x = a.x.max(b.x);
    let y = a.y.max(b.y);
    let right = (a.x + i64::from(a.width)).min(b.x + i64::from(b.width));
    let bottom = (a.y + i64::from(a.height)).min(b.y + i64::from(b.height));
    (right > x && bottom > y).then(|| CoverageRect {
        x,
        y,
        width: (right - x) as u32,
        height: (bottom - y) as u32,
    })
}
fn plane_bounds(frame: CoverageRect, offset: [i64; 2]) -> Result<CoverageRect, CommandError> {
    let x = frame
        .x
        .checked_add(offset[0])
        .ok_or(DocumentError::ResourceLimit)?;
    let y = frame
        .y
        .checked_add(offset[1])
        .ok_or(DocumentError::ResourceLimit)?;
    let result = CoverageRect { x, y, ..frame };
    result.validate().map_err(DocumentError::from)?;
    Ok(result)
}
fn pixel(pixels: &PixelBuffer, i: usize) -> [f64; 4] {
    match pixels {
        PixelBuffer::Rgba8(v) => std::array::from_fn(|c| f64::from(v[i * 4 + c]) / 255.0),
        PixelBuffer::Rgba16(v) => std::array::from_fn(|c| f64::from(v[i * 4 + c]) / 65535.0),
    }
}
fn set_alpha(pixels: &mut PixelBuffer, index: usize, alpha: f64) {
    match pixels {
        PixelBuffer::Rgba8(v) => v[index * 4 + 3] = (alpha.clamp(0.0, 1.0) * 255.0).round() as u8,
        PixelBuffer::Rgba16(v) => {
            v[index * 4 + 3] = (alpha.clamp(0.0, 1.0) * 65535.0).round() as u16
        }
    }
}
fn copy_color(target: &mut PixelBuffer, source: &PixelBuffer, index: usize) {
    match (target, source) {
        (PixelBuffer::Rgba8(a), PixelBuffer::Rgba8(b)) => {
            a[index * 4..index * 4 + 3].copy_from_slice(&b[index * 4..index * 4 + 3])
        }
        (PixelBuffer::Rgba16(a), PixelBuffer::Rgba16(b)) => {
            a[index * 4..index * 4 + 3].copy_from_slice(&b[index * 4..index * 4 + 3])
        }
        _ => unreachable!("matching working depth"),
    }
}

fn tip(dab: &Dab, x: i64, y: i64, hardness: f64) -> f64 {
    let inner = dab.radius * hardness;
    let field = |distance: f64| {
        if distance >= dab.radius {
            0.0
        } else if distance <= inner || hardness == 1.0 {
            1.0
        } else {
            let t = ((dab.radius - distance) / (dab.radius - inner)).clamp(0.0, 1.0);
            t * t * (3.0 - 2.0 * t)
        }
    };
    let distance = (x as f64 + 0.5 - dab.x).hypot(y as f64 + 0.5 - dab.y);
    let half_diagonal = std::f64::consts::FRAC_1_SQRT_2;
    if distance + half_diagonal <= inner {
        return 1.0;
    }
    if distance - half_diagonal >= dab.radius {
        return 0.0;
    }
    if distance + half_diagonal <= dab.radius && dab.radius >= 1.0 {
        return field(distance);
    }
    let mut coverage = 0.0;
    for sy in 0..4 {
        for sx in 0..4 {
            coverage += field(
                (x as f64 + (f64::from(sx) + 0.5) / 4.0 - dab.x)
                    .hypot(y as f64 + (f64::from(sy) + 0.5) / 4.0 - dab.y),
            );
        }
    }
    coverage / 16.0
}
pub(super) fn mutate_cancellable(
    doc: &mut Document,
    id: &str,
    params: Value,
    cancel: &dyn Fn() -> bool,
) -> Result<Option<Value>, CommandError> {
    if !matches!(id, "paint.stroke" | "paint.erase") {
        return Ok(None);
    }
    let p: BrushStroke = parse(params)?;
    let capacity = validate(&p)?;
    check(cancel)?;
    let target = doc.resolve(&p.layer)?;
    let index = doc.layers().iter().position(|l| l.id() == target).unwrap();
    let info = doc.layers[index].info.clone();
    let erase = id == "paint.erase";
    if doc.layers.iter().any(|l| {
        (l.id() == target || doc.is_descendant(target, l.id()))
            && (l.info.locks.all || l.info.locks.pixels)
    }) {
        return Err(CommandError::Invalid(
            "brush target or ancestor locks pixels".into(),
        ));
    }
    if matches!(p.target, BrushTarget::Pixels)
        && (!matches!(info.kind, LayerKind::Pixel) || erase && info.locks.transparency)
    {
        return Err(CommandError::Invalid("pixel brush needs a pixel layer; unlock transparency before erasing, or select its mask".into()));
    }
    let anchor = match p.target {
        BrushTarget::Pixels => [0, 0],
        BrushTarget::Mask => {
            let mask = info.mask.as_ref().ok_or_else(|| {
                CommandError::Invalid("add a layer mask before painting its coverage".into())
            })?;
            [mask.anchor_x, mask.anchor_y]
        }
    };
    let offset = [
        anchor[0]
            .checked_sub(info.x)
            .ok_or(DocumentError::ResourceLimit)?,
        anchor[1]
            .checked_sub(info.y)
            .ok_or(DocumentError::ResourceLimit)?,
    ];
    let f = doc.frame();
    let frame = plane_bounds(
        CoverageRect {
            x: f.x,
            y: f.y,
            width: f.width,
            height: f.height,
        },
        offset,
    )?;
    let bounds = match p.target {
        BrushTarget::Pixels => Some(frame),
        BrushTarget::Mask => intersection(
            frame,
            doc.layers[index]
                .mask
                .as_ref()
                .ok_or(DocumentError::LayerNotFound)?
                .layout()
                .bounds,
        ),
    };
    let result = |tiles| json!({"layer_id":target,"target":if matches!(p.target,BrushTarget::Mask){"mask"}else{"pixels"},"touched_tiles":tiles});
    let Some(bounds) = bounds else {
        return Ok(Some(result(0)));
    };
    if p.opacity == 0.0
        || p.flow == 0.0
        || !erase && matches!(p.target, BrushTarget::Pixels) && p.color[3] == 0.0
    {
        return Ok(Some(result(0)));
    }
    let _points_budget = doc
        .memory_pool()
        .reserve(MemoryKind::Transient, capacity as u64 * 32 + 1024)
        .map_err(DocumentError::from)?;
    let mut dabs = dabs(&p, capacity, cancel)?;
    for dab in &mut dabs {
        dab.x += offset[0] as f64;
        dab.y += offset[1] as f64;
    }
    dabs.retain(|d| d.pressure > 0.0 || !(p.pressure_size || p.pressure_opacity));
    let mut work = 0.0;
    let mut extent: Option<(f64, f64, f64, f64)> = None;
    let right = bounds.x as f64 + f64::from(bounds.width);
    let bottom = bounds.y as f64 + f64::from(bounds.height);
    for d in &dabs {
        let left = (d.x - d.radius - 0.5).max(bounds.x as f64);
        let top = (d.y - d.radius - 0.5).max(bounds.y as f64);
        let r = (d.x + d.radius + 0.5).min(right);
        let b = (d.y + d.radius + 0.5).min(bottom);
        if r <= left || b <= top {
            continue;
        }
        work += (r - left).ceil() * (b - top).ceil();
        extent = Some(extent.map_or((left, top, r, b), |(a, c, e, f)| {
            (a.min(left), c.min(top), e.max(r), f.max(b))
        }));
    }
    if work > 250_000_000.0 {
        return Err(CommandError::Invalid("brush stroke exceeds 250 million bounded dab-pixel visits; shorten the stroke or increase spacing".into()));
    }
    let Some((left, top, right, bottom)) = extent else {
        return Ok(Some(result(0)));
    };
    let (first, _) = TileCoord::locate(left.floor() as i64, top.floor() as i64);
    let (last, _) = TileCoord::locate(right.ceil() as i64 - 1, bottom.ceil() as i64 - 1);
    let grid = (i128::from(last.x()) - i128::from(first.x()) + 1)
        * (i128::from(last.y()) - i128::from(first.y()) + 1);
    if grid * dabs.len() as i128 > 250_000_000 {
        return Err(CommandError::Invalid("brush stroke exceeds 250 million bounded tile-dab tests; shorten the stroke or increase spacing".into()));
    }
    let color = strata_color::working_to_linear_p3(&[p.color.map(|v| v as f32)], doc.working_icc())
        .map_err(DocumentError::from)?[0]
        .map(f64::from);
    let mut touched = 0;
    for tx in first.x()..=last.x() {
        for ty in first.y()..=last.y() {
            check(cancel)?;
            let coord = TileCoord::new(tx, ty).map_err(DocumentError::from)?;
            let [ox, oy] = coord.origin();
            let _scratch = doc
                .memory_pool()
                .reserve(MemoryKind::Transient, 256 * 256 * 80)
                .map_err(DocumentError::from)?;
            let mut coverage = vec![0u16; 256 * 256];
            let mut cap = vec![0u16; 256 * 256];
            for (n, dab) in dabs.iter().enumerate() {
                if n.is_multiple_of(128) {
                    check(cancel)?;
                }
                let left = (dab.x - dab.radius - 0.5)
                    .floor()
                    .max(ox.max(bounds.x) as f64) as i64;
                let right = (dab.x + dab.radius + 0.5)
                    .ceil()
                    .min((ox + 256).min(bounds.x + i64::from(bounds.width)) as f64)
                    as i64;
                let top = (dab.y - dab.radius - 0.5)
                    .floor()
                    .max(oy.max(bounds.y) as f64) as i64;
                let bottom = (dab.y + dab.radius + 0.5)
                    .ceil()
                    .min((oy + 256).min(bounds.y + i64::from(bounds.height)) as f64)
                    as i64;
                if left >= right || top >= bottom {
                    continue;
                }
                for y in top..bottom {
                    check(cancel)?;
                    for x in left..right {
                        let amount = p.flow * tip(dab, x, y, p.hardness);
                        if amount == 0.0 {
                            continue;
                        }
                        let i = (y - oy) as usize * 256 + (x - ox) as usize;
                        coverage[i] = (f64::from(coverage[i])
                            + (65535.0 - f64::from(coverage[i])) * amount)
                            .round() as u16;
                        cap[i] = cap[i].max(if p.pressure_opacity {
                            (dab.pressure * 65535.0).round() as u16
                        } else {
                            65535
                        });
                    }
                }
            }
            let canvas_x = ox
                .checked_sub(offset[0])
                .ok_or(DocumentError::ResourceLimit)?;
            let canvas_y = oy
                .checked_sub(offset[1])
                .ok_or(DocumentError::ResourceLimit)?;
            let selected = doc
                .selection()
                .map(|mask| mask.region_tile(canvas_x, canvas_y))
                .transpose()
                .map_err(DocumentError::from)?;
            let weight = (0..coverage.len())
                .map(|i| {
                    f64::from(coverage[i].min(cap[i])) / 65535.0
                        * p.opacity
                        * f64::from(selected.as_ref().map_or(65535, |m| m[i]))
                        / 65535.0
                })
                .collect::<Vec<_>>();
            if weight.iter().all(|v| *v == 0.0) {
                continue;
            }
            match p.target {
                BrushTarget::Mask => {
                    let mask = doc.layers[index].mask.as_mut().expect("validated mask");
                    let mut values = mask.region_tile(ox, oy).map_err(DocumentError::from)?;
                    for (i, v) in values.iter_mut().enumerate() {
                        let to = p.value * 65535.0;
                        *v = (f64::from(*v) + (to - f64::from(*v)) * weight[i])
                            .round()
                            .clamp(0.0, 65535.0) as u16;
                    }
                    mask.insert_tile(coord, values)
                        .map_err(DocumentError::from)?;
                }
                BrushTarget::Pixels => {
                    let mut source = doc.layers[index]
                        .pixels
                        .tile(coord)
                        .map_err(DocumentError::from)?
                        .map(|v| (*v).clone())
                        .unwrap_or_else(|| match doc.format {
                            PixelFormat::Rgba8 => PixelBuffer::Rgba8(vec![0; 256 * 256 * 4]),
                            PixelFormat::Rgba16 => PixelBuffer::Rgba16(vec![0; 256 * 256 * 4]),
                        });
                    if erase {
                        for (i, w) in weight.iter().enumerate() {
                            if *w > 0.0 {
                                let alpha = pixel(&source, i)[3] * (1.0 - w);
                                set_alpha(&mut source, i, alpha);
                            }
                        }
                    } else {
                        let mut linear =
                            strata_color::to_linear_p3(&source, 256, 256, doc.working_icc())
                                .map_err(DocumentError::from)?;
                        let mut changed = vec![false; weight.len()];
                        let mut alphas = vec![0.0; weight.len()];
                        for (i, w) in weight.iter().enumerate() {
                            if *w == 0.0 {
                                continue;
                            }
                            let old = pixel(&source, i);
                            if info.locks.transparency && old[3] == 0.0 {
                                continue;
                            }
                            let a = color[3] * w;
                            let alpha = if info.locks.transparency {
                                old[3]
                            } else {
                                a + old[3] * (1.0 - a)
                            };
                            for (c, channel) in color.iter().enumerate().take(3) {
                                linear[i][c] = if info.locks.transparency {
                                    (f64::from(linear[i][c]) * (1.0 - a) + channel * a) as f32
                                } else {
                                    ((f64::from(linear[i][c]) * old[3] * (1.0 - a) + channel * a)
                                        / alpha) as f32
                                };
                            }
                            linear[i][3] = alpha as f32;
                            alphas[i] = alpha;
                            changed[i] = true;
                        }
                        let converted =
                            strata_color::from_linear_p3(&linear, doc.format, doc.working_icc())
                                .map_err(DocumentError::from)?;
                        for (i, changed) in changed.into_iter().enumerate() {
                            if changed {
                                copy_color(&mut source, &converted, i);
                                set_alpha(&mut source, i, alphas[i]);
                            }
                        }
                    }
                    doc.layers[index]
                        .pixels
                        .insert_tile(coord, source)
                        .map_err(DocumentError::from)?;
                }
            }
            touched += 1;
        }
    }
    Ok(Some(result(touched)))
}
