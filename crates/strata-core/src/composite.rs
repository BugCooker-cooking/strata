//! Linear Display-P3 composition. Source samples stay untouched. The CPU path
//! uses f64 blending; output quantization happens only at the export boundary.
use crate::{BlendMode, Document, DocumentError, LayerKind};
use std::{
    collections::HashMap,
    hash::{Hash, Hasher},
    sync::Arc,
};
use strata_pixels::memory::{CacheSlot, MemoryKind, Reservation};
use strata_pixels::{PixelBuffer, PixelFormat, TILE_EDGE, TileCoord};

pub type LinearTile = Arc<LinearPixels>; // straight linear P3, row-major
pub struct LinearPixels {
    pixels: Vec<[f32; 4]>,
    _reservation: Reservation,
}
impl LinearPixels {
    pub fn new(
        pixels: Vec<[f32; 4]>,
        pool: &strata_pixels::memory::MemoryPool,
    ) -> Result<Self, strata_pixels::memory::MemoryError> {
        let reservation = pool.reserve(MemoryKind::CpuCache, (pixels.capacity() * 16) as u64)?;
        Ok(Self {
            pixels,
            _reservation: reservation,
        })
    }
}
impl std::ops::Deref for LinearPixels {
    type Target = Vec<[f32; 4]>;
    fn deref(&self) -> &Self::Target {
        &self.pixels
    }
}

pub fn blend(back: [f64; 4], front: [f64; 4], mode: BlendMode, opacity: f64) -> [f64; 4] {
    let ab = back[3];
    let asrc = front[3] * opacity;
    let alpha = asrc + ab * (1.0 - asrc);
    if alpha == 0.0 {
        return [0.0; 4];
    }
    let mut out = [0.0; 4];
    out[3] = alpha;
    for i in 0..3 {
        let b = back[i];
        let s = front[i];
        let mixed = match mode {
            BlendMode::Normal => s,
            BlendMode::Multiply => b * s,
            BlendMode::Screen => b + s - b * s,
            BlendMode::Overlay => {
                if b <= 0.5 {
                    2.0 * b * s
                } else {
                    1.0 - 2.0 * (1.0 - b) * (1.0 - s)
                }
            }
        };
        out[i] = ((1.0 - asrc) * ab * b + (1.0 - ab) * asrc * s + ab * asrc * mixed) / alpha;
    }
    out
}

#[derive(Clone)]
pub struct Compositor {
    cache: HashMap<TileCoord, (u64, Arc<CacheSlot<LinearPixels>>, u64)>,
    clock: u64,
    capacity: usize,
    pub recomputed: u64,
}
impl Compositor {
    pub fn signature(doc: &Document, coord: TileCoord) -> u64 {
        fingerprint(doc, coord, doc.layers().len())
    }
    pub fn signature_prefix(doc: &Document, coord: TileCoord, layers: usize) -> u64 {
        fingerprint(doc, coord, layers)
    }
    pub fn new(capacity: usize) -> Self {
        Self {
            cache: HashMap::new(),
            clock: 0,
            capacity: capacity.max(1),
            recomputed: 0,
        }
    }
    pub fn clear(&mut self) {
        self.cache.clear();
    }
    pub fn tile(&mut self, doc: &Document, coord: TileCoord) -> Result<LinearTile, DocumentError> {
        self.tile_prefix(doc, coord, doc.layers().len())
    }
    /// Display-only prefix composition; the immutable document stays complete.
    pub fn tile_prefix(
        &mut self,
        doc: &Document,
        coord: TileCoord,
        layers: usize,
    ) -> Result<LinearTile, DocumentError> {
        self.clock += 1;
        let key = fingerprint(doc, coord, layers);
        if let Some((old, tile, used)) = self.cache.get_mut(&coord)
            && *old == key
            && let Some(tile) = tile.get()
        {
            *used = self.clock;
            return Ok(tile);
        }
        let pool = doc.memory_pool();
        let reservation = pool.reserve(MemoryKind::CpuCache, 256 * 256 * 16)?;
        let tile = Arc::new(LinearPixels {
            pixels: compose_tile(doc, coord, layers)?,
            _reservation: reservation,
        });
        // Pin before global registration: a pressure callback may trim caches
        // concurrently, including a just-created slot.
        let slot = CacheSlot::from_arc(&pool, tile.clone());
        self.recomputed += 1;
        if self.cache.len() >= self.capacity
            && let Some(victim) = self.cache.iter().min_by_key(|(_, v)| v.2).map(|(k, _)| *k)
        {
            self.cache.remove(&victim);
        }
        self.cache.insert(coord, (key, slot, self.clock));
        Ok(tile)
    }
    pub fn cached_tiles(&self) -> usize {
        self.cache.len()
    }
}

