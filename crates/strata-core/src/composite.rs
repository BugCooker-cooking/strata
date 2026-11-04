//! Linear Display-P3 composition. Source samples stay untouched. The CPU path
//! uses f64 blending; output quantization happens only at the export boundary.
use crate::{BlendMode, Document, DocumentError};
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
        fingerprint(doc, coord)
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
        self.clock += 1;
        let key = fingerprint(doc, coord);
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
            pixels: compose_tile(doc, coord)?,
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

fn fingerprint(doc: &Document, coord: TileCoord) -> u64 {
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    doc.id().hash(&mut hash);
    doc.working_icc().hash(&mut hash);
    let [ox, oy] = coord.origin();
    for layer in doc.layers() {
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
        let mut touched = false;
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
        }
    }
    hash.finish()
}

fn compose_tile(doc: &Document, coord: TileCoord) -> Result<Vec<[f32; 4]>, DocumentError> {
    let _scratch = doc
        .memory_pool()
        .reserve(MemoryKind::Transient, 256 * 256 * (32 + 16))?;
    let mut out = vec![[0.0f64; 4]; 256 * 256];
    let [ox, oy] = coord.origin();
    for layer in doc.layers() {
        if !layer.info.visible || layer.info.opacity == 0.0 {
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
                        out[index] = blend(
                            out[index],
                            linear[ly as usize * 256 + lx as usize].map(f64::from),
                            layer.info.blend_mode,
                            layer.info.opacity,
                        );
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

impl Document {
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
