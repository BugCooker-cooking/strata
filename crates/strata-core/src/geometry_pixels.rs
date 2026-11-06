//! Tiled projective sampling, shared by geometry commands. The caller chooses
//! source bounds and owns publication; no artboard clipping occurs here.
use crate::{DocumentError, geometry::Homography};
use std::collections::VecDeque;
use strata_pixels::{
    CoverageRect, PixelBuffer, PixelFormat, TileCoord, TiledPixels,
    memory::{MemoryKind, MemoryPool},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sampling {
    Nearest,
    Bilinear,
}
pub struct PixelSource<'a> {
    pub pixels: &'a TiledPixels,
    pub offset: [i64; 2],
    pub bounds: CoverageRect,
    pub icc: &'a [u8],
}
fn check(cancel: &dyn Fn() -> bool) -> Result<(), DocumentError> {
    if cancel() {
        Err(DocumentError::Cancelled)
    } else {
        Ok(())
    }
}
fn invalid(e: impl std::fmt::Display) -> DocumentError {
    DocumentError::Invalid(e.to_string())
}
fn raw(p: &PixelBuffer, index: usize) -> [u16; 4] {
    match p {
        PixelBuffer::Rgba8(v) => std::array::from_fn(|c| u16::from(v[index * 4 + c])),
        PixelBuffer::Rgba16(v) => std::array::from_fn(|c| v[index * 4 + c]),
    }
}
fn set(p: &mut PixelBuffer, index: usize, value: [u16; 4]) {
    match p {
        PixelBuffer::Rgba8(v) => {
            for c in 0..4 {
                v[index * 4 + c] = value[c] as u8;
            }
        }
        PixelBuffer::Rgba16(v) => v[index * 4..index * 4 + 4].copy_from_slice(&value),
    }
}
pub fn content_bounds(
    source: &TiledPixels,
    offset: [i64; 2],
    cancel: &dyn Fn() -> bool,
) -> Result<Option<CoverageRect>, DocumentError> {
    let mut extent: Option<[i64; 4]> = None;
    for (coord, tile) in source.tiles() {
        check(cancel)?;
        let tile = tile?;
        let [ox, oy] = coord.origin();
        for y in 0..256 {
            check(cancel)?;
            for x in 0..256 {
                if raw(&tile, y * 256 + x).iter().all(|v| *v == 0) {
                    continue;
                }
                let gx = ox
                    .checked_add(offset[0])
                    .and_then(|v| v.checked_add(x as i64))
                    .ok_or(DocumentError::ResourceLimit)?;
                let gy = oy
                    .checked_add(offset[1])
                    .and_then(|v| v.checked_add(y as i64))
                    .ok_or(DocumentError::ResourceLimit)?;
                let r = gx.checked_add(1).ok_or(DocumentError::ResourceLimit)?;
                let b = gy.checked_add(1).ok_or(DocumentError::ResourceLimit)?;
                extent = Some(extent.map_or([gx, gy, r, b], |[l, t, r0, b0]| {
                    [gx.min(l), gy.min(t), r.max(r0), b.max(b0)]
                }));
            }
        }
    }
    extent
        .map(|[x, y, r, b]| {
            Ok(CoverageRect {
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
pub fn transformed_bounds(
    bounds: CoverageRect,
    transform: &Homography,
    sampling: Sampling,
) -> Result<CoverageRect, DocumentError> {
    bounds.validate()?;
    let pad = if sampling == Sampling::Bilinear && !transform.preserves_lattice() {
        0.5
    } else {
        0.0
    };
    let x = bounds.x as f64 - pad;
    let y = bounds.y as f64 - pad;
    let r = bounds.x as f64 + f64::from(bounds.width) + pad;
    let b = bounds.y as f64 + f64::from(bounds.height) + pad;
    let quad = transform
        .quad([[x, y], [r, y], [r, b], [x, b]])
        .map_err(invalid)?;
    let min = |c| {
        quad.iter()
            .map(|p| p[c])
            .fold(f64::INFINITY, f64::min)
            .floor() as i64
    };
    let max = |c| {
        quad.iter()
            .map(|p| p[c])
            .fold(f64::NEG_INFINITY, f64::max)
            .ceil() as i64
    };
    let x = min(0);
    let y = min(1);
    let width = u32::try_from(i128::from(max(0)) - i128::from(x))
        .map_err(|_| DocumentError::ResourceLimit)?;
    let height = u32::try_from(i128::from(max(1)) - i128::from(y))
        .map_err(|_| DocumentError::ResourceLimit)?;
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > 64_000_000 {
        return Err(invalid(
            "transform output exceeds 64 million bounded pixel visits or collapses to zero",
        ));
    }
    Ok(CoverageRect {
        x,
        y,
        width,
        height,
    })
}
struct SourceTile {
    coord: TileCoord,
    raw: PixelBuffer,
    linear: Option<Vec<[f32; 4]>>,
}
struct Sampler<'a> {
    source: &'a TiledPixels,
    offset: [i64; 2],
    bounds: CoverageRect,
    icc: &'a [u8],
    tiles: VecDeque<SourceTile>,
    loads: usize,
}
impl Sampler<'_> {
    fn tile(
        &mut self,
        x: i64,
        y: i64,
        linear: bool,
        cancel: &dyn Fn() -> bool,
    ) -> Result<Option<(usize, usize)>, DocumentError> {
        if !self.bounds.contains(x, y) {
            return Ok(None);
        }
        let x = x
            .checked_sub(self.offset[0])
            .ok_or(DocumentError::ResourceLimit)?;
        let y = y
            .checked_sub(self.offset[1])
            .ok_or(DocumentError::ResourceLimit)?;
        let (coord, [px, py]) = TileCoord::locate(x, y);
        if let Some(i) = self.tiles.iter().position(|t| t.coord == coord) {
            let value = self.tiles.remove(i).unwrap();
            self.tiles.push_back(value);
        } else {
            check(cancel)?;
            if self.loads >= 4096 {
                return Err(invalid(
                    "transform exceeds 4096 decoded source tiles; reduce the transform scale or source area",
                ));
            }
            self.loads += 1;
            if self.tiles.len() == 8 {
                self.tiles.pop_front();
            }
            let raw = self
                .source
                .tile(coord)?
                .map(|v| (*v).clone())
                .unwrap_or_else(|| match self.source.format() {
                    PixelFormat::Rgba8 => PixelBuffer::Rgba8(vec![0; 256 * 256 * 4]),
                    PixelFormat::Rgba16 => PixelBuffer::Rgba16(vec![0; 256 * 256 * 4]),
                });
            self.tiles.push_back(SourceTile {
                coord,
                raw,
                linear: None,
            });
        }
        let i = self.tiles.len() - 1;
        if linear && self.tiles[i].linear.is_none() {
            check(cancel)?;
            self.tiles[i].linear = Some(strata_color::to_linear_p3(
                &self.tiles[i].raw,
                256,
                256,
                self.icc,
            )?);
        }
        Ok(Some((i, usize::from(py) * 256 + usize::from(px))))
    }
    fn raw(
        &mut self,
        x: i64,
        y: i64,
        cancel: &dyn Fn() -> bool,
    ) -> Result<[u16; 4], DocumentError> {
        Ok(if let Some((i, p)) = self.tile(x, y, false, cancel)? {
            raw(&self.tiles[i].raw, p)
        } else {
            [0; 4]
        })
    }
    fn linear(
        &mut self,
        x: i64,
        y: i64,
        cancel: &dyn Fn() -> bool,
    ) -> Result<[f64; 4], DocumentError> {
        Ok(if let Some((i, p)) = self.tile(x, y, true, cancel)? {
            self.tiles[i].linear.as_ref().unwrap()[p].map(f64::from)
        } else {
            [0.0; 4]
        })
    }
}
pub fn transform_pixels(
    input: PixelSource<'_>,
    transform: &Homography,
    sampling: Sampling,
    pool: MemoryPool,
    cancel: &dyn Fn() -> bool,
) -> Result<TiledPixels, DocumentError> {
    transform_pixels_at(input, transform, sampling, [0, 0], pool, cancel)
}
/// Store samples relative to output_origin while mapping in canvas coordinates.
pub fn transform_pixels_at(
    input: PixelSource<'_>,
    transform: &Homography,
    sampling: Sampling,
    output_origin: [i64; 2],
    pool: MemoryPool,
    cancel: &dyn Fn() -> bool,
) -> Result<TiledPixels, DocumentError> {
    let PixelSource {
        pixels: source,
        offset,
        bounds,
        icc,
    } = input;
    check(cancel)?;
    let dest = transformed_bounds(bounds, transform, sampling)?;
    let _cache = pool.reserve(MemoryKind::Transient, 8 * 256 * 256 * 24 + 65536)?;
    let mut input = Sampler {
        source,
        offset,
        bounds,
        icc,
        tiles: VecDeque::new(),
        loads: 0,
    };
    let mut output = TiledPixels::with_pool(
        source.format(),
        (crate::MAX_DOCUMENT_BYTES / (256 * 256 * 4 * source.format().bytes_per_sample() as u64))
            as usize,
        pool.clone(),
    );
    let local_x = dest
        .x
        .checked_sub(output_origin[0])
        .ok_or(DocumentError::ResourceLimit)?;
    let local_y = dest
        .y
        .checked_sub(output_origin[1])
        .ok_or(DocumentError::ResourceLimit)?;
    let (first, _) = TileCoord::locate(local_x, local_y);
    let (last, _) = TileCoord::locate(
        local_x
            .checked_add(i64::from(dest.width) - 1)
            .ok_or(DocumentError::ResourceLimit)?,
        local_y
            .checked_add(i64::from(dest.height) - 1)
            .ok_or(DocumentError::ResourceLimit)?,
    );
    for ty in first.y()..=last.y() {
        for tx in first.x()..=last.x() {
            check(cancel)?;
            let coord = TileCoord::new(tx, ty)?;
            let [ox, oy] = coord.origin();
            let ox = ox
                .checked_add(output_origin[0])
                .ok_or(DocumentError::ResourceLimit)?;
            let oy = oy
                .checked_add(output_origin[1])
                .ok_or(DocumentError::ResourceLimit)?;
            let _scratch = pool.reserve(MemoryKind::Transient, 256 * 256 * 64)?;
            let mut pixels = match source.format() {
                PixelFormat::Rgba8 => PixelBuffer::Rgba8(vec![0; 256 * 256 * 4]),
                PixelFormat::Rgba16 => PixelBuffer::Rgba16(vec![0; 256 * 256 * 4]),
            };
            let mut linear = vec![[0.0f32; 4]; 256 * 256];
            let mut interpolated = vec![false; 256 * 256];
            for row in 0..256 {
                check(cancel)?;
                for col in 0..256 {
                    let x = ox + col as i64;
                    let y = oy + row as i64;
                    if !dest.contains(x, y) {
                        continue;
                    }
                    let i = row * 256 + col;
                    if let Some([sx, sy]) = transform.inverse_lattice_pixel([x, y]) {
                        set(&mut pixels, i, input.raw(sx, sy, cancel)?);
                        continue;
                    }
                    let Ok([sx, sy]) = transform.unmap([x as f64 + 0.5, y as f64 + 0.5]) else {
                        continue;
                    };
                    if sampling == Sampling::Nearest {
                        set(
                            &mut pixels,
                            i,
                            input.raw(sx.floor() as i64, sy.floor() as i64, cancel)?,
                        );
                        continue;
                    }
                    let x = sx - 0.5;
                    let y = sy - 0.5;
                    if (x - x.round()).abs() < 1e-7 && (y - y.round()).abs() < 1e-7 {
                        set(
                            &mut pixels,
                            i,
                            input.raw(x.round() as i64, y.round() as i64, cancel)?,
                        );
                        continue;
                    }
                    let x0 = x.floor() as i64;
                    let y0 = y.floor() as i64;
                    let fx = x - x0 as f64;
                    let fy = y - y0 as f64;
                    let mut value = [0.0; 4];
                    for (x, wx) in [(x0, 1.0 - fx), (x0 + 1, fx)] {
                        for (y, wy) in [(y0, 1.0 - fy), (y0 + 1, fy)] {
                            if wx * wy == 0.0 {
                                continue;
                            }
                            let p = input.linear(x, y, cancel)?;
                            for c in 0..3 {
                                value[c] += p[c] * p[3] * wx * wy;
                            }
                            value[3] += p[3] * wx * wy;
                        }
                    }
                    if value[3] > 0.0 {
                        for c in 0..3 {
                            value[c] /= value[3];
                        }
                        linear[i] = value.map(|v| v as f32);
                        interpolated[i] = true;
                    }
                }
            }
            if interpolated.iter().any(|v| *v) {
                let converted = strata_color::from_linear_p3(&linear, source.format(), icc)?;
                for (i, changed) in interpolated.into_iter().enumerate() {
                    if changed {
                        set(&mut pixels, i, raw(&converted, i));
                    }
                }
            }
            output.insert_tile(coord, pixels)?;
        }
    }
    check(cancel)?;
    Ok(output)
}
