//! Compact full-precision coverage planes. Sparse blocks encode XOR deltas
//! from a default value; inversion is metadata and outside bounds is always 0.
use crate::{
    TILE_EDGE, TileCoord, TileError,
    memory::{DiskSlot, Evictable, MemoryError, MemoryKind, MemoryPool, Reservation},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    ops::Deref,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

const SAMPLES: usize = TILE_EDGE as usize * TILE_EDGE as usize;
const BYTES: u64 = SAMPLES as u64 * 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CoverageRect {
    pub x: i64,
    pub y: i64,
    pub width: u32,
    pub height: u32,
}
impl CoverageRect {
    pub fn validate(self) -> Result<(), TileError> {
        if self.width == 0
            || self.height == 0
            || self.x.checked_add(i64::from(self.width)).is_none()
            || self.y.checked_add(i64::from(self.height)).is_none()
        {
            return Err(TileError::InvalidCoordinate {
                x: self.x,
                y: self.y,
            });
        }
        Ok(())
    }
    pub fn contains(self, x: i64, y: i64) -> bool {
        x >= self.x
            && y >= self.y
            && i128::from(x) < i128::from(self.x) + i128::from(self.width)
            && i128::from(y) < i128::from(self.y) + i128::from(self.height)
    }
    pub fn intersects_tile(self, coord: TileCoord) -> bool {
        let [x, y] = coord.origin();
        i128::from(x) < i128::from(self.x) + i128::from(self.width)
            && i128::from(y) < i128::from(self.y) + i128::from(self.height)
            && i128::from(x) + 256 > i128::from(self.x)
            && i128::from(y) + 256 > i128::from(self.y)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MaskLayout {
    pub bounds: CoverageRect,
    pub default_coverage: u16,
    pub inverted: bool,
}

#[derive(Debug)]
struct Resident {
    samples: Vec<u16>,
    _reservation: Reservation,
}
#[derive(Debug)]
struct Data {
    resident: Option<Arc<Resident>>,
    backing: Option<DiskSlot>,
}
#[derive(Debug)]
struct Block {
    data: Mutex<Data>,
    pool: MemoryPool,
    version: u64,
    used: AtomicU64,
}
impl Block {
    fn new(pool: &MemoryPool, samples: Vec<u16>) -> Result<Arc<Self>, TileError> {
        let reservation = pool.reserve(MemoryKind::Source, samples.capacity() as u64 * 2)?;
        let block = Arc::new(Self {
            data: Mutex::new(Data {
                resident: Some(Arc::new(Resident {
                    samples,
                    _reservation: reservation,
                })),
                backing: None,
            }),
            pool: pool.clone(),
            version: crate::tiles::block_version(),
            used: AtomicU64::new(pool.tick()),
        });
        pool.register(block.clone());
        Ok(block)
    }
    fn load(self: &Arc<Self>) -> Result<MaskTile, TileError> {
        self.used.store(self.pool.tick(), Ordering::Relaxed);
        let mut data = self.data.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(resident) = &data.resident {
            return Ok(MaskTile(resident.clone()));
        }
        let reservation = self.pool.reserve(MemoryKind::Source, BYTES)?;
        let mut samples = allocated(0)?;
        self.pool.reload(
            data.backing.as_ref().expect("nonresident mask has backing"),
            bytemuck::cast_slice_mut(&mut samples),
        )?;
        let resident = Arc::new(Resident {
            samples,
            _reservation: reservation,
        });
        data.resident = Some(resident.clone());
        drop(data);
        self.pool.register(self.clone());
        self.pool.reloaded();
        Ok(MaskTile(resident))
    }
}
impl Evictable for Block {
    fn kind(&self) -> MemoryKind {
        MemoryKind::Source
    }
    fn used(&self) -> u64 {
        self.used.load(Ordering::Relaxed)
    }
    fn evict(&self) -> Result<bool, MemoryError> {
        let Ok(mut data) = self.data.try_lock() else {
            return Ok(false);
        };
        let Some(resident) = &data.resident else {
            return Ok(false);
        };
        if Arc::strong_count(resident) > 1 {
            return Ok(false);
        }
        if data.backing.is_none() {
            data.backing = Some(self.pool.spill(bytemuck::cast_slice(&resident.samples))?);
        }
        data.resident = None;
        Ok(true)
    }
}

/// A pinned encoded block; XOR with layout.default_coverage and optional
/// inversion to decode, inside layout.bounds only. Cloning pins the same bytes.
#[derive(Clone, Debug)]
pub struct MaskTile(Arc<Resident>);
impl Deref for MaskTile {
    type Target = [u16];
    fn deref(&self) -> &[u16] {
        &self.0.samples
    }
}

#[derive(Clone, Debug)]
pub struct TiledMask {
    layout: MaskLayout,
    tiles: BTreeMap<TileCoord, Arc<Block>>,
    pool: MemoryPool,
    max_tiles: usize,
}
impl TiledMask {
    pub fn new(layout: MaskLayout, max_tiles: usize, pool: MemoryPool) -> Result<Self, TileError> {
        layout.bounds.validate()?;
        Ok(Self {
            layout,
            tiles: BTreeMap::new(),
            pool,
            max_tiles,
        })
    }
    pub fn layout(&self) -> MaskLayout {
        self.layout
    }
    pub fn invert(&mut self) {
        self.layout.inverted = !self.layout.inverted;
    }
    pub fn tile_count(&self) -> usize {
        self.tiles.len()
    }
    pub fn referenced_bytes(&self) -> u64 {
        self.tiles.len() as u64 * BYTES
    }
    pub fn tile_coords(&self) -> impl ExactSizeIterator<Item = &TileCoord> {
        self.tiles.keys()
    }
    pub fn tile_version(&self, coord: TileCoord) -> Option<u64> {
        self.tiles.get(&coord).map(|b| b.version)
    }
    pub fn encoded_tile(&self, coord: TileCoord) -> Result<Option<MaskTile>, TileError> {
        self.tiles.get(&coord).map(|b| b.load()).transpose()
    }
    pub fn tiles(
        &self,
    ) -> impl ExactSizeIterator<Item = (&TileCoord, Result<MaskTile, TileError>)> {
        self.tiles.iter().map(|(c, b)| (c, b.load()))
    }
    pub fn sample(&self, x: i64, y: i64) -> Result<u16, TileError> {
        if !self.layout.bounds.contains(x, y) {
            return Ok(0);
        }
        let (coord, [px, py]) = TileCoord::locate(x, y);
        let encoded = self
            .encoded_tile(coord)?
            .map_or(0, |t| t[usize::from(py) * 256 + usize::from(px)]);
        Ok(self.decode(encoded))
    }
    fn decode(&self, encoded: u16) -> u16 {
        encoded ^ self.layout.default_coverage ^ if self.layout.inverted { u16::MAX } else { 0 }
    }
    pub fn insert_encoded_tile(
        &mut self,
        coord: TileCoord,
        samples: Vec<u16>,
    ) -> Result<(), TileError> {
        if samples.len() != SAMPLES || !self.layout.bounds.intersects_tile(coord) {
            return Err(TileError::InvalidBuffer);
        }
        let [ox, oy] = coord.origin();
        for y in 0..256usize {
            for x in 0..256usize {
                if samples[y * 256 + x] != 0
                    && !self.layout.bounds.contains(ox + x as i64, oy + y as i64)
                {
                    return Err(TileError::InvalidBuffer);
                }
            }
        }
        if samples.iter().all(|v| *v == 0) {
            self.tiles.remove(&coord);
            return Ok(());
        }
        if !self.tiles.contains_key(&coord) && self.tiles.len() >= self.max_tiles {
            return Err(TileError::TileLimit {
                limit: self.max_tiles,
            });
        }
        // Allocate/register before touching the index; budget/backing failures
        // cannot change this snapshot or any undo snapshot sharing its blocks.
        let block = Block::new(&self.pool, samples)?;
        self.tiles.insert(coord, block);
        Ok(())
    }
    pub fn insert_tile(
        &mut self,
        coord: TileCoord,
        mut coverage: Vec<u16>,
    ) -> Result<(), TileError> {
        if coverage.len() != SAMPLES {
            return Err(TileError::InvalidBuffer);
        }
        let [ox, oy] = coord.origin();
        let baseline = self.decode(0);
        for y in 0..256usize {
            for x in 0..256usize {
                let i = y * 256 + x;
                coverage[i] = if self.layout.bounds.contains(ox + x as i64, oy + y as i64) {
                    coverage[i] ^ baseline
                } else {
                    0
                };
            }
        }
        self.insert_encoded_tile(coord, coverage)
    }
    /// Decode a 256×256 region at any origin. At most four source blocks are
    /// pinned; no full mask or image copy is required by the compositor.
    pub fn region_tile(&self, x: i64, y: i64) -> Result<Vec<u16>, TileError> {
        let mut out = allocated(0)?;
        let (first, _) = TileCoord::locate(x, y);
        let (last, _) = TileCoord::locate(
            x.checked_add(255)
                .ok_or(TileError::InvalidCoordinate { x, y })?,
            y.checked_add(255)
                .ok_or(TileError::InvalidCoordinate { x, y })?,
        );
        for tx in first.x()..=last.x() {
            for ty in first.y()..=last.y() {
                let coord = TileCoord::new(tx, ty)?;
                let tile = self.encoded_tile(coord)?;
                let [ox, oy] = coord.origin();
                for dy in 0..256usize {
                    let sy = y + dy as i64;
                    if !(0..256).contains(&(i128::from(sy) - i128::from(oy))) {
                        continue;
                    }
                    for dx in 0..256usize {
                        let sx = x + dx as i64;
                        if !(0..256).contains(&(i128::from(sx) - i128::from(ox)))
                            || !self.layout.bounds.contains(sx, sy)
                        {
                            continue;
                        }
                        let encoded = tile
                            .as_ref()
                            .map_or(0, |t| t[(sy - oy) as usize * 256 + (sx - ox) as usize]);
                        out[dy * 256 + dx] = self.decode(encoded);
                    }
                }
            }
        }
        Ok(out)
    }
}

fn allocated(value: u16) -> Result<Vec<u16>, TileError> {
    let mut out = Vec::new();
    out.try_reserve_exact(SAMPLES)
        .map_err(|_| TileError::AllocationFailed)?;
    out.resize(SAMPLES, value);
    Ok(out)
}
