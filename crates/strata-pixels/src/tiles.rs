//! Sparse integer sample storage. Display mipmaps and global memory accounting
//! belong to consumers; neither may replace these full-depth source samples.

use crate::{PixelBuffer, PixelFormat};
use std::{
    collections::BTreeMap,
    fmt,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

pub const TILE_EDGE: u32 = 256;
const TILE_SAMPLES: usize = TILE_EDGE as usize * TILE_EDGE as usize * 4;
static NEXT_BLOCK_VERSION: AtomicU64 = AtomicU64::new(1);
fn block_version() -> u64 {
    NEXT_BLOCK_VERSION.fetch_add(1, Ordering::Relaxed)
}
#[derive(Debug)]
struct Block {
    samples: PixelBuffer,
    version: u64,
}
impl Block {
    fn new(samples: PixelBuffer) -> Self {
        Self {
            samples,
            version: block_version(),
        }
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
}

impl TiledPixels {
    /// Validated bulk import. Padding remains zero; hidden RGB is preserved.
    pub fn from_dense(
        width: u32,
        height: u32,
        source: &PixelBuffer,
        max_tiles: usize,
    ) -> Result<Self, TileError> {
        source
            .validate(width, height)
            .map_err(|_| TileError::InvalidBuffer)?;
        let mut result = Self::new(source.format(), max_tiles);
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
        self.tiles.insert(coord, Arc::new(Block::new(block)));
        Ok(())
    }
    pub fn new(format: PixelFormat, max_tiles: usize) -> Self {
        Self {
            format,
            max_tiles,
            tiles: BTreeMap::new(),
        }
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

    pub fn tile(&self, coord: TileCoord) -> Option<&PixelBuffer> {
        self.tiles.get(&coord).map(|b| &b.samples)
    }
    /// Process-local identity changes on every real write and cannot be confused
    /// by allocator address reuse. Shared immutable snapshots keep the version.
    pub fn tile_version(&self, coord: TileCoord) -> Option<u64> {
        self.tiles.get(&coord).map(|b| b.version)
    }

    /// Deterministic x-then-y grid order; samples inside each tile are row-major.
    pub fn tiles(&self) -> impl ExactSizeIterator<Item = (&TileCoord, &PixelBuffer)> {
        self.tiles
            .iter()
            .map(|(coord, data)| (coord, &data.samples))
    }

    pub fn pixel(&self, x: i64, y: i64) -> PixelValue {
        let (coord, offset) = TileCoord::locate(x, y);
        self.tile(coord)
            .map_or(PixelValue::zero(self.format), |tile| {
                read_pixel(tile, sample_index(offset))
            })
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
        if self.pixel(x, y) == value {
            return Ok(false);
        }
        let (coord, offset) = TileCoord::locate(x, y);
        if !self.tiles.contains_key(&coord) {
            if self.tiles.len() >= self.max_tiles {
                return Err(TileError::TileLimit {
                    limit: self.max_tiles,
                });
            }
            let pixels = match self.format {
                PixelFormat::Rgba8 => PixelBuffer::Rgba8(zero_samples()?),
                PixelFormat::Rgba16 => PixelBuffer::Rgba16(zero_samples()?),
            };
            self.tiles.insert(coord, Arc::new(Block::new(pixels)));
        }
        let shared = self.tiles.get_mut(&coord).expect("tile was inserted");
        // Allocate the copy before replacing the shared block. A failed pixel
        // allocation cannot partially mutate a historical version.
        if Arc::get_mut(shared).is_none() {
            let copy = match &shared.samples {
                PixelBuffer::Rgba8(samples) => PixelBuffer::Rgba8(copy_samples(samples)?),
                PixelBuffer::Rgba16(samples) => PixelBuffer::Rgba16(copy_samples(samples)?),
            };
            *shared = Arc::new(Block::new(copy));
        }
        let tile = Arc::get_mut(shared).expect("private block has no external Arc or Weak handles");
        tile.version = block_version();
        let i = sample_index(offset);
        match (&mut tile.samples, value) {
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
        if value.is_zero() && all_zero(&tile.samples) {
            self.tiles.remove(&coord);
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TileError {
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
