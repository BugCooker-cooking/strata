//! Axis-aligned image sizing. Rational coordinates avoid loss of pixel phase at
//! large canvas offsets. Reduction integrates pixel areas; enlargement samples
//! pixel centers. Source caches, output tiles and scratch share the document pool.
use crate::{DocumentError, Frame, commands::ResizeInterpolation, geometry_pixels::PixelSource};
use strata_pixels::{
    CoverageRect, MaskLayout, PixelBuffer, TileCoord, TiledMask, TiledPixels,
    memory::{MemoryKind, MemoryPool},
};

fn check(cancel: &dyn Fn() -> bool) -> Result<(), DocumentError> {
    if cancel() {
        Err(DocumentError::Cancelled)
    } else {
        Ok(())
    }
}
#[derive(Default)]
pub(crate) struct Work {
    pixels: u64,
    taps: u64,
}
impl Work {
    fn charge(&mut self, x: &Axis, y: &Axis) -> Result<(), DocumentError> {
        self.pixels += 1;
        self.taps = self
            .taps
            .checked_add(x.len().saturating_mul(y.len()))
            .ok_or(DocumentError::ResourceLimit)?;
        if self.pixels > 64_000_000 || self.taps > 256_000_000 {
            return Err(DocumentError::ResourceLimit);
        }
        Ok(())
    }
}
#[derive(Clone, Copy)]
pub(crate) struct Geometry {
    origin: [i64; 2],
    input: [u32; 2],
    output: [u32; 2],
}
impl Geometry {
    pub(crate) fn new(frame: &Frame, size: [u32; 2]) -> Self {
        Self {
            origin: [frame.x, frame.y],
            input: [frame.width, frame.height],
            output: size,
        }
    }
    pub(crate) fn homography(&self) -> Result<crate::geometry::Homography, DocumentError> {
        let [sx, sy] =
            std::array::from_fn(|i| f64::from(self.output[i]) / f64::from(self.input[i]));
        crate::geometry::Homography::affine([
            sx,
            0.0,
            0.0,
            sy,
            self.origin[0] as f64 * (1.0 - sx),
            self.origin[1] as f64 * (1.0 - sy),
        ])
        .map_err(|e| DocumentError::Invalid(e.to_string()))
    }
    fn edge(&self, value: i64, axis: usize, ceil: bool) -> Result<i64, DocumentError> {
        self.half_edge(2 * i128::from(value), axis, ceil)
    }
    fn half_edge(&self, twice_value: i128, axis: usize, ceil: bool) -> Result<i64, DocumentError> {
        let n = (twice_value - 2 * i128::from(self.origin[axis])) * i128::from(self.output[axis]);
        let d = 2 * i128::from(self.input[axis]);
        let v = i128::from(self.origin[axis])
            + if ceil {
                -(-n).div_euclid(d)
            } else {
                n.div_euclid(d)
            };
        if v.abs() > 1_000_000_000_000 {
            return Err(DocumentError::ResourceLimit);
        }
        i64::try_from(v).map_err(|_| DocumentError::ResourceLimit)
    }
    pub(crate) fn position(&self, p: [i64; 2]) -> Result<[i64; 2], DocumentError> {
        Ok([self.edge(p[0], 0, false)?, self.edge(p[1], 1, false)?])
    }
    pub(crate) fn bounds(&self, source: CoverageRect) -> Result<CoverageRect, DocumentError> {
        self.sample_bounds(source, ResizeInterpolation::Nearest)
    }
    fn sample_bounds(
        &self,
        source: CoverageRect,
        sampling: ResizeInterpolation,
    ) -> Result<CoverageRect, DocumentError> {
        source.validate()?;
        let edge = |value: i64, axis: usize, upper: bool| {
            let linear = match sampling {
                ResizeInterpolation::Nearest => false,
                ResizeInterpolation::Bilinear => self.input[axis] != self.output[axis],
                ResizeInterpolation::Area => self.input[axis] < self.output[axis],
            };
            let canvas_edge = self.origin[axis]
                + if upper {
                    i64::from(self.input[axis])
                } else {
                    0
                };
            // Finite interior/off-canvas layer edges blend with transparent
            // neighbors. Only an edge coinciding with the original artboard
            // extends its sample, so an opaque full image keeps opaque borders.
            let pad = if linear && value != canvas_edge {
                if upper { 1 } else { -1 }
            } else {
                0
            };
            self.half_edge(2 * i128::from(value) + pad, axis, upper)
        };
        let x = edge(source.x, 0, false)?;
        let y = edge(source.y, 1, false)?;
        let r = edge(
            source
                .x
                .checked_add(i64::from(source.width))
                .ok_or(DocumentError::ResourceLimit)?,
            0,
            true,
        )?;
        let b = edge(
            source
                .y
                .checked_add(i64::from(source.height))
                .ok_or(DocumentError::ResourceLimit)?,
            1,
            true,
        )?;
        let width = u32::try_from(r - x).map_err(|_| DocumentError::ResourceLimit)?;
        let height = u32::try_from(b - y).map_err(|_| DocumentError::ResourceLimit)?;
        if width == 0 || height == 0 || u64::from(width) * u64::from(height) > 64_000_000 {
            return Err(DocumentError::ResourceLimit);
        }
        Ok(CoverageRect {
            x,
            y,
            width,
            height,
        })
    }
    fn axes(&self, p: [i64; 2], sampling: ResizeInterpolation, bounds: CoverageRect) -> [Axis; 2] {
        [
            self.axis(p[0], 0, sampling, bounds.x, bounds.width),
            self.axis(p[1], 1, sampling, bounds.y, bounds.height),
        ]
    }
    fn axis(
        &self,
        p: i64,
        axis: usize,
        sampling: ResizeInterpolation,
        min: i64,
        count: u32,
    ) -> Axis {
        let origin = self.origin[axis];
        let input = i128::from(self.input[axis]);
        let output = i128::from(self.output[axis]);
        let max = min + i64::from(count) - 1;
        let n = (i128::from(p) - i128::from(origin)) * input;
        match sampling {
            ResizeInterpolation::Nearest => {
                let index = i128::from(origin) + (2 * n + input).div_euclid(2 * output);
                let index = index as i64;
                Axis {
                    first: index,
                    last: index,
                    kernel: Kernel::Point,
                }
            }
            ResizeInterpolation::Area if input > output => {
                let first = (i128::from(origin) + n.div_euclid(output)) as i64;
                let last = (i128::from(origin) - (-(n + input)).div_euclid(output) - 1) as i64;
                Axis {
                    first: first.max(min),
                    last: last.min(max),
                    kernel: Kernel::Area {
                        origin,
                        start: n,
                        end: n + input,
                        cell: output,
                        width: input,
                    },
                }
            }
            ResizeInterpolation::Bilinear | ResizeInterpolation::Area => {
                let n = 2 * n + input - output;
                let d = 2 * output;
                let first = (i128::from(origin) + n.div_euclid(d)) as i64;
                let fraction = n.rem_euclid(d) as f64 / d as f64;
                let last = if fraction == 0. { first } else { first + 1 };
                let edge_index = |v: i64| {
                    if v < min && min == origin {
                        min
                    } else if v > max && max + 1 == origin + i64::from(self.input[axis]) {
                        max
                    } else {
                        v
                    }
                };
                let (first, last) = (edge_index(first), edge_index(last));
                Axis {
                    first,
                    last,
                    kernel: if first == last {
                        Kernel::Point
                    } else {
                        Kernel::Linear(fraction)
                    },
                }
            }
        }
    }
}
enum Kernel {
    Point,
    Linear(f64),
    Area {
        origin: i64,
        start: i128,
        end: i128,
        cell: i128,
        width: i128,
    },
}
struct Axis {
    first: i64,
    last: i64,
    kernel: Kernel,
}
impl Axis {
    fn len(&self) -> u64 {
        (self.last - self.first + 1).max(0) as u64
    }
    fn exact(&self) -> Option<i64> {
        matches!(self.kernel, Kernel::Point).then_some(self.first)
    }
    fn weight(&self, index: i64) -> f64 {
        match self.kernel {
            Kernel::Point => 1.,
            Kernel::Linear(f) => {
                if index == self.first {
                    1. - f
                } else {
                    f
                }
            }
            Kernel::Area {
                origin,
                start,
                end,
                cell,
                width,
            } => {
                let left = (i128::from(index) - i128::from(origin)) * cell;
                (end.min(left + cell) - start.max(left)).max(0) as f64 / width as f64
            }
        }
    }
}
fn raw(p: &PixelBuffer, i: usize) -> [u16; 4] {
    match p {
        PixelBuffer::Rgba8(v) => std::array::from_fn(|c| u16::from(v[4 * i + c])),
        PixelBuffer::Rgba16(v) => std::array::from_fn(|c| v[4 * i + c]),
    }
}
fn put(p: &mut PixelBuffer, i: usize, v: [u16; 4]) {
    match p {
        PixelBuffer::Rgba8(p) => {
            for c in 0..4 {
                p[4 * i + c] = v[c] as u8;
            }
        }
        PixelBuffer::Rgba16(p) => p[4 * i..4 * i + 4].copy_from_slice(&v),
    }
}
fn tiles(bounds: CoverageRect, origin: [i64; 2]) -> ((TileCoord, TileCoord), [i64; 2]) {
    let x = bounds.x - origin[0];
    let y = bounds.y - origin[1];
    let (first, _) = TileCoord::locate(x, y);
    let (last, _) = TileCoord::locate(
        x + i64::from(bounds.width) - 1,
        y + i64::from(bounds.height) - 1,
    );
    ((first, last), origin)
}
pub(crate) fn pixels(
    input: PixelSource<'_>,
    geometry: Geometry,
    sampling: ResizeInterpolation,
    pool: MemoryPool,
    work: &mut Work,
    cancel: &dyn Fn() -> bool,
) -> Result<(TiledPixels, [i64; 2]), DocumentError> {
    check(cancel)?;
    let dest = geometry.sample_bounds(input.bounds, sampling)?;
    let origin = [dest.x, dest.y];
    let format = input.pixels.format();
    let bounds = input.bounds;
    let icc = input.icc;
    let _cache = pool.reserve(MemoryKind::Transient, 8 * 256 * 256 * 24 + 65536)?;
    let mut sampler = crate::geometry_pixels::Sampler::new(input);
    let mut result = TiledPixels::with_pool(
        format,
        (crate::MAX_DOCUMENT_BYTES / (256 * 256 * 4 * format.bytes_per_sample() as u64)) as usize,
        pool.clone(),
    );
    let ((first, last), _) = tiles(dest, origin);
    for ty in first.y()..=last.y() {
        for tx in first.x()..=last.x() {
            check(cancel)?;
            let coord = TileCoord::new(tx, ty)?;
            let [ox, oy] = coord.origin();
            let _scratch = pool.reserve(MemoryKind::Transient, 256 * 256 * 64)?;
            let mut out = match format {
                strata_pixels::PixelFormat::Rgba8 => PixelBuffer::Rgba8(vec![0; 256 * 256 * 4]),
                strata_pixels::PixelFormat::Rgba16 => PixelBuffer::Rgba16(vec![0; 256 * 256 * 4]),
            };
            let mut linear = vec![[0f32; 4]; 256 * 256];
            let mut converted = vec![false; 256 * 256];
            for row in 0..256 {
                check(cancel)?;
                for col in 0..256 {
                    let x = ox + origin[0] + col as i64;
                    let y = oy + origin[1] + row as i64;
                    if !dest.contains(x, y) {
                        continue;
                    }
                    let [xs, ys] = geometry.axes([x, y], sampling, bounds);
                    work.charge(&xs, &ys)?;
                    let i = row * 256 + col;
                    if let (Some(sx), Some(sy)) = (xs.exact(), ys.exact()) {
                        put(&mut out, i, sampler.raw(sx, sy, cancel)?);
                        continue;
                    }
                    let mut sum = [0.; 4];
                    for sy in ys.first..=ys.last {
                        check(cancel)?;
                        let wy = ys.weight(sy);
                        for sx in xs.first..=xs.last {
                            if (sx - xs.first) % 1024 == 0 {
                                check(cancel)?;
                            }
                            let w = xs.weight(sx) * wy;
                            let p = sampler.linear(sx, sy, cancel)?;
                            for c in 0..3 {
                                sum[c] += p[c] * p[3] * w;
                            }
                            sum[3] += p[3] * w;
                        }
                    }
                    if sum[3] > 0. {
                        for c in 0..3 {
                            sum[c] /= sum[3];
                        }
                        linear[i] = sum.map(|v| v as f32);
                        converted[i] = true;
                    }
                }
            }
            if converted.iter().any(|v| *v) {
                let codes = strata_color::from_linear_p3(&linear, format, icc)?;
                for (i, changed) in converted.into_iter().enumerate() {
                    if changed {
                        put(&mut out, i, raw(&codes, i));
                    }
                }
            }
            result.insert_tile(coord, out)?;
        }
    }
    check(cancel)?;
    Ok((result, origin))
}
pub(crate) fn mask(
    source: &TiledMask,
    offset: [i64; 2],
    geometry: Geometry,
    sampling: ResizeInterpolation,
    pool: MemoryPool,
    work: &mut Work,
    cancel: &dyn Fn() -> bool,
) -> Result<TiledMask, DocumentError> {
    check(cancel)?;
    let mut bounds = source.layout().bounds;
    bounds.x = bounds
        .x
        .checked_add(offset[0])
        .ok_or(DocumentError::ResourceLimit)?;
    bounds.y = bounds
        .y
        .checked_add(offset[1])
        .ok_or(DocumentError::ResourceLimit)?;
    let dest = geometry.sample_bounds(bounds, sampling)?;
    let _cache = pool.reserve(MemoryKind::Transient, 8 * 256 * 256 * 2)?;
    let mut sampler = crate::geometry_masks::MaskSampler::new(source);
    let mut result = TiledMask::new(
        MaskLayout {
            bounds: dest,
            default_coverage: 0,
            inverted: false,
        },
        (crate::MAX_DOCUMENT_BYTES / (256 * 256 * 2)) as usize,
        pool.clone(),
    )?;
    let ((first, last), _) = tiles(dest, [0, 0]);
    for ty in first.y()..=last.y() {
        for tx in first.x()..=last.x() {
            check(cancel)?;
            let coordinate = TileCoord::new(tx, ty)?;
            let [ox, oy] = coordinate.origin();
            let _scratch = pool.reserve(MemoryKind::Transient, 256 * 256 * 2)?;
            let mut values = vec![0u16; 256 * 256];
            for row in 0..256 {
                check(cancel)?;
                for col in 0..256 {
                    let x = ox + col as i64;
                    let y = oy + row as i64;
                    if !dest.contains(x, y) {
                        continue;
                    }
                    let [xs, ys] = geometry.axes([x, y], sampling, bounds);
                    work.charge(&xs, &ys)?;
                    let mut value = 0.;
                    for sy in ys.first..=ys.last {
                        check(cancel)?;
                        for sx in xs.first..=xs.last {
                            if (sx - xs.first) % 1024 == 0 {
                                check(cancel)?;
                            }
                            value += f64::from(sampler.sample(
                                sx - offset[0],
                                sy - offset[1],
                                cancel,
                            )?) * xs.weight(sx)
                                * ys.weight(sy);
                        }
                    }
                    values[row * 256 + col] = value.round().clamp(0., 65535.) as u16;
                }
            }
            result.insert_tile(coordinate, values)?;
        }
    }
    check(cancel)?;
    Ok(result)
}