fn fingerprint(doc: &Document, coord: TileCoord, layers: usize) -> u64 {
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    doc.id().hash(&mut hash);
    doc.working_icc().hash(&mut hash);
    let [ox, oy] = coord.origin();
    for layer in doc.layers().iter().take(layers) {
        if !layer.info.visible || layer.info.opacity == 0.0 {
            continue;
        }
        let Some(x) = ox.checked_sub(layer.info.x) else {
            continue;
        };
        let Some(y) = oy.checked_sub(layer.info.y) else {
            continue;
        };
        let (first, _) = TileCoord::locate(x, y);
        let (last, _) = TileCoord::locate(x.saturating_add(255), y.saturating_add(255));
        let mut touched = matches!(layer.info.kind, LayerKind::Exposure { .. });
        for tx in first.x()..=last.x() {
            for ty in first.y()..=last.y() {
                if let Ok(c) = TileCoord::new(tx, ty)
                    && layer.pixels.contains_tile(c)
                {
                    touched = true;
                    layer.pixels.tile_version(c).hash(&mut hash);
                    c.hash(&mut hash);
                }
            }
        }
        if touched {
            layer.info.id.hash(&mut hash);
            layer.info.x.hash(&mut hash);
            layer.info.y.hash(&mut hash);
            layer.info.opacity.to_bits().hash(&mut hash);
            std::mem::discriminant(&layer.info.blend_mode).hash(&mut hash);
            std::mem::discriminant(&layer.info.kind).hash(&mut hash);
            if let LayerKind::Exposure { stops } = layer.info.kind {
                stops.to_bits().hash(&mut hash);
            }
            if let Some(mask) = &layer.mask {
                mask.layout().hash(&mut hash);
                if let Some(meta) = &layer.info.mask {
                    let mx = i128::from(ox) - i128::from(layer.info.x) + i128::from(meta.anchor_x);
                    let my = i128::from(oy) - i128::from(layer.info.y) + i128::from(meta.anchor_y);
                    for tx in mx.div_euclid(256)..=(mx + 255).div_euclid(256) {
                        for ty in my.div_euclid(256)..=(my + 255).div_euclid(256) {
                            if let (Ok(tx), Ok(ty)) = (i64::try_from(tx), i64::try_from(ty))
                                && let Ok(c) = TileCoord::new(tx, ty)
                            {
                                mask.tile_version(c).hash(&mut hash);
                                c.hash(&mut hash);
                            }
                        }
                    }
                }
                layer
                    .info
                    .mask
                    .as_ref()
                    .map(|m| (m.enabled, m.anchor_x, m.anchor_y))
                    .hash(&mut hash);
            }
        }
    }
    hash.finish()
}

