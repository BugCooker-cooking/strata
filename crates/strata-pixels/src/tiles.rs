//! Sparse integer sample storage. Display mipmaps and global memory accounting
//! belong to consumers; neither may replace these full-depth source samples.

use crate::memory::{DiskSlot, Evictable, MemoryError, MemoryKind, MemoryPool, Reservation};
use crate::{PixelBuffer, PixelFormat};
use std::{
    collections::BTreeMap,
    fmt,
    ops::Deref,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

pub const TILE_EDGE: u32 = 256;
const TILE_SAMPLES: usize = TILE_EDGE as usize * TILE_EDGE as usize * 4;
static NEXT_BLOCK_VERSION: AtomicU64 = AtomicU64::new(1);
pub(crate) fn block_version() -> u64 {
    NEXT_BLOCK_VERSION.fetch_add(1, Ordering::Relaxed)
}
#[derive(Debug)]
struct Block {
    data: Mutex<BlockData>,
    pool: MemoryPool,
    version: u64,
    format: PixelFormat,
    used: AtomicU64,
}
#[derive(Debug)]
struct BlockData {
    resident: Option<Arc<Resident>>,
    backing: Option<DiskSlot>,
}
#[derive(Debug)]
struct Resident {
    samples: PixelBuffer,
    _reservation: Reservation,
}
#[derive(Clone, Debug)]
pub struct TileBuffer(Arc<Resident>);
impl Deref for TileBuffer {
    type Target = PixelBuffer;
    fn deref(&self) -> &PixelBuffer {
        &self.0.samples
    }
}
impl AsRef<PixelBuffer> for TileBuffer {
    fn as_ref(&self) -> &PixelBuffer {
        self
    }
}
fn raw(samples: &PixelBuffer) -> &[u8] {
    match samples {
        PixelBuffer::Rgba8(v) => v,
        PixelBuffer::Rgba16(v) => bytemuck::cast_slice(v),
    }
}
impl Block {
    fn new(pool: &MemoryPool, samples: PixelBuffer) -> Result<Arc<Self>, TileError> {
        let reservation = pool.reserve(MemoryKind::Source, samples.allocation_bytes() as u64)?;
        let block = Arc::new(Self {
            format: samples.format(),
            data: Mutex::new(BlockData {
                resident: Some(Arc::new(Resident {
                    samples,
                    _reservation: reservation,
                })),
                backing: None,
            }),
            pool: pool.clone(),
            version: block_version(),
            used: AtomicU64::new(pool.tick()),
        });
        pool.register(block.clone());
        Ok(block)
    }
    fn load(self: &Arc<Self>) -> Result<TileBuffer, TileError> {
        self.used.store(self.pool.tick(), Ordering::Relaxed);
        let mut data = self.data.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(resident) = &data.resident {
            return Ok(TileBuffer(resident.clone()));
        }
        let reservation = self.pool.reserve(
            MemoryKind::Source,
            (TILE_SAMPLES * self.format.bytes_per_sample()) as u64,
        )?;
        let mut samples = match self.format {
            PixelFormat::Rgba8 => PixelBuffer::Rgba8(zero_samples()?),
            PixelFormat::Rgba16 => PixelBuffer::Rgba16(zero_samples()?),
        };
        let backing = data
            .backing
            .as_ref()
            .expect("nonresident blocks have a backing file");
        let bytes: &mut [u8] = match &mut samples {
            PixelBuffer::Rgba8(v) => v,
            PixelBuffer::Rgba16(v) => bytemuck::cast_slice_mut(v),
        };
        self.pool.reload(backing, bytes)?;
        let resident = Arc::new(Resident {
            samples,
            _reservation: reservation,
        });
        data.resident = Some(resident.clone());
        drop(data);
        self.pool.register(self.clone());
        self.pool.reloaded();
        Ok(TileBuffer(resident))
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
            let bytes = raw(&resident.samples);
            data.backing = Some(self.pool.spill(bytes)?);
        }
        data.resident = None;
        Ok(true)
    }
}

/// Coordinates on the tile grid, with origins representable as i64 pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TileCoord {
    x: i64,
    y: i64,
}

