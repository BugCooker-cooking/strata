//! Platform-independent, straight-alpha RGBA sample buffers.

use std::fmt;

mod mask;
pub mod memory;
mod tiles;
pub use mask::{CoverageRect, MaskLayout, MaskTile, TiledMask};
pub use tiles::{PixelValue, TILE_EDGE, TileBuffer, TileCoord, TileError, TiledPixels};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PixelFormat {
    Rgba8,
    Rgba16,
}

impl PixelFormat {
    pub fn bit_depth(self) -> u8 {
        match self {
            Self::Rgba8 => 8,
            Self::Rgba16 => 16,
        }
    }

    pub fn bytes_per_sample(self) -> usize {
        usize::from(self.bit_depth() / 8)
    }

    /// Validate dimensions before allocating. Counts are interleaved RGBA samples.
    pub fn sample_count(self, width: u32, height: u32) -> Result<usize, PixelError> {
        if width == 0 || height == 0 {
            return Err(PixelError::ZeroDimensions);
        }
        let samples = usize::try_from(width)
            .ok()
            .and_then(|w| w.checked_mul(height as usize))
            .and_then(|area| area.checked_mul(4))
            .ok_or(PixelError::SizeOverflow)?;
        let bytes = samples
            .checked_mul(self.bytes_per_sample())
            .ok_or(PixelError::SizeOverflow)?;
        if bytes > isize::MAX as usize {
            return Err(PixelError::SizeOverflow);
        }
        Ok(samples)
    }

    pub fn byte_count(self, width: u32, height: u32) -> Result<usize, PixelError> {
        Ok(self.sample_count(width, height)? * self.bytes_per_sample())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PixelBuffer {
    Rgba8(Vec<u8>),
    Rgba16(Vec<u16>),
}

impl PixelBuffer {
    pub fn format(&self) -> PixelFormat {
        match self {
            Self::Rgba8(_) => PixelFormat::Rgba8,
            Self::Rgba16(_) => PixelFormat::Rgba16,
        }
    }

    pub fn sample_len(&self) -> usize {
        match self {
            Self::Rgba8(v) => v.len(),
            Self::Rgba16(v) => v.len(),
        }
    }

    pub fn byte_len(&self) -> usize {
        self.sample_len() * self.format().bytes_per_sample()
    }
    pub fn allocation_bytes(&self) -> usize {
        match self {
            Self::Rgba8(v) => v.capacity(),
            Self::Rgba16(v) => v.capacity() * 2,
        }
    }

    pub fn validate(&self, width: u32, height: u32) -> Result<(), PixelError> {
        let expected = self.format().sample_count(width, height)?;
        let actual = self.sample_len();
        if expected != actual {
            return Err(PixelError::LengthMismatch { expected, actual });
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PixelError {
    ZeroDimensions,
    SizeOverflow,
    LengthMismatch { expected: usize, actual: usize },
}

impl fmt::Display for PixelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroDimensions => write!(f, "image dimensions must be nonzero"),
            Self::SizeOverflow => write!(f, "image dimensions exceed addressable memory"),
            Self::LengthMismatch { expected, actual } => {
                write!(f, "expected {expected} RGBA samples, received {actual}")
            }
        }
    }
}

impl std::error::Error for PixelError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgba8_reports_format_and_lengths() {
        let pixels = PixelBuffer::Rgba8(vec![1, 2, 3, 4]);
        assert_eq!(pixels.format(), PixelFormat::Rgba8);
        assert_eq!(pixels.sample_len(), 4);
        assert_eq!(pixels.byte_len(), 4);
        assert_eq!(pixels.validate(1, 1), Ok(()));
    }

    #[test]
    fn rgba16_preserves_adjacent_codes_and_reports_byte_length() {
        let pixels = PixelBuffer::Rgba16(vec![32768, 32769, 50001, 65535]);
        assert_eq!(pixels.format(), PixelFormat::Rgba16);
        assert_eq!(pixels.sample_len(), 4);
        assert_eq!(pixels.byte_len(), 8);
        assert_eq!(pixels.validate(1, 1), Ok(()));
        assert_eq!(pixels.clone(), pixels);
        let PixelBuffer::Rgba16(samples) = pixels else {
            panic!("wrong format")
        };
        assert_eq!(samples, [32768, 32769, 50001, 65535]);
    }

    #[test]
    fn zero_dimensions_are_rejected() {
        for (w, h) in [(0, 0), (0, 1), (1, 0)] {
            assert_eq!(
                PixelBuffer::Rgba8(vec![]).validate(w, h),
                Err(PixelError::ZeroDimensions)
            );
        }
    }

    #[test]
    fn dimension_overflow_is_rejected_before_length_check() {
        assert_eq!(
            PixelBuffer::Rgba16(vec![]).validate(u32::MAX, u32::MAX),
            Err(PixelError::SizeOverflow)
        );
    }

    #[test]
    fn mismatch_reports_samples_not_bytes() {
        assert_eq!(
            PixelBuffer::Rgba16(vec![0; 7]).validate(1, 2),
            Err(PixelError::LengthMismatch {
                expected: 8,
                actual: 7
            })
        );
    }
}