fn compose_tile(
    doc: &Document,
    coord: TileCoord,
    layers: usize,
) -> Result<Vec<[f32; 4]>, DocumentError> {
    let _scratch = doc
        .memory_pool()
        .reserve(MemoryKind::Transient, 256 * 256 * (32 + 16 + 2))?;
    let [ox, oy] = coord.origin();
    // The common imported-photo case has no layer mixing. Avoid a full f64
    // accumulator and per-pixel coordinate/blend work for aligned source tiles.
    let mut visible = doc
        .layers()
        .iter()
        .take(layers)
        .filter(|layer| layer.info.visible && layer.info.opacity > 0.0);
    if let Some(layer) = visible.next()
        && visible.next().is_none()
        && layer.info.opacity == 1.0
        && layer.info.blend_mode == BlendMode::Normal
        && matches!(layer.info.kind, LayerKind::Pixel)
        && layer.mask.is_none()
        && let Some(sx) = ox.checked_sub(layer.info.x)
        && let Some(sy) = oy.checked_sub(layer.info.y)
        && sx.rem_euclid(256) == 0
        && sy.rem_euclid(256) == 0
    {
        let (source_coord, _) = TileCoord::locate(sx, sy);
        if let Some(source) = layer.pixels.tile(source_coord)? {
            let mut linear =
                strata_color::to_linear_p3(&source, TILE_EDGE, TILE_EDGE, doc.working_icc())?;
            for pixel in &mut linear {
                if pixel[3] == 0.0 {
                    *pixel = [0.0; 4];
                }
            }
            return Ok(linear);
        }
    }
    let mut out = vec![[0.0f64; 4]; 256 * 256];
    for layer in doc.layers().iter().take(layers) {
        if !layer.info.visible || layer.info.opacity == 0.0 {
            continue;
        }
        let mask = layer_mask_tile(layer, ox, oy)?;
        if let LayerKind::Exposure { stops } = layer.info.kind {
            let gain = 2f64.powf(stops);
            for y in 0..256usize {
                for x in 0..256usize {
                    let index = y * 256 + x;
                    let amount = f64::from(mask.as_ref().map_or(65535, |m| m[index])) / 65535.0
                        * layer.info.opacity;
                    for channel in &mut out[index][..3] {
                        let back = *channel;
                        let adjusted = back * gain;
                        let mixed = match layer.info.blend_mode {
                            BlendMode::Normal => adjusted,
                            BlendMode::Multiply => back * adjusted,
                            BlendMode::Screen => back + adjusted - back * adjusted,
                            BlendMode::Overlay if back <= 0.5 => 2.0 * back * adjusted,
                            BlendMode::Overlay => 1.0 - 2.0 * (1.0 - back) * (1.0 - adjusted),
                        };
                        *channel += (mixed - back) * amount;
                    }
                }
            }
            continue;
        }
        let Some(sx) = ox.checked_sub(layer.info.x) else {
            continue;
        };
        let Some(sy) = oy.checked_sub(layer.info.y) else {
            continue;
        };
        let (first, _) = TileCoord::locate(sx, sy);
        let (last, _) = TileCoord::locate(sx.saturating_add(255), sy.saturating_add(255));
        for tx in first.x()..=last.x() {
            for ty in first.y()..=last.y() {
                let source_coord = TileCoord::new(tx, ty)?;
                let Some(source) = layer.pixels.tile(source_coord)? else {
                    continue;
                };
                let linear =
                    strata_color::to_linear_p3(&source, TILE_EDGE, TILE_EDGE, doc.working_icc())?;
                let [px, py] = source_coord.origin();
                for y in 0..256usize {
                    for x in 0..256usize {
                        let Some(lx) = sx.checked_add(x as i64).and_then(|v| v.checked_sub(px))
                        else {
                            continue;
                        };
                        let Some(ly) = sy.checked_add(y as i64).and_then(|v| v.checked_sub(py))
                        else {
                            continue;
                        };
                        if !(0..256).contains(&lx) || !(0..256).contains(&ly) {
                            continue;
                        }
                        let index = y * 256 + x;
                        let mut front = linear[ly as usize * 256 + lx as usize].map(f64::from);
                        front[3] *= f64::from(mask.as_ref().map_or(65535, |m| m[index])) / 65535.0;
                        out[index] =
                            blend(out[index], front, layer.info.blend_mode, layer.info.opacity);
                    }
                }
            }
        }
    }
    // Avoid retaining the larger f64 allocation through an in-place collect.
    let mut result = Vec::with_capacity(out.len());
    for pixel in out {
        result.push(pixel.map(|v| v as f32));
    }
    Ok(result)
}

