//! Immutable-input, bounded-halo CPU filters. All storage remains at source
//! depth; floating ICC conversions are used only for temporary filter samples.
use super::*;
use crate::{Document, DocumentError};
use strata_pixels::{
    CoverageRect, PixelBuffer, TileCoord,
    memory::{MemoryKind, Reservation},
};

fn check(cancel: &dyn Fn() -> bool) -> Result<(), CommandError> {
    if cancel() {
        Err(CommandError::Expired)
    } else {
        Ok(())
    }
}
struct FilteredTile {
    bounds: CoverageRect,
    values: Vec<[f32; 4]>,
    _budget: Reservation,
}
impl FilteredTile {
    fn pixel(&self, x: i64, y: i64) -> [f32; 4] {
        self.values[(y - self.bounds.y) as usize * self.bounds.width as usize
            + (x - self.bounds.x) as usize]
    }
}
struct Blur {
    source: Document,
    index: usize,
    bounds: CoverageRect,
    kernel: Vec<f64>,
    radius: usize,
    cached: Option<(TileCoord, FilteredTile)>,
    _budget: Reservation,
}
impl Blur {
    fn new(
        source: &Document,
        index: usize,
        bounds: CoverageRect,
        sigma: f64,
    ) -> Result<Self, CommandError> {
        if !sigma.is_finite() || !(0.1..=64.0).contains(&sigma) {
            return Err(CommandError::Invalid(
                "Gaussian sigma must be 0.1..64 canvas pixels".into(),
            ));
        }
        let radius = (3.0 * sigma).ceil() as usize;
        let budget = source
            .memory_pool()
            .reserve(MemoryKind::Transient, ((radius * 2 + 1) * 8 + 4096) as u64)
            .map_err(DocumentError::from)?;
        let mut kernel = (-(radius as i32)..=radius as i32)
            .map(|v| (-f64::from(v).powi(2) / (2.0 * sigma * sigma)).exp())
            .collect::<Vec<_>>();
        let sum = kernel.iter().sum::<f64>();
        for value in &mut kernel {
            *value /= sum;
        }
        Ok(Self {
            source: source.clone(),
            index,
            bounds,
            kernel,
            radius,
            cached: None,
            _budget: budget,
        })
    }
    fn patch(
        &self,
        b: CoverageRect,
        cancel: &dyn Fn() -> bool,
    ) -> Result<Vec<[f64; 4]>, CommandError> {
        let mut patch = vec![[0.0; 4]; b.width as usize * b.height as usize];
        let frame = self.source.frame();
        let layer = &self.source.layers()[self.index];
        let right = frame.x + i64::from(frame.width) - 1;
        let bottom = frame.y + i64::from(frame.height) - 1;
        let left = b.x.clamp(frame.x, right) - layer.info().x;
        let top = b.y.clamp(frame.y, bottom) - layer.info().y;
        let r = (b.x + i64::from(b.width) - 1).clamp(frame.x, right) - layer.info().x;
        let bot = (b.y + i64::from(b.height) - 1).clamp(frame.y, bottom) - layer.info().y;
        let (first, _) = TileCoord::locate(left, top);
        let (last, _) = TileCoord::locate(r, bot);
        for ty in first.y()..=last.y() {
            for tx in first.x()..=last.x() {
                check(cancel)?;
                let coord = TileCoord::new(tx, ty).map_err(DocumentError::from)?;
                let Some(raw) = layer.pixels().tile(coord).map_err(DocumentError::from)? else {
                    continue;
                };
                let values = strata_color::to_linear_p3(&raw, 256, 256, self.source.working_icc())
                    .map_err(DocumentError::from)?;
                let [ox, oy] = coord.origin();
                let gx = ox + layer.info().x;
                let gy = oy + layer.info().y;
                let x0 = if gx <= frame.x {
                    0
                } else {
                    (gx - b.x).clamp(0, i64::from(b.width)) as usize
                };
                let y0 = if gy <= frame.y {
                    0
                } else {
                    (gy - b.y).clamp(0, i64::from(b.height)) as usize
                };
                let x1 = if gx + 256 > right {
                    b.width as usize
                } else {
                    (gx + 256 - b.x).clamp(0, i64::from(b.width)) as usize
                };
                let y1 = if gy + 256 > bottom {
                    b.height as usize
                } else {
                    (gy + 256 - b.y).clamp(0, i64::from(b.height)) as usize
                };
                for y in y0..y1 {
                    check(cancel)?;
                    let sy = (b.y + y as i64).clamp(frame.y, bottom) - gy;
                    for x in x0..x1 {
                        let sx = (b.x + x as i64).clamp(frame.x, right) - gx;
                        let p = values[sy as usize * 256 + sx as usize].map(f64::from);
                        patch[y * b.width as usize + x] =
                            [p[0] * p[3], p[1] * p[3], p[2] * p[3], p[3]];
                    }
                }
            }
        }
        Ok(patch)
    }
    fn compute(
        &self,
        coord: TileCoord,
        cancel: &dyn Fn() -> bool,
    ) -> Result<FilteredTile, CommandError> {
        let layer = &self.source.layers()[self.index];
        let [ox, oy] = coord.origin();
        let gx = ox + layer.info().x;
        let gy = oy + layer.info().y;
        let x = gx.max(self.bounds.x);
        let y = gy.max(self.bounds.y);
        let r = (gx + 256).min(self.bounds.x + i64::from(self.bounds.width));
        let bottom = (gy + 256).min(self.bounds.y + i64::from(self.bounds.height));
        let b = CoverageRect {
            x,
            y,
            width: (r - x) as u32,
            height: (bottom - y) as u32,
        };
        let w = b.width as usize;
        let h = b.height as usize;
        let pw = w + 2 * self.radius;
        let ph = h + 2 * self.radius;
        let output_budget = self
            .source
            .memory_pool()
            .reserve(MemoryKind::Transient, (w * h * 16) as u64 + 1024)
            .map_err(DocumentError::from)?;
        let _scratch = self
            .source
            .memory_pool()
            .reserve(
                MemoryKind::Transient,
                ((pw * ph + ph * w) * 32 + 256 * 256 * 24 + 32768) as u64,
            )
            .map_err(DocumentError::from)?;
        let patch = self.patch(
            CoverageRect {
                x: x - self.radius as i64,
                y: y - self.radius as i64,
                width: pw as u32,
                height: ph as u32,
            },
            cancel,
        )?;
        let mut horizontal = vec![[0.0f64; 4]; ph * w];
        for y in 0..ph {
            check(cancel)?;
            for x in 0..w {
                let mut sum = [0.0f64; 4];
                for (k, &weight) in self.kernel.iter().enumerate() {
                    let p = patch[y * pw + x + k];
                    for c in 0..4 {
                        sum[c] += p[c] * weight;
                    }
                }
                horizontal[y * w + x] = sum;
            }
        }
        drop(patch);
        let mut values = vec![[0.0f32; 4]; w * h];
        for y in 0..h {
            check(cancel)?;
            for x in 0..w {
                let mut sum = [0.0f64; 4];
                for (k, &weight) in self.kernel.iter().enumerate() {
                    let p = horizontal[(y + k) * w + x];
                    for c in 0..4 {
                        sum[c] += p[c] * weight;
                    }
                }
                let alpha = sum[3].clamp(0.0, 1.0);
                values[y * w + x] = if alpha > 1e-12 {
                    [
                        (sum[0] / alpha) as f32,
                        (sum[1] / alpha) as f32,
                        (sum[2] / alpha) as f32,
                        alpha as f32,
                    ]
                } else {
                    [0.0; 4]
                };
            }
        }
        Ok(FilteredTile {
            bounds: b,
            values,
            _budget: output_budget,
        })
    }
    fn pixel(
        &mut self,
        x: i64,
        y: i64,
        cancel: &dyn Fn() -> bool,
    ) -> Result<[f32; 4], CommandError> {
        let layer = &self.source.layers()[self.index];
        let (coord, _) = TileCoord::locate(x - layer.info().x, y - layer.info().y);
        if self.cached.as_ref().is_none_or(|(c, _)| *c != coord) {
            self.cached = None;
            self.cached = Some((coord, self.compute(coord, cancel)?));
        }
        Ok(self.cached.as_ref().unwrap().1.pixel(x, y))
    }
}
fn mix(mut v: u64) -> u64 {
    v = (v ^ (v >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    v = (v ^ (v >> 27)).wrapping_mul(0x94d049bb133111eb);
    v ^ (v >> 31)
}
fn uniform(seed: u32, x: i64, y: i64, channel: u64, stream: u64) -> f64 {
    let key = u64::from(seed)
        ^ (x as u64).wrapping_mul(0x9e3779b97f4a7c15)
        ^ (y as u64).wrapping_mul(0xd1b54a32d192ed03)
        ^ channel.wrapping_mul(0x94d049bb133111eb)
        ^ stream.wrapping_mul(0xbf58476d1ce4e5b9);
    ((mix(key.wrapping_add(0x9e3779b97f4a7c15)) >> 11) as f64 + 0.5) / 9007199254740992.0
}
fn noise(p: &AddNoise, x: i64, y: i64, c: u64) -> f64 {
    let c = if p.monochrome { 0 } else { c };
    let u = uniform(p.seed, x, y, c, 0);
    p.amount
        * match p.distribution {
            NoiseDistribution::Uniform => 2.0 * u - 1.0,
            NoiseDistribution::Gaussian => {
                (-2.0 * u.ln()).sqrt() * (std::f64::consts::TAU * uniform(p.seed, x, y, c, 1)).cos()
            }
        }
}
struct Noise {
    source: Document,
    index: usize,
    p: AddNoise,
    cached: Option<(TileCoord, FilteredTile)>,
}
impl Noise {
    fn pixel(
        &mut self,
        x: i64,
        y: i64,
        cancel: &dyn Fn() -> bool,
    ) -> Result<[f32; 4], CommandError> {
        let layer = &self.source.layers()[self.index];
        let info = layer.info();
        let (coord, _) = TileCoord::locate(x - info.x, y - info.y);
        if self.cached.as_ref().is_none_or(|(c, _)| *c != coord) {
            self.cached = None;
            check(cancel)?;
            let [ox, oy] = coord.origin();
            let budget = self
                .source
                .memory_pool()
                .reserve(MemoryKind::Transient, 256 * 256 * 48)
                .map_err(DocumentError::from)?;
            let raw = layer.pixels().tile(coord).map_err(DocumentError::from)?;
            let mut encoded = vec![[0.0f32; 4]; 256 * 256];
            if let Some(raw) = raw {
                for row in 0..256 {
                    check(cancel)?;
                    for col in 0..256 {
                        let i = row * 256 + col;
                        encoded[i] = match &*raw {
                            PixelBuffer::Rgba8(v) => {
                                std::array::from_fn(|c| f32::from(v[i * 4 + c]) / 255.0)
                            }
                            PixelBuffer::Rgba16(v) => {
                                std::array::from_fn(|c| f32::from(v[i * 4 + c]) / 65535.0)
                            }
                        };
                        if encoded[i][3] > 0.0 {
                            for (c, value) in encoded[i].iter_mut().enumerate().take(3) {
                                *value = (f64::from(*value)
                                    + noise(
                                        &self.p,
                                        ox + info.x + col as i64,
                                        oy + info.y + row as i64,
                                        c as u64,
                                    ))
                                .clamp(0.0, 1.0) as f32;
                            }
                        }
                    }
                }
            }
            let values = strata_color::working_to_linear_p3(&encoded, self.source.working_icc())
                .map_err(DocumentError::from)?;
            self.cached = Some((
                coord,
                FilteredTile {
                    bounds: CoverageRect {
                        x: ox + info.x,
                        y: oy + info.y,
                        width: 256,
                        height: 256,
                    },
                    values,
                    _budget: budget,
                },
            ));
        }
        Ok(self.cached.as_ref().unwrap().1.pixel(x, y))
    }
}
pub(super) fn mutate(
    doc: &mut Document,
    id: &str,
    params: Value,
    cancel: &dyn Fn() -> bool,
) -> Result<Option<Value>, CommandError> {
    if !matches!(
        id,
        "filter.gaussian_blur" | "filter.unsharp_mask" | "filter.add_noise"
    ) {
        return Ok(None);
    }
    check(cancel)?;
    let (layer, sigma, unsharp, noise_params) = match id {
        "filter.gaussian_blur" => {
            let p: GaussianBlur = parse(params)?;
            (p.layer, p.sigma, None, None)
        }
        "filter.unsharp_mask" => {
            let p: UnsharpMask = parse(params)?;
            if !p.amount.is_finite()
                || !(0.0..=5.0).contains(&p.amount)
                || !p.threshold.is_finite()
                || !(0.0..=1.0).contains(&p.threshold)
            {
                return Err(CommandError::Invalid(
                    "USM amount must be 0..5 and threshold 0..1 linear RGB".into(),
                ));
            }
            (p.layer, p.sigma, Some((p.amount, p.threshold)), None)
        }
        _ => {
            let p: AddNoise = parse(params)?;
            if !p.amount.is_finite() || !(0.0..=1.0).contains(&p.amount) {
                return Err(CommandError::Invalid(
                    "noise amount must be 0..1 working RGB".into(),
                ));
            }
            (p.layer.clone(), 2.0, None, Some(p))
        }
    };
    if !sigma.is_finite() || !(0.1..=64.0).contains(&sigma) {
        return Err(CommandError::Invalid(
            "Gaussian sigma must be 0.1..64 canvas pixels".into(),
        ));
    }
    let (target, index) = super::pixel_transform::target(doc, &layer)?;
    let empty = || serde_json::json!({"layer_id":target,"target":"pixels","touched_tiles":0});
    if unsharp.is_some_and(|(a, _)| a == 0.0)
        || noise_params.as_ref().is_some_and(|p| p.amount == 0.0)
    {
        return Ok(Some(empty()));
    }
    let coverage = doc
        .selection()
        .cloned()
        .map(Ok)
        .unwrap_or_else(|| super::local::uniform(doc, 65535))?;
    let Some(bounds) = coverage.content_bounds().map_err(DocumentError::from)? else {
        return Ok(Some(empty()));
    };
    if let Some(p) = noise_params {
        let mut source = Noise {
            source: doc.clone(),
            index,
            p,
            cached: None,
        };
        return Ok(Some(super::pixel_transform::apply(
            doc,
            layer,
            &coverage,
            |old, x, y| {
                if old[3] == 0.0 {
                    Ok(old)
                } else {
                    source.pixel(x, y, cancel)
                }
            },
            cancel,
        )?));
    }
    let mut blur = Blur::new(doc, index, bounds, sigma)?;
    Ok(Some(super::pixel_transform::apply(
        doc,
        layer,
        &coverage,
        |old, x, y| {
            if unsharp.is_some() && old[3] == 0.0 {
                return Ok(old);
            }
            let filtered = blur.pixel(x, y, cancel)?;
            if let Some((amount, threshold)) = unsharp {
                if filtered[3] <= 1e-12 {
                    return Ok(old);
                }
                let delta =
                    std::array::from_fn::<_, 3, _>(|c| f64::from(old[c]) - f64::from(filtered[c]));
                if delta.iter().all(|v| v.abs() < threshold) {
                    return Ok(old);
                }
                Ok([
                    (f64::from(old[0]) + amount * delta[0]).clamp(0.0, 1.0) as f32,
                    (f64::from(old[1]) + amount * delta[1]).clamp(0.0, 1.0) as f32,
                    (f64::from(old[2]) + amount * delta[2]).clamp(0.0, 1.0) as f32,
                    old[3],
                ])
            } else {
                Ok(filtered)
            }
        },
        cancel,
    )?))
}
