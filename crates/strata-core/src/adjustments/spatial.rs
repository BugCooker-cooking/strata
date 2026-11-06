use super::TonalAdjustment;
use crate::{Document, DocumentError};
use strata_pixels::{TileCoord, memory::MemoryKind};

fn check(cancel: &dyn Fn() -> bool) -> Result<(), DocumentError> {
    if cancel() {
        Err(DocumentError::Cancelled)
    } else {
        Ok(())
    }
}
fn kernels(radius: f64) -> [usize; 3] {
    let sigma = radius / 3.0;
    let ideal = (4.0 * sigma * sigma + 1.0).sqrt();
    let mut lower = ideal.floor() as usize;
    if lower.is_multiple_of(2) {
        lower = lower.saturating_sub(1);
    }
    lower = lower.max(1);
    let upper = lower + 2;
    let n = ((12.0 * sigma * sigma - 3.0 * (lower * lower) as f64 - 12.0 * lower as f64 - 9.0)
        / (-4.0 * lower as f64 - 4.0))
        .round()
        .clamp(0.0, 3.0) as usize;
    std::array::from_fn(|i| if i < n { lower / 2 } else { upper / 2 })
}
fn blur(
    values: &mut [f64],
    side: usize,
    radius: usize,
    temporary: &mut [f64],
    prefix: &mut [f64],
    cancel: &dyn Fn() -> bool,
) -> Result<(), DocumentError> {
    if radius == 0 {
        return Ok(());
    }
    let scale = 1.0 / (radius * 2 + 1) as f64;
    for y in 0..side {
        check(cancel)?;
        prefix[0] = 0.0;
        for x in 0..side {
            prefix[x + 1] = prefix[x] + values[y * side + x];
        }
        for x in 0..side {
            temporary[y * side + x] =
                (prefix[(x + radius + 1).min(side)] - prefix[x.saturating_sub(radius)]) * scale;
        }
    }
    for x in 0..side {
        check(cancel)?;
        prefix[0] = 0.0;
        for y in 0..side {
            prefix[y + 1] = prefix[y] + temporary[y * side + x];
        }
        for y in 0..side {
            values[y * side + x] =
                (prefix[(y + radius + 1).min(side)] - prefix[y.saturating_sub(radius)]) * scale;
        }
    }
    Ok(())
}
/// Alpha-normalized local luminance. Only one neighboring prefix tile is live
/// at a time; the finite box-kernel support determines the exact cross-tile halo.
pub(crate) fn render(
    doc: &Document,
    coord: TileCoord,
    input: &[[f64; 4]],
    settings: &TonalAdjustment,
    reference: Option<strata_pixels::CoverageRect>,
    mut tile: impl FnMut(TileCoord) -> Result<Vec<[f64; 4]>, DocumentError>,
    cancel: &dyn Fn() -> bool,
) -> Result<Vec<[f32; 4]>, DocumentError> {
    check(cancel)?;
    let TonalAdjustment::ShadowsHighlights {
        shadows,
        highlights,
        shadows_width,
        highlights_width,
        radius,
    } = settings
    else {
        return Err(DocumentError::Invalid(
            "not a spatial tone adjustment".into(),
        ));
    };
    if *shadows == 0.0 && *highlights == 0.0 {
        return Ok(input.iter().map(|p| p.map(|v| v as f32)).collect());
    }
    let radii = kernels(*radius);
    let halo = radii.iter().sum::<usize>();
    let side = 256 + halo * 2;
    let count = side * side;
    let _budget = doc.memory_pool().reserve(
        MemoryKind::Transient,
        count as u64 * 32 + 256 * 256 * 96 + (side as u64 + 1) * 24,
    )?;
    let [ox, oy] = coord.origin();
    let left = ox
        .checked_sub(halo as i64)
        .ok_or_else(|| DocumentError::Invalid("local tone bounds overflow".into()))?;
    let top = oy
        .checked_sub(halo as i64)
        .ok_or_else(|| DocumentError::Invalid("local tone bounds overflow".into()))?;
    let f = reference.unwrap_or_else(|| {
        let f = doc.frame();
        strata_pixels::CoverageRect {
            x: f.x,
            y: f.y,
            width: f.width,
            height: f.height,
        }
    });
    let right = f.x + i64::from(f.width) - 1;
    let bottom = f.y + i64::from(f.height) - 1;
    let xs = (0..side)
        .map(|x| (left + x as i64).clamp(f.x, right))
        .collect::<Vec<_>>();
    let ys = (0..side)
        .map(|y| (top + y as i64).clamp(f.y, bottom))
        .collect::<Vec<_>>();
    let mut luminance = vec![0.0; count];
    let mut alpha = vec![0.0; count];
    let mut x0 = 0;
    while x0 < side {
        let tx = xs[x0].div_euclid(256);
        let x1 = xs.partition_point(|x| x.div_euclid(256) <= tx);
        let mut y0 = 0;
        while y0 < side {
            check(cancel)?;
            let ty = ys[y0].div_euclid(256);
            let y1 = ys.partition_point(|y| y.div_euclid(256) <= ty);
            let c = TileCoord::new(tx, ty)?;
            let [cx, cy] = c.origin();
            let values = tile(c)?;
            for y in y0..y1 {
                check(cancel)?;
                for x in x0..x1 {
                    let p = values[(ys[y] - cy) as usize * 256 + (xs[x] - cx) as usize];
                    let a = p[3].clamp(0.0, 1.0);
                    let index = y * side + x;
                    alpha[index] = a;
                    luminance[index] = (p[0] * 0.2289745640697488
                        + p[1] * 0.6917385218365064
                        + p[2] * 0.0792869140937450)
                        .max(0.0)
                        * a;
                }
            }
            y0 = y1;
        }
        x0 = x1;
    }
    let mut temporary = vec![0.0; count];
    let mut prefix = vec![0.0; side + 1];
    for radius in radii {
        blur(
            &mut luminance,
            side,
            radius,
            &mut temporary,
            &mut prefix,
            cancel,
        )?;
        blur(
            &mut alpha,
            side,
            radius,
            &mut temporary,
            &mut prefix,
            cancel,
        )?;
    }
    let mut out = Vec::with_capacity(256 * 256);
    for y in 0..256 {
        check(cancel)?;
        for x in 0..256 {
            let i = (y + halo) * side + x + halo;
            let p = input[y * 256 + x];
            let base = if alpha[i] > 1e-12 {
                luminance[i] / alpha[i]
            } else {
                0.0
            };
            let encoded = if base <= 0.0031308 {
                base * 12.92
            } else {
                1.055 * base.powf(1.0 / 2.4) - 0.055
            };
            let dark = ((shadows_width - encoded) / shadows_width).clamp(0.0, 1.0);
            let light = ((encoded - (1.0 - highlights_width)) / highlights_width).clamp(0.0, 1.0);
            let gain = 2.0f64.powf(2.0 * (shadows / 100.0 * dark - highlights / 100.0 * light));
            out.push([
                (p[0] * gain) as f32,
                (p[1] * gain) as f32,
                (p[2] * gain) as f32,
                p[3] as f32,
            ]);
        }
    }
    Ok(out)
}
