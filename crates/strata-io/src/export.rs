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
    WebPLossy { quality: u8 },
}

#[derive(Clone, Copy, Debug)]
pub struct ExportOptions {
    pub format: ExportFormat,
    pub resolution_dpi: u32,
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
    export_with_options(
        path,
        ExportOptions {
            format,
            resolution_dpi: 72,
        },
        width,
        height,
        pixels,
        icc,
        overwrite,
    )
}

pub fn export_with_options(
    path: &Path,
    options: ExportOptions,
    width: u32,
    height: u32,
    pixels: &PixelBuffer,
    icc: &[u8],
    overwrite: bool,
) -> Result<(), StorageError> {
    let format = options.format;
    if !(1..=2400).contains(&options.resolution_dpi) {
        return Err(StorageError::Invalid(
            "resolution_dpi must be 1..2400".into(),
        ));
    }
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
        } | ExportFormat::WebPLossy {
            quality: 0 | 101..=255
        }
    ) {
        return Err(StorageError::Invalid(
            "JPEG/WebP quality must be 1..100".into(),
        ));
    }
    if matches!(
        format,
        ExportFormat::Jpeg { .. } | ExportFormat::WebP | ExportFormat::WebPLossy { .. }
    ) && !matches!(pixels, PixelBuffer::Rgba8(_))
    {
        return Err(StorageError::Invalid(
            "JPEG/WebP require explicitly quantized 8-bit input".into(),
        ));
    }
    if matches!(format, ExportFormat::WebP | ExportFormat::WebPLossy { .. })
        && (width > 16383 || height > 16383)
    {
        return Err(StorageError::Invalid(
            "WebP dimensions must be at most 16383 pixels".into(),
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
    let data = match pixels {
        PixelBuffer::Rgba8(v) => v.as_slice(),
        PixelBuffer::Rgba16(_) => &[],
    };
    let color = ExtendedColorType::Rgba8;
    let error = |e: image::ImageError| StorageError::Invalid(e.to_string());
    match format {
        ExportFormat::Png => {
            let mut info = png::Info::with_size(width, height);
            info.color_type = png::ColorType::Rgba;
            info.bit_depth = if matches!(pixels, PixelBuffer::Rgba16(_)) {
                png::BitDepth::Sixteen
            } else {
                png::BitDepth::Eight
            };
            info.icc_profile = Some(std::borrow::Cow::Borrowed(icc));
            let ppm = (f64::from(options.resolution_dpi) / 0.0254).round() as u32;
            info.pixel_dims = Some(png::PixelDimensions {
                xppu: ppm,
                yppu: ppm,
                unit: png::Unit::Meter,
            });
            let encoder = png::Encoder::with_info(temp.as_file_mut(), info)
                .map_err(|e| StorageError::Invalid(e.to_string()))?;
            let mut writer = encoder
                .write_header()
                .map_err(|e| StorageError::Invalid(e.to_string()))?;
            match pixels {
                PixelBuffer::Rgba8(v) => writer.write_image_data(v),
                PixelBuffer::Rgba16(v) => writer
                    .write_image_data(&v.iter().flat_map(|s| s.to_be_bytes()).collect::<Vec<_>>()),
            }
            .map_err(|e| StorageError::Invalid(e.to_string()))?;
            writer
                .finish()
                .map_err(|e| StorageError::Invalid(e.to_string()))?;
        }
        ExportFormat::Tiff => {
            use tiff::{
                encoder::{
                    Rational, TiffEncoder,
                    colortype::{RGBA8, RGBA16},
                },
                tags::{ResolutionUnit, Tag},
            };
            let mut encoder = TiffEncoder::new(temp.as_file_mut())
                .map_err(|e| StorageError::Invalid(e.to_string()))?;
            macro_rules! write {
                ($kind:ty,$v:expr) => {{
                    let mut image = encoder
                        .new_image::<$kind>(width, height)
                        .map_err(|e| StorageError::Invalid(e.to_string()))?;
                    image
                        .encoder()
                        .write_tag(Tag::ResolutionUnit, ResolutionUnit::Inch.to_u16())
                        .map_err(|e| StorageError::Invalid(e.to_string()))?;
                    for tag in [Tag::XResolution, Tag::YResolution] {
                        image
                            .encoder()
                            .write_tag(
                                tag,
                                Rational {
                                    n: options.resolution_dpi,
                                    d: 1,
                                },
                            )
                            .map_err(|e| StorageError::Invalid(e.to_string()))?;
                    }
                    image
                        .encoder()
                        .write_tag(Tag::IccProfile, icc)
                        .map_err(|e| StorageError::Invalid(e.to_string()))?;
                    image
                        .write_data($v)
                        .map_err(|e| StorageError::Invalid(e.to_string()))?;
                }};
            }
            match pixels {
                PixelBuffer::Rgba8(v) => write!(RGBA8, v),
                PixelBuffer::Rgba16(v) => write!(RGBA16, v),
            }
        }
        ExportFormat::WebP => {
            let mut encoder = image::codecs::webp::WebPEncoder::new_lossless(temp.as_file_mut());
            encoder
                .set_icc_profile(icc.to_vec())
                .map_err(|e| StorageError::Invalid(e.to_string()))?;
            encoder
                .set_exif_metadata(crate::metadata::exif(options.resolution_dpi))
                .map_err(|e| StorageError::Invalid(e.to_string()))?;
            encoder
                .write_image(data, width, height, color)
                .map_err(error)?;
        }
        ExportFormat::WebPLossy { quality } => {
            crate::webp_lossy::encode(
                temp.as_file_mut(),
                data,
                width,
                height,
                quality,
                icc,
                options.resolution_dpi,
            )?;
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
            encoder.set_pixel_density(image::codecs::jpeg::PixelDensity::dpi(
                options.resolution_dpi as u16,
            ));
            encoder
                .set_exif_metadata(crate::metadata::exif(options.resolution_dpi))
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
    } else if matches!(format, ExportFormat::WebPLossy { .. }) {
        matches!((&decoded.pixels,pixels),(PixelBuffer::Rgba8(a),PixelBuffer::Rgba8(b)) if a.chunks_exact(4).zip(b.chunks_exact(4)).all(|(a,b)|a[3]==b[3]))
    } else {
        decoded.pixels == *pixels
    };
    if !samples_valid
        || decoded.resolution_dpi != options.resolution_dpi
        || decoded.width != width
        || decoded.height != height
        || decoded.icc_profile.as_deref() != Some(icc)
    {
        return Err(StorageError::Invalid(format!(
            "export verification failed: format={format:?}, pixels_equal={}, profile_equal={}, decoded_profile_bytes={}, dpi={}/{}",
            decoded.pixels == *pixels,
            decoded.icc_profile.as_deref() == Some(icc),
            decoded.icc_profile.as_ref().map_or(0, Vec::len),
            decoded.resolution_dpi,
            options.resolution_dpi
        )));
    }
    publish(temp, path, overwrite)
}

/// Verify the editable directory and the staged file before atomically publishing.
pub fn publish_psd(
    path: &Path,
    bytes: &[u8],
    overwrite: bool,
    validate_target: impl FnOnce() -> Result<(), StorageError>,
) -> Result<(), StorageError> {
    use std::io::{Read, Seek, Write};
    crate::psd::Psd::parse(bytes, i16::MAX as usize)
        .map_err(|e| StorageError::Invalid(e.to_string()))?;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    temp.write_all(bytes)?;
    temp.as_file().sync_all()?;
    temp.rewind()?;
    let mut scratch = vec![0; 64 * 1024];
    for expected in bytes.chunks(scratch.len()) {
        let actual = &mut scratch[..expected.len()];
        temp.read_exact(actual)?;
        if actual != expected {
            return Err(StorageError::Invalid(
                "PSD temporary file verification failed".into(),
            ));
        }
    }
    validate_target()?;
    publish(temp, path, overwrite)
}

fn publish(
    temp: tempfile::NamedTempFile,
    path: &Path,
    overwrite: bool,
) -> Result<(), StorageError> {
    if overwrite {
        temp.persist(path).map_err(|e| e.error)?;
    } else {
        temp.persist_noclobber(path).map_err(|e| e.error)?;
    }
    sync_parent(path).map_err(|e| StorageError::PublishedUncertain(e.to_string()))
}
