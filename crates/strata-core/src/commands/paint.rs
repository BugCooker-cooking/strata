use super::*;
use crate::{Document, DocumentError, LayerKind, composite::Compositor};
use serde_json::json;
use strata_pixels::{PixelBuffer, PixelFormat, TileCoord, TiledMask, memory::MemoryKind};
fn check(cancel: &dyn Fn() -> bool) -> Result<(), CommandError> {
    if cancel() {
        Err(CommandError::Expired)
    } else {
        Ok(())
    }
}
fn raw(buffer: &PixelBuffer, index: usize) -> [f64; 4] {
    match buffer {
        PixelBuffer::Rgba8(v) => std::array::from_fn(|c| f64::from(v[index * 4 + c]) / 255.0),
        PixelBuffer::Rgba16(v) => std::array::from_fn(|c| f64::from(v[index * 4 + c]) / 65535.0),
    }
}
fn unit(v: f64) -> bool {
    v.is_finite() && (0.0..=1.0).contains(&v)
}
fn validate_color(color: [f64; 4], opacity: f64) -> Result<(), CommandError> {
    if color.into_iter().all(unit) && unit(opacity) {
        Ok(())
    } else {
        Err(CommandError::Invalid(
            "fill color and opacity must be finite normalized 0..1".into(),
        ))
    }
}
pub(super) fn sample(doc: &Document, params: Value) -> Result<Value, CommandError> {
    let p: SampleColor = parse(params)?;
    let f = doc.frame();
    if p.radius > 31
        || p.x < f.x
        || p.y < f.y
        || i128::from(p.x) >= i128::from(f.x) + i128::from(f.width)
        || i128::from(p.y) >= i128::from(f.y) + i128::from(f.height)
    {
        return Err(CommandError::Invalid(
            "sample center must be in artboard, radius 0..31 pixels".into(),
        ));
    }
    let source = if matches!(p.sample, SelectionSample::CurrentLayer) {
        Some(
            doc.layers()
                .iter()
                .find(|l| {
                    Some(l.id()) == doc.active_layer()
                        && matches!(l.info().kind, LayerKind::Pixel)
                        && doc.effective_visibility(l.id())
                })
                .ok_or_else(|| {
                    CommandError::Invalid(
                        "current-layer sampling requires a visible active pixel layer".into(),
                    )
                })?,
        )
    } else {
        None
    };
    if p.radius == 0
        && let Some(layer) = source
    {
        let pixel = layer
            .pixels()
            .pixel(
                p.x.checked_sub(layer.info().x)
                    .ok_or(DocumentError::ResourceLimit)?,
                p.y.checked_sub(layer.info().y)
                    .ok_or(DocumentError::ResourceLimit)?,
            )
            .map_err(DocumentError::from)?;
        let color = match pixel {
            strata_pixels::PixelValue::Rgba8(v) => v.map(|v| f64::from(v) / 255.0),
            strata_pixels::PixelValue::Rgba16(v) => v.map(|v| f64::from(v) / 65535.0),
        };
        return Ok(
            json!({"color":color,"bit_depth":doc.bit_depth(),"profile":doc.profile_name(),"sampled_pixels":1}),
        );
    }
    let _budget = doc
        .memory_pool()
        .reserve(MemoryKind::Transient, 6 * 1024 * 1024)
        .map_err(DocumentError::from)?;
    let radius = i64::from(p.radius);
    let left = p.x.saturating_sub(radius).max(f.x);
    let top = p.y.saturating_sub(radius).max(f.y);
    let right = p.x.saturating_add(radius).min(f.x + i64::from(f.width) - 1);
    let bottom =
        p.y.saturating_add(radius)
            .min(f.y + i64::from(f.height) - 1);
    let mut compositor = Compositor::new(1);
    let mut cache = None;
    let mut sum = [0.0f64; 4];
    let mut count = 0;
    for y in top..=bottom {
        for x in left..=right {
            let (sx, sy) = if let Some(layer) = source {
                (
                    x.checked_sub(layer.info().x)
                        .ok_or(DocumentError::ResourceLimit)?,
                    y.checked_sub(layer.info().y)
                        .ok_or(DocumentError::ResourceLimit)?,
                )
            } else {
                (x, y)
            };
            let (coord, [px, py]) = TileCoord::locate(sx, sy);
            if cache.as_ref().is_none_or(|(c, _)| *c != coord) {
                let linear = if let Some(layer) = source {
                    if let Some(tile) = layer.pixels().tile(coord).map_err(DocumentError::from)? {
                        strata_color::to_linear_p3(&tile, 256, 256, doc.working_icc())
                            .map_err(DocumentError::from)?
                    } else {
                        vec![[0.0; 4]; 256 * 256]
                    }
                } else {
                    compositor.tile(doc, coord)?.iter().copied().collect()
                };
                cache = Some((coord, linear));
            }
            let value = cache.as_ref().unwrap().1[usize::from(py) * 256 + usize::from(px)];
            for c in 0..3 {
                sum[c] += f64::from(value[c]) * f64::from(value[3]);
            }
            sum[3] += f64::from(value[3]);
            count += 1;
        }
    }
    let linear = if sum[3] > 0.0 {
        [
            sum[0] / sum[3],
            sum[1] / sum[3],
            sum[2] / sum[3],
            sum[3] / f64::from(count),
        ]
    } else {
        [0.0; 4]
    };
    let values = strata_color::linear_p3_to_working(&[linear.map(|v| v as f32)], doc.working_icc())
        .map_err(DocumentError::from)?[0]
        .map(f64::from);
    Ok(
        json!({"color":values.map(|v|v.clamp(0.0,1.0)),"bit_depth":doc.bit_depth(),"profile":doc.profile_name(),"sampled_pixels":count}),
    )
}
pub(super) fn fill(
    doc: &mut Document,
    layer: LayerRef,
    opacity: f64,
    region: Option<&TiledMask>,
    color: impl Fn(i64, i64) -> [f64; 4],
    cancel: &dyn Fn() -> bool,
) -> Result<Value, CommandError> {
    if !unit(opacity) {
        return Err(CommandError::Invalid("fill opacity must be 0..1".into()));
    }
    let target = doc.resolve(&layer)?;
    let index = doc.layers().iter().position(|l| l.id() == target).unwrap();
    let info = doc.layers()[index].info().clone();
    if !matches!(info.kind, LayerKind::Pixel)
        || doc.layers().iter().any(|l| {
            (l.id() == target || doc.is_descendant(target, l.id()))
                && (l.info().locks.all || l.info().locks.pixels)
        })
    {
        return Err(CommandError::Invalid(
            "fill needs an unlocked pixel layer and unlocked pixel ancestors".into(),
        ));
    }
    let f = doc.frame();
    let (mut x, mut y) = (f.x, f.y);
    let (mut right, mut bottom) = (f.x + i64::from(f.width), f.y + i64::from(f.height));
    // Finite brush/bucket coverage must not turn a small retouch into an
    // artboard-wide scan. Masks remain sampled in canvas coordinates below.
    for mask in [region, doc.selection()].into_iter().flatten() {
        let Some(bounds) = mask.content_bounds().map_err(DocumentError::from)? else {
            return Ok(json!({"layer_id":target,"target":"pixels","touched_tiles":0}));
        };
        x = x.max(bounds.x);
        y = y.max(bounds.y);
        right = right.min(bounds.x + i64::from(bounds.width));
        bottom = bottom.min(bounds.y + i64::from(bounds.height));
    }
    if x >= right || y >= bottom {
        return Ok(json!({"layer_id":target,"target":"pixels","touched_tiles":0}));
    }
    let left = x.checked_sub(info.x).ok_or(DocumentError::ResourceLimit)?;
    let top = y.checked_sub(info.y).ok_or(DocumentError::ResourceLimit)?;
    let right = right
        .checked_sub(info.x)
        .and_then(|v| v.checked_sub(1))
        .ok_or(DocumentError::ResourceLimit)?;
    let bottom = bottom
        .checked_sub(info.y)
        .and_then(|v| v.checked_sub(1))
        .ok_or(DocumentError::ResourceLimit)?;
    let (first, _) = TileCoord::locate(left, top);
    let (last, _) = TileCoord::locate(right, bottom);
    let mut touched = 0;
    for tx in first.x()..=last.x() {
        for ty in first.y()..=last.y() {
            check(cancel)?;
            let coord = TileCoord::new(tx, ty).map_err(DocumentError::from)?;
            let [ox, oy] = coord.origin();
            let gx = ox.checked_add(info.x).ok_or(DocumentError::ResourceLimit)?;
            let gy = oy.checked_add(info.y).ok_or(DocumentError::ResourceLimit)?;
            let _budget = doc
                .memory_pool()
                .reserve(MemoryKind::Transient, 256 * 256 * 100)
                .map_err(DocumentError::from)?;
            let selected = doc
                .selection()
                .map(|m| m.region_tile(gx, gy))
                .transpose()
                .map_err(DocumentError::from)?;
            let region = region
                .map(|m| m.region_tile(gx, gy))
                .transpose()
                .map_err(DocumentError::from)?;
            let mut weights = vec![0.0; 256 * 256];
            let mut encoded = vec![[0.0f32; 4]; 256 * 256];
            for y in 0..256 {
                check(cancel)?;
                for x in 0..256 {
                    let sx = ox + x as i64;
                    let sy = oy + y as i64;
                    if sx < left || sx > right || sy < top || sy > bottom {
                        continue;
                    }
                    let i = y * 256 + x;
                    weights[i] = opacity * f64::from(selected.as_ref().map_or(65535, |m| m[i]))
                        / 65535.0
                        * f64::from(region.as_ref().map_or(65535, |m| m[i]))
                        / 65535.0;
                    if weights[i] > 0.0 {
                        encoded[i] = color(gx + x as i64, gy + y as i64).map(|v| v as f32);
                    }
                }
            }
            if weights.iter().all(|v| *v == 0.0) {
                continue;
            }
            let front = strata_color::working_to_linear_p3(&encoded, doc.working_icc())
                .map_err(DocumentError::from)?;
            let mut source = doc.layers[index]
                .pixels
                .tile(coord)
                .map_err(DocumentError::from)?
                .map(|v| (*v).clone())
                .unwrap_or_else(|| match doc.format {
                    PixelFormat::Rgba8 => PixelBuffer::Rgba8(vec![0; 256 * 256 * 4]),
                    PixelFormat::Rgba16 => PixelBuffer::Rgba16(vec![0; 256 * 256 * 4]),
                });
            let mut back = strata_color::to_linear_p3(&source, 256, 256, doc.working_icc())
                .map_err(DocumentError::from)?;
            let mut changed = vec![false; 256 * 256];
            for (i, w) in weights.into_iter().enumerate() {
                if w == 0.0 {
                    continue;
                }
                let old = raw(&source, i);
                if info.locks.transparency && old[3] == 0.0 {
                    continue;
                }
                let a = f64::from(front[i][3]) * w;
                if a == 0.0 {
                    continue;
                }
                let alpha = if info.locks.transparency {
                    old[3]
                } else {
                    a + old[3] * (1.0 - a)
                };
                for c in 0..3 {
                    back[i][c] = if info.locks.transparency {
                        (f64::from(back[i][c]) * (1.0 - a) + f64::from(front[i][c]) * a) as f32
                    } else {
                        ((f64::from(back[i][c]) * old[3] * (1.0 - a) + f64::from(front[i][c]) * a)
                            / alpha) as f32
                    };
                }
                back[i][3] = alpha as f32;
                changed[i] = true;
            }
            let output = strata_color::from_linear_p3(&back, doc.format, doc.working_icc())
                .map_err(DocumentError::from)?;
            for (i, changed) in changed.into_iter().enumerate() {
                if changed {
                    match (&mut source, &output) {
                        (PixelBuffer::Rgba8(a), PixelBuffer::Rgba8(b)) => {
                            a[i * 4..i * 4 + 4].copy_from_slice(&b[i * 4..i * 4 + 4])
                        }
                        (PixelBuffer::Rgba16(a), PixelBuffer::Rgba16(b)) => {
                            a[i * 4..i * 4 + 4].copy_from_slice(&b[i * 4..i * 4 + 4])
                        }
                        _ => unreachable!(),
                    }
                }
            }
            doc.layers[index]
                .pixels
                .insert_tile(coord, source)
                .map_err(DocumentError::from)?;
            touched += 1;
        }
    }
    Ok(json!({"layer_id":target,"target":"pixels","touched_tiles":touched}))
}
pub(super) fn mutate(
    doc: &mut Document,
    id: &str,
    params: Value,
    cancel: &dyn Fn() -> bool,
) -> Result<Option<Value>, CommandError> {
    match id {
        "paint.fill" => {
            let p: FillPixels = parse(params)?;
            validate_color(p.color, p.opacity)?;
            Ok(Some(fill(
                doc,
                p.layer,
                p.opacity,
                None,
                |_, _| p.color,
                cancel,
            )?))
        }
        "paint.bucket" => {
            let p: BucketPixels = parse(params)?;
            validate_color(p.color, p.opacity)?;
            let mask = super::sample_selection::wand(
                doc,
                &WandSelection {
                    x: p.x,
                    y: p.y,
                    tolerance: p.tolerance,
                    contiguous: p.contiguous,
                    sample: p.sample,
                    mode: SelectionMode::Replace,
                },
                cancel,
            )?;
            Ok(Some(fill(
                doc,
                p.layer,
                p.opacity,
                Some(&mask),
                |_, _| p.color,
                cancel,
            )?))
        }
        "paint.gradient" => {
            let p: GradientPixels = parse(params)?;
            if !(2..=64).contains(&p.stops.len())
                || p.stops.first().unwrap().at != 0.0
                || p.stops.last().unwrap().at != 1.0
                || p.stops.windows(2).any(|s| s[1].at - s[0].at < 1e-6)
                || p.stops
                    .iter()
                    .any(|s| !unit(s.at) || s.color.into_iter().any(|v| !unit(v)))
                || p.start
                    .iter()
                    .chain(&p.end)
                    .any(|v| !v.is_finite() || v.abs() > 1e12)
            {
                return Err(CommandError::Invalid("gradient needs finite bounded endpoints and 2..64 normalized RGBA stops with increasing positions from 0 to 1".into()));
            }
            let dx = p.end[0] - p.start[0];
            let dy = p.end[1] - p.start[1];
            let length = dx.hypot(dy);
            if length < 1e-6 {
                return Err(CommandError::Invalid(
                    "gradient start and end must be distinct".into(),
                ));
            }
            let colors = |x: i64, y: i64| {
                let x = x as f64 + 0.5 - p.start[0];
                let y = y as f64 + 0.5 - p.start[1];
                let along = (x * dx + y * dy) / (length * length);
                let across = (y * dx - x * dy) / (length * length);
                let t = match p.shape {
                    PaintGradientShape::Linear => along,
                    PaintGradientShape::Radial => x.hypot(y) / length,
                    PaintGradientShape::Reflected => along.abs(),
                    PaintGradientShape::Diamond => along.abs() + across.abs(),
                    PaintGradientShape::Angle => {
                        across.atan2(along).rem_euclid(std::f64::consts::TAU)
                            / std::f64::consts::TAU
                    }
                }
                .clamp(0.0, 1.0);
                let t = if p.reverse { 1.0 - t } else { t };
                let i = p
                    .stops
                    .partition_point(|s| s.at <= t)
                    .saturating_sub(1)
                    .min(p.stops.len() - 2);
                let a = &p.stops[i];
                let b = &p.stops[i + 1];
                let t = (t - a.at) / (b.at - a.at);
                let alpha = a.color[3] * (1.0 - t) + b.color[3] * t;
                std::array::from_fn(|c| {
                    if c == 3 {
                        alpha
                    } else if alpha > 0.0 {
                        (a.color[c] * a.color[3] * (1.0 - t) + b.color[c] * b.color[3] * t) / alpha
                    } else {
                        0.0
                    }
                })
            };
            Ok(Some(fill(doc, p.layer, p.opacity, None, colors, cancel)?))
        }
        _ => Ok(None),
    }
}
