use crate::project::{StorageError, sync_parent};
use image::{ExtendedColorType, ImageEncoder};
use std::{fs, path::Path};
use strata_pixels::PixelBuffer;

#[derive(Clone, Copy, Debug)]
pub enum ExportFormat {
    Png,
    Tiff,
    Jpeg { quality: u8 },
    WebP,
}

pub fn export_image(
    path: &Path,
    format: ExportFormat,
    width: u32,
    height: u32,
    pixels: &PixelBuffer,
    icc: &[u8],
    overwrite: bool,
) -> Result<(), StorageError> {
    pixels
        .validate(width, height)
        .map_err(|e| StorageError::Invalid(e.to_string()))?;
    if pixels.byte_len() as u64 > crate::DEFAULT_DECODE_LIMIT || icc.len() > 4 * 1024 * 1024 {
        return Err(StorageError::Invalid("export resource limit".into()));
    }
    if matches!(
        format,
        ExportFormat::Jpeg {
            quality: 0 | 101..=255
        }
    ) {
        return Err(StorageError::Invalid("JPEG quality must be 1..100".into()));
    }
    if matches!(format, ExportFormat::Jpeg { .. } | ExportFormat::WebP)
        && !matches!(pixels, PixelBuffer::Rgba8(_))
    {
        return Err(StorageError::Invalid(
            "JPEG/WebP require explicitly quantized 8-bit input".into(),
        ));
    }
    if matches!(format, ExportFormat::Jpeg { .. })
        && matches!(pixels, PixelBuffer::Rgba8(v) if v.chunks_exact(4).any(|p| p[3] != 255))
    {
        return Err(StorageError::Invalid(
            "JPEG input must already be flattened".into(),
        ));
    }
    if path.exists() && !overwrite {
        return Err(StorageError::Exists);
    }
    if path.exists() && fs::symlink_metadata(path)?.file_type().is_symlink() {
        return Err(StorageError::Invalid("symlink target".into()));
    }
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    let bytes;
    let (data, color) = match pixels {
        PixelBuffer::Rgba8(v) => (v.as_slice(), ExtendedColorType::Rgba8),
        PixelBuffer::Rgba16(v) => {
            bytes = v.iter().flat_map(|v| v.to_ne_bytes()).collect::<Vec<_>>();
            (bytes.as_slice(), ExtendedColorType::Rgba16)
        }
    };
    let error = |e: image::ImageError| StorageError::Invalid(e.to_string());
    match format {
        ExportFormat::Png => {
            let mut encoder = image::codecs::png::PngEncoder::new(temp.as_file_mut());
            encoder
                .set_icc_profile(icc.to_vec())
                .map_err(|e| StorageError::Invalid(e.to_string()))?;
            encoder
                .write_image(data, width, height, color)
                .map_err(error)?;
        }
        ExportFormat::Tiff => {
            let mut encoder = image::codecs::tiff::TiffEncoder::new(temp.as_file_mut());
            encoder
                .set_icc_profile(icc.to_vec())
                .map_err(|e| StorageError::Invalid(e.to_string()))?;
            encoder
                .write_image(data, width, height, color)
                .map_err(error)?;
        }
        ExportFormat::WebP => {
            let mut encoder = image::codecs::webp::WebPEncoder::new_lossless(temp.as_file_mut());
            encoder
                .set_icc_profile(icc.to_vec())
                .map_err(|e| StorageError::Invalid(e.to_string()))?;
            encoder
                .write_image(data, width, height, color)
                .map_err(error)?;
        }
        ExportFormat::Jpeg { quality } => {
            let rgb: Vec<u8> = data
                .chunks_exact(4)
                .flat_map(|p| p[..3].iter().copied())
                .collect();
            let mut encoder =
                image::codecs::jpeg::JpegEncoder::new_with_quality(temp.as_file_mut(), quality);
            encoder
                .set_icc_profile(icc.to_vec())
                .map_err(|e| StorageError::Invalid(e.to_string()))?;
            encoder
                .write_image(&rgb, width, height, ExtendedColorType::Rgb8)
                .map_err(error)?;
        }
    }
    temp.as_file().sync_all()?;
    let decoded =
        crate::decode_image(temp.path()).map_err(|e| StorageError::Invalid(e.to_string()))?;
    // Lossless outputs must be sample-exact. JPEG is intentionally lossy, but
    // dimensions, output depth/alpha and the embedded ICC are still verified.
    let samples_valid = if matches!(format, ExportFormat::Jpeg { .. }) {
        matches!(&decoded.pixels, PixelBuffer::Rgba8(v) if v.chunks_exact(4).all(|p| p[3] == 255))
    } else {
        decoded.pixels == *pixels
    };
    if !samples_valid
        || decoded.width != width
        || decoded.height != height
        || decoded.icc_profile.as_deref() != Some(icc)
    {
        return Err(StorageError::Invalid(format!(
            "export verification failed: format={format:?}, pixels_equal={}, profile_equal={}, decoded_profile_bytes={}",
            decoded.pixels == *pixels,
            decoded.icc_profile.as_deref() == Some(icc),
            decoded.icc_profile.as_ref().map_or(0, Vec::len)
        )));
    }
    if overwrite {
        temp.persist(path).map_err(|e| e.error)?;
    } else {
        temp.persist_noclobber(path).map_err(|e| e.error)?;
    }
    sync_parent(path).map_err(|e| StorageError::PublishedUncertain(e.to_string()))
}