impl TileCoord {
    pub fn new(x: i64, y: i64) -> Result<Self, TileError> {
        let range = (i64::MIN / i64::from(TILE_EDGE))..=(i64::MAX / i64::from(TILE_EDGE));
        if !range.contains(&x) || !range.contains(&y) {
            return Err(TileError::InvalidCoordinate { x, y });
        }
        Ok(Self { x, y })
    }

    /// Find the containing block and local offset, including negative pixels.
    pub fn locate(x: i64, y: i64) -> (Self, [u8; 2]) {
        let edge = i64::from(TILE_EDGE);
        (
            Self {
                x: x.div_euclid(edge),
                y: y.div_euclid(edge),
            },
            [x.rem_euclid(edge) as u8, y.rem_euclid(edge) as u8],
        )
    }

    pub fn x(self) -> i64 {
        self.x
    }

    pub fn y(self) -> i64 {
        self.y
    }

    pub fn origin(self) -> [i64; 2] {
        [self.x * i64::from(TILE_EDGE), self.y * i64::from(TILE_EDGE)]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PixelValue {
    Rgba8([u8; 4]),
    Rgba16([u16; 4]),
}

impl PixelValue {
    pub fn format(self) -> PixelFormat {
        match self {
            Self::Rgba8(_) => PixelFormat::Rgba8,
            Self::Rgba16(_) => PixelFormat::Rgba16,
        }
    }

    pub fn zero(format: PixelFormat) -> Self {
        match format {
            PixelFormat::Rgba8 => Self::Rgba8([0; 4]),
            PixelFormat::Rgba16 => Self::Rgba16([0; 4]),
        }
    }

    fn is_zero(self) -> bool {
        self == Self::zero(self.format())
    }
}

/// Cloning copies the index and shares immutable pixel blocks. Writes detach
/// only the touched block. This is a pixel container, not a document API:
/// editable documents must keep it private behind the command executor.
///
/// `max_tiles` caps blocks referenced by THIS snapshot. It does not account for
/// old snapshots, temporary copies, index overhead or GPU caches. Consumers
/// must add global reservations/eviction before enabling large working sets.
#[derive(Clone, Debug)]
pub struct TiledPixels {
    format: PixelFormat,
    max_tiles: usize,
    tiles: BTreeMap<TileCoord, Arc<Block>>,
    pool: MemoryPool,
}

impl TiledPixels {
    /// Validated bulk import. Padding remains zero; hidden RGB is preserved.
    pub fn from_dense(
        width: u32,
        height: u32,
        source: &PixelBuffer,
        max_tiles: usize,
    ) -> Result<Self, TileError> {
        Self::from_dense_with_pool(width, height, source, max_tiles, MemoryPool::global())
    }
    pub fn from_dense_with_pool(
        width: u32,
        height: u32,
        source: &PixelBuffer,
        max_tiles: usize,
        pool: MemoryPool,
    ) -> Result<Self, TileError> {
        source
            .validate(width, height)
            .map_err(|_| TileError::InvalidBuffer)?;
        let mut result = Self::with_pool(source.format(), max_tiles, pool);
        for ty in 0..height.div_ceil(TILE_EDGE) {
            for tx in 0..width.div_ceil(TILE_EDGE) {
                let mut block = match source.format() {
                    PixelFormat::Rgba8 => PixelBuffer::Rgba8(zero_samples()?),
                    PixelFormat::Rgba16 => PixelBuffer::Rgba16(zero_samples()?),
                };
                let rows = TILE_EDGE.min(height - ty * TILE_EDGE) as usize;
                let cols = TILE_EDGE.min(width - tx * TILE_EDGE) as usize * 4;
                for row in 0..rows {
                    let src = (((ty * TILE_EDGE) as usize + row) * width as usize
                        + (tx * TILE_EDGE) as usize)
                        * 4;
                    let dst = row * TILE_EDGE as usize * 4;
                    match (&mut block, source) {
                        (PixelBuffer::Rgba8(out), PixelBuffer::Rgba8(input)) => {
                            out[dst..dst + cols].copy_from_slice(&input[src..src + cols])
                        }
                        (PixelBuffer::Rgba16(out), PixelBuffer::Rgba16(input)) => {
                            out[dst..dst + cols].copy_from_slice(&input[src..src + cols])
                        }
                        _ => unreachable!(),
                    }
                }
                result.insert_tile(TileCoord::new(i64::from(tx), i64::from(ty))?, block)?;
            }
        }
        Ok(result)
    }

    /// Replace a complete block after validation. Only all-RGBA-zero blocks vanish.
    pub fn insert_tile(&mut self, coord: TileCoord, block: PixelBuffer) -> Result<(), TileError> {
        if block.format() != self.format {
            return Err(TileError::FormatMismatch {
                expected: self.format,
                actual: block.format(),
            });
        }
        block
            .validate(TILE_EDGE, TILE_EDGE)
            .map_err(|_| TileError::InvalidBuffer)?;
        if all_zero(&block) {
            self.tiles.remove(&coord);
            return Ok(());
        }
        if !self.tiles.contains_key(&coord) && self.tiles.len() >= self.max_tiles {
            return Err(TileError::TileLimit {
                limit: self.max_tiles,
            });
        }
        let block = Block::new(&self.pool, block)?;
        self.tiles.insert(coord, block);
        Ok(())
    }
    pub fn new(format: PixelFormat, max_tiles: usize) -> Self {
        Self::with_pool(format, max_tiles, MemoryPool::global())
    }
    pub fn with_pool(format: PixelFormat, max_tiles: usize, pool: MemoryPool) -> Self {
        Self {
            format,
            max_tiles,
            tiles: BTreeMap::new(),
            pool,
        }
    }
    pub fn memory_pool(&self) -> &MemoryPool {
        &self.pool
    }

    pub fn format(&self) -> PixelFormat {
        self.format
    }

    pub fn tile_count(&self) -> usize {
        self.tiles.len()
    }

    /// Sample bytes referenced by this instance, not unique process allocation.
    pub fn referenced_bytes(&self) -> u64 {
        self.tiles.len() as u64 * TILE_SAMPLES as u64 * self.format.bytes_per_sample() as u64
    }

    pub fn tile(&self, coord: TileCoord) -> Result<Option<TileBuffer>, TileError> {
        self.tiles.get(&coord).map(|b| b.load()).transpose()
    }
    pub fn contains_tile(&self, coord: TileCoord) -> bool {
        self.tiles.contains_key(&coord)
    }
    pub fn tile_coords(&self) -> impl ExactSizeIterator<Item = &TileCoord> {
        self.tiles.keys()
    }
    /// Process-local identity changes on every real write and cannot be confused
    /// by allocator address reuse. Shared immutable snapshots keep the version.
    pub fn tile_version(&self, coord: TileCoord) -> Option<u64> {
        self.tiles.get(&coord).map(|b| b.version)
    }

    /// Deterministic x-then-y grid order; samples inside each tile are row-major.
    pub fn tiles(
        &self,
    ) -> impl ExactSizeIterator<Item = (&TileCoord, Result<TileBuffer, TileError>)> {
        self.tiles.iter().map(|(coord, data)| (coord, data.load()))
    }

    pub fn pixel(&self, x: i64, y: i64) -> Result<PixelValue, TileError> {
        let (coord, offset) = TileCoord::locate(x, y);
        Ok(self
            .tile(coord)?
            .map_or(PixelValue::zero(self.format), |tile| {
                read_pixel(&tile, sample_index(offset))
            }))
    }

    /// Returns whether samples changed. Validation, capacity and pixel-buffer
    /// allocation errors leave this instance and its snapshots unchanged.
    pub fn set_pixel(&mut self, x: i64, y: i64, value: PixelValue) -> Result<bool, TileError> {
        if value.format() != self.format {
            return Err(TileError::FormatMismatch {
                expected: self.format,
                actual: value.format(),
            });
        }
        if self.pixel(x, y)? == value {
            return Ok(false);
        }
        let (coord, offset) = TileCoord::locate(x, y);
        let mut pixels = if let Some(tile) = self.tile(coord)? {
            match &*tile {
                PixelBuffer::Rgba8(samples) => PixelBuffer::Rgba8(copy_samples(samples)?),
                PixelBuffer::Rgba16(samples) => PixelBuffer::Rgba16(copy_samples(samples)?),
            }
        } else {
            if self.tiles.len() >= self.max_tiles {
                return Err(TileError::TileLimit {
                    limit: self.max_tiles,
                });
            }
            match self.format {
                PixelFormat::Rgba8 => PixelBuffer::Rgba8(zero_samples()?),
                PixelFormat::Rgba16 => PixelBuffer::Rgba16(zero_samples()?),
            }
        };
        let i = sample_index(offset);
        match (&mut pixels, value) {
            (PixelBuffer::Rgba8(samples), PixelValue::Rgba8(pixel)) => {
                samples[i..i + 4].copy_from_slice(&pixel)
            }
            (PixelBuffer::Rgba16(samples), PixelValue::Rgba16(pixel)) => {
                samples[i..i + 4].copy_from_slice(&pixel)
            }
            _ => unreachable!("format checked before write"),
        }
        // Only a write of four zeros can make the whole tile removable.
        // Transparent pixels with hidden RGB are still document data.
        if value.is_zero() && all_zero(&pixels) {
            self.tiles.remove(&coord);
        } else {
            self.insert_tile(coord, pixels)?;
        }
        Ok(true)
    }
}

fn sample_index([x, y]: [u8; 2]) -> usize {
    (usize::from(y) * TILE_EDGE as usize + usize::from(x)) * 4
}

fn read_pixel(tile: &PixelBuffer, i: usize) -> PixelValue {
    match tile {
        PixelBuffer::Rgba8(v) => PixelValue::Rgba8([v[i], v[i + 1], v[i + 2], v[i + 3]]),
        PixelBuffer::Rgba16(v) => PixelValue::Rgba16([v[i], v[i + 1], v[i + 2], v[i + 3]]),
    }
}

fn all_zero(tile: &PixelBuffer) -> bool {
    match tile {
        PixelBuffer::Rgba8(v) => v.iter().all(|&v| v == 0),
        PixelBuffer::Rgba16(v) => v.iter().all(|&v| v == 0),
    }
}

fn zero_samples<T: Default + Clone>() -> Result<Vec<T>, TileError> {
    let mut samples = Vec::new();
    samples
        .try_reserve_exact(TILE_SAMPLES)
        .map_err(|_| TileError::AllocationFailed)?;
    samples.resize(TILE_SAMPLES, T::default());
    Ok(samples)
}

fn copy_samples<T: Copy>(source: &[T]) -> Result<Vec<T>, TileError> {
    let mut samples = Vec::new();
    samples
        .try_reserve_exact(source.len())
        .map_err(|_| TileError::AllocationFailed)?;
    samples.extend_from_slice(source);
    Ok(samples)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TileError {
    Memory(String),
    InvalidBuffer,
    InvalidCoordinate {
        x: i64,
        y: i64,
    },
    FormatMismatch {
        expected: PixelFormat,
        actual: PixelFormat,
    },
    TileLimit {
        limit: usize,
    },
    AllocationFailed,
}

impl fmt::Display for TileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Memory(e) => write!(f, "{e}"),
            Self::InvalidBuffer => write!(f, "invalid tile or image sample count"),
            Self::InvalidCoordinate { x, y } => {
                write!(f, "tile ({x}, {y}) exceeds pixel coordinate range")
            }
            Self::FormatMismatch { expected, actual } => {
                write!(f, "expected {expected:?} pixel, received {actual:?}")
            }
            Self::TileLimit { limit } => write!(f, "tile limit ({limit}) reached"),
            Self::AllocationFailed => write!(f, "cannot allocate tile samples"),
        }
    }
}

impl std::error::Error for TileError {}
impl From<MemoryError> for TileError {
    fn from(e: MemoryError) -> Self {
        Self::Memory(e.to_string())
    }
}
