//! Bounded, bit-depth-preserving image decoding.

use image::{ColorType, DynamicImage, ImageDecoder, ImageError, ImageReader, Limits};
use std::{
    fs::File,
    io::BufReader,
    path::{Path, PathBuf},
};
use strata_pixels::{PixelBuffer, PixelFormat};

/// Maximum final RGBA buffer size. Decoder scratch/metadata are also limited where supported.
pub const DEFAULT_DECODE_LIMIT: u64 = 512 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq)]
pub struct DecodedImage {
    pub width: u32,
    pub height: u32,
    pub pixels: PixelBuffer,
    pub icc_profile: Option<Vec<u8>>,
}

#[derive(Debug, thiserror::Error)]
pub enum IoError {
    #[error("file not found: {0}")]
    NotFound(PathBuf),
    #[error("unsupported image {path}: {reason}")]
    Unsupported { path: PathBuf, reason: String },
    #[error("cannot decode {path}: {reason}")]
    Decode { path: PathBuf, reason: String },
    #[error("image exceeds decode limit ({limit} bytes): {path}")]
    ResourceLimit { path: PathBuf, limit: u64 },
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub fn decode_image(path: &Path) -> Result<DecodedImage, IoError> {
    decode_image_with_limit(path, DEFAULT_DECODE_LIMIT)
}

/// The limit applies to the expanded RGBA buffer, not just the source channel count.
/// Some third-party codecs only support best-effort scratch allocation limits.
pub fn decode_image_with_limit(path: &Path, limit: u64) -> Result<DecodedImage, IoError> {
    let file = File::open(path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            IoError::NotFound(path.to_owned())
        } else {
            IoError::Io(e)
        }
    })?;
    // Start with no extension hint. with_guessed_format otherwise retains a false .jpg hint.
    let mut reader = ImageReader::new(BufReader::new(file)).with_guessed_format()?;
    if reader.format().is_none() {
        return Err(IoError::Unsupported {
            path: path.to_owned(),
            reason: "unrecognized file signature".into(),
        });
    }
    let resource_error = || IoError::ResourceLimit {
        path: path.to_owned(),
        limit,
    };
    let map_error = |error: ImageError| match error {
        ImageError::Limits(_) => resource_error(),
        ImageError::Unsupported(e) => IoError::Unsupported {
            path: path.to_owned(),
            reason: e.to_string(),
        },
        e => IoError::Decode {
            path: path.to_owned(),
            reason: e.to_string(),
        },
    };
    let mut limits = Limits::default();
    limits.max_alloc = Some(limit);
    // A single pixel is at least four output bytes; this also bounds header dimensions.
    let max_dimension = (limit / 4).min(u64::from(u32::MAX)) as u32;
    limits.max_image_width = Some(max_dimension);
    limits.max_image_height = Some(max_dimension);
    reader.limits(limits);
    let mut decoder = reader.into_decoder().map_err(&map_error)?;
    let (width, height) = decoder.dimensions();
    let format = match decoder.color_type() {
        ColorType::L8 | ColorType::La8 | ColorType::Rgb8 | ColorType::Rgba8 => PixelFormat::Rgba8,
        ColorType::L16 | ColorType::La16 | ColorType::Rgb16 | ColorType::Rgba16 => {
            PixelFormat::Rgba16
        }
        other => {
            return Err(IoError::Unsupported {
                path: path.to_owned(),
                reason: format!("unsupported pixel format {other:?}"),
            });
        }
    };
    let bytes = format
        .byte_count(width, height)
        .map_err(|_| resource_error())?;
    if bytes as u64 > limit {
        return Err(resource_error());
    }
    let icc_profile = decoder.icc_profile().map_err(&map_error)?;
    if icc_profile
        .as_ref()
        .is_some_and(|icc| icc.len() as u64 > limit)
    {
        return Err(resource_error());
    }
    let orientation = decoder.orientation().map_err(&map_error)?;
    let mut decoded = DynamicImage::from_decoder(decoder).map_err(&map_error)?;
    decoded.apply_orientation(orientation);
    let (width, height) = (decoded.width(), decoded.height());
    let pixels = match format {
        PixelFormat::Rgba8 => PixelBuffer::Rgba8(decoded.into_rgba8().into_raw()),
        PixelFormat::Rgba16 => PixelBuffer::Rgba16(decoded.into_rgba16().into_raw()),
    };
    pixels
        .validate(width, height)
        .map_err(|e| IoError::Decode {
            path: path.to_owned(),
            reason: e.to_string(),
        })?;
    Ok(DecodedImage {
        width,
        height,
        pixels,
        icc_profile,
    })
}
