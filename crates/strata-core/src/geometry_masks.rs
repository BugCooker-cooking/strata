//! u16 mask resampling in canvas coordinates, independent of RGB/ICC storage.
use crate::{
    DocumentError,
    geometry::Homography,
    geometry_pixels::{Sampling, transformed_bounds},
};
use std::collections::VecDeque;
use strata_pixels::{
    MaskLayout, TileCoord, TiledMask,
    memory::{MemoryKind, MemoryPool},
};
/// Cancel-aware sparse coverage bounds. Nonzero implicit coverage conservatively
/// occupies the finite layout; a zero baseline needs only stored tile scans.
pub fn coverage_bounds(
    source: &TiledMask,
    cancel: &dyn Fn() -> bool,
) -> Result<Option<strata_pixels::CoverageRect>, DocumentError> {
    if cancel() {
        return Err(DocumentError::Cancelled);
    }
    let layout = source.layout();
    let baseline = layout.default_coverage ^ if layout.inverted { 65535 } else { 0 };
    if baseline != 0 {
        return Ok(Some(layout.bounds));
    }
    let mut extent: Option<[i64; 4]> = None;
    let mut visits = 0u64;
    for (coord, tile) in source.tiles() {
        if cancel() {
            return Err(DocumentError::Cancelled);
        }
        visits += 256 * 256;
        if visits > 64_000_000 {
            return Err(DocumentError::ResourceLimit);
        }
        let values = tile?;
        let [ox, oy] = coord.origin();
        for row in 0..256 {
            if cancel() {
                return Err(DocumentError::Cancelled);
            }
            for col in 0..256 {
                let x = ox + col as i64;
                let y = oy + row as i64;
                if values[row * 256 + col] == 0 || !layout.bounds.contains(x, y) {
                    continue;
                }
                extent = Some(extent.map_or([x, y, x + 1, y + 1], |[l, t, r, b]| {
                    [l.min(x), t.min(y), r.max(x + 1), b.max(y + 1)]
                }));
            }
        }
    }
    extent
        .map(|[x, y, r, b]| {
            Ok(strata_pixels::CoverageRect {
                x,
                y,
                width: u32::try_from(i128::from(r) - i128::from(x))
                    .map_err(|_| DocumentError::ResourceLimit)?,
                height: u32::try_from(i128::from(b) - i128::from(y))
                    .map_err(|_| DocumentError::ResourceLimit)?,
            })
        })
        .transpose()
}
/// Rebind exact u16 selection coverage to a new artboard. Expansion is zero,
/// including when the old finite plane had an implicit white/inverted default.
pub fn rebind(
    source: &TiledMask,
    bounds: strata_pixels::CoverageRect,
    pool: MemoryPool,
    cancel: &dyn Fn() -> bool,
) -> Result<TiledMask, DocumentError> {
    if cancel() {
        return Err(DocumentError::Cancelled);
    }
    bounds.validate()?;
    if source.layout().bounds == bounds {
        return Ok(source.clone());
    }
    let mut output = TiledMask::new(
        MaskLayout {
            bounds,
            default_coverage: 0,
            inverted: false,
        },
        (crate::MAX_DOCUMENT_BYTES / (256 * 256 * 2)) as usize,
        pool.clone(),
    )?;
    let old = source.layout().bounds;
    let x = bounds.x.max(old.x);
    let y = bounds.y.max(old.y);
    let right = (bounds.x + i64::from(bounds.width)).min(old.x + i64::from(old.width));
    let bottom = (bounds.y + i64::from(bounds.height)).min(old.y + i64::from(old.height));
    if x >= right || y >= bottom {
        return Ok(output);
    }
    if i128::from(right - x) * i128::from(bottom - y) > 64_000_000 {
        return Err(DocumentError::ResourceLimit);
    }
    let (first, _) = TileCoord::locate(x, y);
    let (last, _) = TileCoord::locate(right - 1, bottom - 1);
    for ty in first.y()..=last.y() {
        for tx in first.x()..=last.x() {
            if cancel() {
                return Err(DocumentError::Cancelled);
            }
            let _scratch = pool.reserve(MemoryKind::Transient, 256 * 256 * 2)?;
            let coord = TileCoord::new(tx, ty)?;
            let [ox, oy] = coord.origin();
            let mut values = source.region_tile(ox, oy)?;
            for row in 0..256 {
                if cancel() {
                    return Err(DocumentError::Cancelled);
                }
                for col in 0..256 {
                    if !bounds.contains(ox + col as i64, oy + row as i64) {
                        values[row * 256 + col] = 0;
                    }
                }
            }
            output.insert_tile(coord, values)?;
        }
    }
    Ok(output)
}
struct MaskSampler<'a> {
    source: &'a TiledMask,
    tiles: VecDeque<(TileCoord, Vec<u16>)>,
    loads: usize,
}
impl MaskSampler<'_> {
    fn sample(&mut self, x: i64, y: i64, cancel: &dyn Fn() -> bool) -> Result<u16, DocumentError> {
        if !self.source.layout().bounds.contains(x, y) {
            return Ok(0);
        }
        let (coord, [px, py]) = TileCoord::locate(x, y);
        if let Some(i) = self.tiles.iter().position(|(c, _)| *c == coord) {
            let value = self.tiles.remove(i).unwrap();
            self.tiles.push_back(value);
        } else {
            if cancel() {
                return Err(DocumentError::Cancelled);
            }
            if self.loads >= 4096 {
                return Err(DocumentError::Invalid(
                    "transform mask exceeds 4096 decoded source tiles".into(),
                ));
            }
            self.loads += 1;
            if self.tiles.len() == 8 {
                self.tiles.pop_front();
            }
            let [ox, oy] = coord.origin();
            self.tiles
                .push_back((coord, self.source.region_tile(ox, oy)?));
        }
        Ok(self.tiles.back().unwrap().1[usize::from(py) * 256 + usize::from(px)])
    }
}
pub fn transform_mask(
    source: &TiledMask,
    offset: [i64; 2],
    transform: &Homography,
    sampling: Sampling,
    pool: MemoryPool,
    cancel: &dyn Fn() -> bool,
) -> Result<TiledMask, DocumentError> {
    if cancel() {
        return Err(DocumentError::Cancelled);
    }
    let mut bounds = source.layout().bounds;
    bounds.x = bounds
        .x
        .checked_add(offset[0])
        .ok_or(DocumentError::ResourceLimit)?;
    bounds.y = bounds
        .y
        .checked_add(offset[1])
        .ok_or(DocumentError::ResourceLimit)?;
    let destination = transformed_bounds(bounds, transform, sampling)?;
    let _cache = pool.reserve(MemoryKind::Transient, 8 * 256 * 256 * 2)?;
    let mut input = MaskSampler {
        source,
        tiles: VecDeque::new(),
        loads: 0,
    };
    let mut result = TiledMask::new(
        MaskLayout {
            bounds: destination,
            default_coverage: 0,
            inverted: false,
        },
        (crate::MAX_DOCUMENT_BYTES / (256 * 256 * 2)) as usize,
        pool.clone(),
    )?;
    let (first, _) = TileCoord::locate(destination.x, destination.y);
    let (last, _) = TileCoord::locate(
        destination.x + i64::from(destination.width) - 1,
        destination.y + i64::from(destination.height) - 1,
    );
    for ty in first.y()..=last.y() {
        for tx in first.x()..=last.x() {
            if cancel() {
                return Err(DocumentError::Cancelled);
            }
            let _scratch = pool.reserve(MemoryKind::Transient, 256 * 256 * 2)?;
            let coordinate = TileCoord::new(tx, ty)?;
            let [ox, oy] = coordinate.origin();
            let mut values = vec![0u16; 256 * 256];
            for row in 0..256 {
                if cancel() {
                    return Err(DocumentError::Cancelled);
                }
                for col in 0..256 {
                    let x = ox + col as i64;
                    let y = oy + row as i64;
                    if !destination.contains(x, y) {
                        continue;
                    }
                    if let Some([sx, sy]) = transform.inverse_lattice_pixel([x, y]) {
                        let sx = sx
                            .checked_sub(offset[0])
                            .ok_or(DocumentError::ResourceLimit)?;
                        let sy = sy
                            .checked_sub(offset[1])
                            .ok_or(DocumentError::ResourceLimit)?;
                        values[row * 256 + col] = input.sample(sx, sy, cancel)?;
                        continue;
                    }
                    let Ok([sx, sy]) = transform.unmap([x as f64 + 0.5, y as f64 + 0.5]) else {
                        continue;
                    };
                    let sx = sx - offset[0] as f64;
                    let sy = sy - offset[1] as f64;
                    let coverage = if sampling == Sampling::Nearest {
                        f64::from(input.sample(sx.floor() as i64, sy.floor() as i64, cancel)?)
                    } else {
                        let x = sx - 0.5;
                        let y = sy - 0.5;
                        let x0 = x.floor() as i64;
                        let y0 = y.floor() as i64;
                        let fx = x - x0 as f64;
                        let fy = y - y0 as f64;
                        let mut value = 0.0;
                        for (x, wx) in [(x0, 1.0 - fx), (x0 + 1, fx)] {
                            for (y, wy) in [(y0, 1.0 - fy), (y0 + 1, fy)] {
                                if wx * wy > 0.0 {
                                    value += f64::from(input.sample(x, y, cancel)?) * wx * wy;
                                }
                            }
                        }
                        value
                    };
                    values[row * 256 + col] = coverage.round().clamp(0.0, 65535.0) as u16;
                }
            }
            result.insert_tile(coordinate, values)?;
        }
    }
    if cancel() {
        return Err(DocumentError::Cancelled);
    }
    Ok(result)
}