pub fn layer_mask_tile(
    layer: &crate::Layer,
    x: i64,
    y: i64,
) -> Result<Option<Vec<u16>>, DocumentError> {
    let Some(mask) = &layer.mask else {
        return Ok(None);
    };
    let Some(meta) = &layer.info.mask else {
        return Ok(None);
    };
    if !meta.enabled {
        return Ok(None);
    }
    let mx = i128::from(x) - i128::from(layer.info.x) + i128::from(meta.anchor_x);
    let my = i128::from(y) - i128::from(layer.info.y) + i128::from(meta.anchor_y);
    let bounds = mask.layout().bounds;
    if mx + 256 <= i128::from(bounds.x)
        || my + 256 <= i128::from(bounds.y)
        || mx >= i128::from(bounds.x) + i128::from(bounds.width)
        || my >= i128::from(bounds.y) + i128::from(bounds.height)
    {
        return Ok(Some(vec![0; 256 * 256]));
    }
    let (Ok(mx), Ok(my)) = (i64::try_from(mx), i64::try_from(my)) else {
        return Ok(Some(vec![0; 256 * 256]));
    };
    Ok(Some(mask.region_tile(mx, my)?))
}
pub fn layer_mask_signature(layer: &crate::Layer, coord: TileCoord) -> u64 {
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    layer.info.x.hash(&mut hash);
    layer.info.y.hash(&mut hash);
    if let (Some(mask), Some(meta)) = (&layer.mask, &layer.info.mask) {
        meta.enabled.hash(&mut hash);
        meta.anchor_x.hash(&mut hash);
        meta.anchor_y.hash(&mut hash);
        mask.layout().hash(&mut hash);
        let [ox, oy] = coord.origin();
        let mx = i128::from(ox) - i128::from(layer.info.x) + i128::from(meta.anchor_x);
        let my = i128::from(oy) - i128::from(layer.info.y) + i128::from(meta.anchor_y);
        for tx in mx.div_euclid(256)..=(mx + 255).div_euclid(256) {
            for ty in my.div_euclid(256)..=(my + 255).div_euclid(256) {
                if let (Ok(tx), Ok(ty)) = (i64::try_from(tx), i64::try_from(ty))
                    && let Ok(c) = TileCoord::new(tx, ty)
                {
                    c.hash(&mut hash);
                    mask.tile_version(c).hash(&mut hash);
                }
            }
        }
    }
    hash.finish()
}

impl Document {
    pub fn histogram(&self) -> Result<crate::commands::HistogramResult, DocumentError> {
        let width = self.frame.width.min(256);
        let height = self.frame.height.min(256);
        let _scratch = self.pool.reserve(
            MemoryKind::Transient,
            u64::from(width) * u64::from(height) * 32,
        )?;
        let mut samples = Vec::with_capacity((width * height) as usize);
        for y in 0..height {
            for x in 0..width {
                let sx = self.frame.x
                    + (((u64::from(x) * 2 + 1) * u64::from(self.frame.width))
                        / (u64::from(width) * 2)) as i64;
                let sy = self.frame.y
                    + (((u64::from(y) * 2 + 1) * u64::from(self.frame.height))
                        / (u64::from(height) * 2)) as i64;
                let (coord, [px, py]) = TileCoord::locate(sx, sy);
                samples.push((coord, usize::from(py) * 256 + usize::from(px)));
            }
        }
        samples.sort_unstable_by_key(|s| s.0);
        let mut compositor = Compositor::new(1);
        let mut last = None;
        let mut luminance = vec![0; 256];
        let mut nontransparent_pixels = 0;
        for (coord, index) in samples {
            if last.as_ref().is_none_or(|(c, _)| *c != coord) {
                last = Some((coord, compositor.tile(self, coord)?));
            }
            let p = last.as_ref().expect("sample tile").1[index];
            if p[3] > 0.0 {
                let l = (p[0] * 0.2289746 + p[1] * 0.6917385 + p[2] * 0.0792869).clamp(0.0, 1.0);
                luminance[(l * 255.0).round() as usize] += 1;
                nontransparent_pixels += 1;
            }
        }
        Ok(crate::commands::HistogramResult {
            luminance,
            sampled_pixels: (width * height) as usize,
            nontransparent_pixels,
            method: "bounded pixel-center samples; linear Display P3 luminance".into(),
        })
    }
    /// Export from source/CPU composition, never from an 8-bit display texture.
    pub fn export_pixels(&self) -> Result<PixelBuffer, DocumentError> {
        self.validate()?;
        let count = self
            .format
            .sample_count(self.frame.width, self.frame.height)?;
        // Exact identity path, including hidden RGB at alpha=0.
        let visible: Vec<_> = self
            .layers
            .iter()
            .filter(|l| l.info.visible && l.info.opacity > 0.0)
            .collect();
        if visible.len() == 1
            && visible[0].info.opacity == 1.0
            && visible[0].info.blend_mode == BlendMode::Normal
            && matches!(visible[0].info.kind, LayerKind::Pixel)
            && visible[0].mask.is_none()
        {
            let l = visible[0];
            let mut out = match self.format {
                PixelFormat::Rgba8 => PixelBuffer::Rgba8(vec![0; count]),
                PixelFormat::Rgba16 => PixelBuffer::Rgba16(vec![0; count]),
            };
            for (coord, tile) in l.pixels.tiles() {
                let tile = tile?;
                let [ox, oy] = coord.origin();
                for y in 0..256i64 {
                    let Some(dy) = oy
                        .checked_add(y)
                        .and_then(|v| v.checked_add(l.info.y))
                        .and_then(|v| v.checked_sub(self.frame.y))
                    else {
                        continue;
                    };
                    if !(0..i64::from(self.frame.height)).contains(&dy) {
                        continue;
                    }
                    for x in 0..256i64 {
                        let Some(dx) = ox
                            .checked_add(x)
                            .and_then(|v| v.checked_add(l.info.x))
                            .and_then(|v| v.checked_sub(self.frame.x))
                        else {
                            continue;
                        };
                        if !(0..i64::from(self.frame.width)).contains(&dx) {
                            continue;
                        }
                        let i = (dy as usize * self.frame.width as usize + dx as usize) * 4;
                        let src = (y as usize * 256 + x as usize) * 4;
                        match (&mut out, &*tile) {
                            (PixelBuffer::Rgba8(v), PixelBuffer::Rgba8(p)) => {
                                v[i..i + 4].copy_from_slice(&p[src..src + 4])
                            }
                            (PixelBuffer::Rgba16(v), PixelBuffer::Rgba16(p)) => {
                                v[i..i + 4].copy_from_slice(&p[src..src + 4])
                            }
                            _ => unreachable!(),
                        }
                    }
                }
            }
            return Ok(out);
        }
        let mut out = match self.format {
            PixelFormat::Rgba8 => PixelBuffer::Rgba8(vec![0; count]),
            PixelFormat::Rgba16 => PixelBuffer::Rgba16(vec![0; count]),
        };
        let mut compositor = Compositor::new(1);
        let (first, _) = TileCoord::locate(self.frame.x, self.frame.y);
        let (last, _) = TileCoord::locate(
            self.frame.x + i64::from(self.frame.width) - 1,
            self.frame.y + i64::from(self.frame.height) - 1,
        );
        for tx in first.x()..=last.x() {
            for ty in first.y()..=last.y() {
                let coord = TileCoord::new(tx, ty)?;
                let linear = compositor.tile(self, coord)?;
                let converted =
                    strata_color::from_linear_p3(&linear, self.format, self.working_icc())?;
                let [ox, oy] = coord.origin();
                for y in 0..256i64 {
                    for x in 0..256i64 {
                        let dx = ox + x - self.frame.x;
                        let dy = oy + y - self.frame.y;
                        if dx < 0
                            || dy < 0
                            || dx >= i64::from(self.frame.width)
                            || dy >= i64::from(self.frame.height)
                        {
                            continue;
                        }
                        let dest = (dy as usize * self.frame.width as usize + dx as usize) * 4;
                        let src = (y as usize * 256 + x as usize) * 4;
                        match (&mut out, &converted) {
                            (PixelBuffer::Rgba8(v), PixelBuffer::Rgba8(p)) => {
                                v[dest..dest + 4].copy_from_slice(&p[src..src + 4])
                            }
                            (PixelBuffer::Rgba16(v), PixelBuffer::Rgba16(p)) => {
                                v[dest..dest + 4].copy_from_slice(&p[src..src + 4])
                            }
                            _ => unreachable!(),
                        }
                    }
                }
            }
        }
        Ok(out)
    }
}
