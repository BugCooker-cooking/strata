//! Bit-depth-preserving ICC transforms for display copies, never document storage.

use lcms2::{
    CIExyY, CIExyYTRIPLE, ColorSpaceSignature, Flags, InfoType, Intent, Locale, MLU, PixelFormat,
    Profile, Tag, TagSignature, ToneCurve, Transform,
};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex, OnceLock},
};
use strata_pixels::{PixelBuffer, PixelError};

#[derive(Debug, thiserror::Error)]
pub enum ColorError {
    #[error(transparent)]
    Pixels(#[from] PixelError),
    #[error("ICC profile is not compatible with RGB pixels: {0}")]
    UnsupportedProfile(String),
    #[error("color transform failed: {0}")]
    Transform(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Converted {
    pub pixels: PixelBuffer,
    pub source_profile: String,
    pub profile_fallback: bool,
}

fn transform_error(error: impl std::fmt::Display) -> ColorError {
    ColorError::Transform(error.to_string())
}

pub struct ResolvedProfile {
    pub icc: Vec<u8>,
    pub name: String,
    pub fallback: bool,
}

/// Resolve the working profile without converting source samples.
pub fn resolve_profile(bytes: Option<&[u8]>) -> Result<ResolvedProfile, ColorError> {
    let (profile, name, fallback) = match bytes {
        None => (Profile::new_srgb(), "sRGB (assumed)".to_owned(), false),
        Some(bytes) => match Profile::new_icc(bytes) {
            Ok(profile) => {
                if profile.color_space() != ColorSpaceSignature::RgbData {
                    return Err(ColorError::UnsupportedProfile("expected RGB".into()));
                }
                let name = profile
                    .info(InfoType::Description, Locale::new("en_US"))
                    .unwrap_or_else(|| "Embedded RGB profile".into());
                return Ok(ResolvedProfile {
                    icc: bytes.to_vec(),
                    name,
                    fallback: false,
                });
            }
            Err(_) => (Profile::new_srgb(), "sRGB (fallback)".into(), true),
        },
    };
    Ok(ResolvedProfile {
        icc: profile.icc().map_err(transform_error)?,
        name,
        fallback,
    })
}

fn linear_p3_profile() -> Result<Profile, ColorError> {
    let point = |x, y| CIExyY { x, y, Y: 1.0 };
    // Both branches are exactly y=x, including negative/extended RGB. LCMS's
    // type-1 gamma=1 evaluator still calls pow() for every positive sample;
    // type 5 takes its linear branch throughout the ordinary working range.
    let gamma = ToneCurve::new_parametric(5, &[1.0, 1.0, 0.0, 1.0, 2.0, 0.0, 0.0])
        .map_err(transform_error)?;
    Profile::new_rgb(
        &point(0.3127, 0.3290),
        &CIExyYTRIPLE {
            Red: point(0.680, 0.320),
            Green: point(0.265, 0.690),
            Blue: point(0.150, 0.060),
        },
        &[&gamma, &gamma, &gamma],
    )
    .map_err(transform_error)
}

/// Floating-point, straight-alpha linear P3 for composition, never source storage.
pub fn to_linear_p3(
    pixels: &PixelBuffer,
    width: u32,
    height: u32,
    icc: &[u8],
) -> Result<Vec<[f32; 4]>, ColorError> {
    pixels.validate(width, height)?;
    let table = if matches!(pixels, PixelBuffer::Rgba8(_)) {
        linear_rgb8_table(icc)?
    } else {
        None
    };
    let mut input = Profile::new_icc(icc).map_err(transform_error)?;
    if table.is_some() {
        // Keep the identity as a two-point float table in memory. Serializing
        // it would turn it back into a parametric gamma=1 curve and retain a
        // per-sample pow() call in LCMS's unoptimized float pipeline.
        let identity = ToneCurve::new_tabulated_float(&[0.0, 1.0]);
        for tag in [
            TagSignature::RedTRCTag,
            TagSignature::GreenTRCTag,
            TagSignature::BlueTRCTag,
        ] {
            if !input.write_tag(tag, Tag::ToneCurve(&identity)) {
                return Err(transform_error("cannot prepare linear input profile"));
            }
        }
    }
    let output = linear_p3_profile()?;
    let source: Vec<[f32; 4]> = match pixels {
        PixelBuffer::Rgba8(v) => v
            .chunks_exact(4)
            .map(|p| {
                [
                    table
                        .as_ref()
                        .map_or(p[0] as f32 / 255.0, |t| t.curves[0][p[0] as usize]),
                    table
                        .as_ref()
                        .map_or(p[1] as f32 / 255.0, |t| t.curves[1][p[1] as usize]),
                    table
                        .as_ref()
                        .map_or(p[2] as f32 / 255.0, |t| t.curves[2][p[2] as usize]),
                    p[3] as f32 / 255.0,
                ]
            })
            .collect(),
        PixelBuffer::Rgba16(v) => v
            .chunks_exact(4)
            .map(|p| {
                [
                    p[0] as f32 / 65535.0,
                    p[1] as f32 / 65535.0,
                    p[2] as f32 / 65535.0,
                    p[3] as f32 / 65535.0,
                ]
            })
            .collect(),
    };
    let transform = Transform::<[f32; 4], [f32; 4]>::new_flags(
        &input,
        PixelFormat::RGBA_FLT,
        &output,
        PixelFormat::RGBA_FLT,
        Intent::RelativeColorimetric,
        Flags::NO_OPTIMIZE | Flags::COPY_ALPHA,
    )
    .map_err(transform_error)?;
    let mut result = vec![[0.0; 4]; source.len()];
    transform.transform_pixels(&source, &mut result);
    for (dst, src) in result.iter_mut().zip(source) {
        dst[3] = src[3];
    }
    Ok(result)
}

struct LinearRgb8Table {
    curves: [[f32; 256]; 3],
}

type LinearRgb8Cache = VecDeque<(Vec<u8>, Arc<LinearRgb8Table>)>;
fn linear_rgb8_table(icc: &[u8]) -> Result<Option<Arc<LinearRgb8Table>>, ColorError> {
    // Evaluate each of the 256 integer codes through the original LCMS TRC
    // exactly once, then let LCMS transform those floats using the same input
    // matrix with identity TRCs. Keep matrix adaptation and output clipping in
    // LCMS. No interpolation, quantization, CLUT or 16-bit shortcut is used.
    // Bound shared profile metadata to four entries of at most 64 KiB each.
    if icc.len() > 64 * 1024 {
        return Ok(None);
    }
    static CACHE: OnceLock<Mutex<LinearRgb8Cache>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(VecDeque::new()));
    {
        let mut entries = cache.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(index) = entries.iter().position(|(key, _)| key == icc) {
            let entry = entries.remove(index).expect("cache index exists");
            let table = entry.1.clone();
            entries.push_back(entry);
            return Ok(Some(table));
        }
    }
    let input = Profile::new_icc(icc).map_err(transform_error)?;
    let lut_tags = [
        TagSignature::AToB0Tag,
        TagSignature::AToB1Tag,
        TagSignature::AToB2Tag,
        TagSignature::DToB0Tag,
        TagSignature::DToB1Tag,
        TagSignature::DToB2Tag,
        TagSignature::DToB3Tag,
    ];
    if !input.is_matrix_shaper() || lut_tags.iter().any(|tag| input.has_tag(*tag)) {
        return Ok(None);
    }
    let mut table = LinearRgb8Table {
        curves: [[0.0; 256]; 3],
    };
    let tags = [
        TagSignature::RedTRCTag,
        TagSignature::GreenTRCTag,
        TagSignature::BlueTRCTag,
    ];
    for (channel, tag) in tags.iter().enumerate() {
        let Tag::ToneCurve(curve) = input.read_tag(*tag) else {
            return Ok(None);
        };
        for code in 0..256 {
            table.curves[channel][code] = curve.eval(code as f32 / 255.0);
        }
    }
    // A two-point identity table is exact inside [0,1]. Keep the original
    // LCMS path for unusual curves with negative, extended or nonfinite values.
    if table
        .curves
        .iter()
        .flatten()
        .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
    {
        return Ok(None);
    }
    let table = Arc::new(table);
    let mut entries = cache.lock().unwrap_or_else(|e| e.into_inner());
    if entries.len() >= 4 {
        entries.pop_front();
    }
    entries.push_back((icc.to_vec(), table.clone()));
    Ok(Some(table))
}

/// Convert straight linear composition to the chosen output ICC at document depth.
pub fn from_linear_p3(
    source: &[[f32; 4]],
    format: strata_pixels::PixelFormat,
    icc: &[u8],
) -> Result<PixelBuffer, ColorError> {
    let input = linear_p3_profile()?;
    let output = Profile::new_icc(icc).map_err(transform_error)?;
    let transform = Transform::<[f32; 4], [f32; 4]>::new_flags(
        &input,
        PixelFormat::RGBA_FLT,
        &output,
        PixelFormat::RGBA_FLT,
        Intent::RelativeColorimetric,
        Flags::NO_OPTIMIZE | Flags::COPY_ALPHA,
    )
    .map_err(transform_error)?;
    let mut result = vec![[0.0; 4]; source.len()];
    transform.transform_pixels(source, &mut result);
    for (dst, src) in result.iter_mut().zip(source) {
        dst[3] = src[3];
    }
    Ok(match format {
        strata_pixels::PixelFormat::Rgba8 => PixelBuffer::Rgba8(
            result
                .iter()
                .flatten()
                .map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)
                .collect(),
        ),
        strata_pixels::PixelFormat::Rgba16 => PixelBuffer::Rgba16(
            result
                .iter()
                .flatten()
                .map(|v| (v.clamp(0.0, 1.0) * 65535.0).round() as u16)
                .collect(),
        ),
    })
}

/// Display P3: D65, P3 primaries and the sRGB transfer curve (not gamma 2.2).
pub fn display_p3_profile() -> Result<Profile, ColorError> {
    let point = |x, y| CIExyY { x, y, Y: 1.0 };
    let curve =
        ToneCurve::new_parametric(4, &[2.4, 1.0 / 1.055, 0.055 / 1.055, 1.0 / 12.92, 0.04045])
            .map_err(transform_error)?;
    let primaries = CIExyYTRIPLE {
        Red: point(0.680, 0.320),
        Green: point(0.265, 0.690),
        Blue: point(0.150, 0.060),
    };
    let mut profile = Profile::new_rgb(
        &point(0.3127, 0.3290),
        &primaries,
        &[&curve, &curve, &curve],
    )
    .map_err(transform_error)?;
    let mut description = MLU::new(1);
    if !description.set_text("Display P3", Locale::new("en_US"))
        || !profile.write_tag(TagSignature::ProfileDescriptionTag, Tag::MLU(&description))
    {
        return Err(transform_error("cannot describe Display P3 profile"));
    }
    Ok(profile)
}

pub fn to_display_p3(
    pixels: &PixelBuffer,
    width: u32,
    height: u32,
    source_icc: Option<&[u8]>,
) -> Result<Converted, ColorError> {
    convert_profile(pixels, width, height, source_icc, &display_p3_profile()?)
}

pub fn working_profile(name: &str) -> Result<Vec<u8>, ColorError> {
    match name {
        "srgb" => Profile::new_srgb().icc().map_err(transform_error),
        "display_p3" => display_p3_profile()?.icc().map_err(transform_error),
        _ => Err(transform_error("working space must be srgb or display_p3")),
    }
}
pub fn convert_to_profile(
    pixels: &PixelBuffer,
    width: u32,
    height: u32,
    source_icc: &[u8],
    destination_icc: &[u8],
) -> Result<PixelBuffer, ColorError> {
    let destination = Profile::new_icc(destination_icc).map_err(transform_error)?;
    Ok(convert_profile(pixels, width, height, Some(source_icc), &destination)?.pixels)
}
fn convert_profile(
    pixels: &PixelBuffer,
    width: u32,
    height: u32,
    source_icc: Option<&[u8]>,
    destination: &Profile,
) -> Result<Converted, ColorError> {
    pixels.validate(width, height)?;
    let (source, source_profile, profile_fallback) = match source_icc {
        None => (Profile::new_srgb(), "sRGB (assumed)".to_owned(), false),
        Some(icc) => match Profile::new_icc(icc) {
            Ok(profile) => {
                if profile.color_space() != ColorSpaceSignature::RgbData {
                    return Err(ColorError::UnsupportedProfile(format!(
                        "{:?}",
                        profile.color_space()
                    )));
                }
                let description = profile
                    .info(InfoType::Description, Locale::new("en_US"))
                    .unwrap_or_else(|| "Embedded RGB profile".into());
                (profile, description, false)
            }
            Err(_) => (Profile::new_srgb(), "sRGB (fallback)".into(), true),
        },
    };
    // Do not round-trip equal profiles through PCS: serialized ICC coefficients
    // have finite precision, so even an identity transform may lose adjacent codes.
    if equivalent_profile_bytes(&source, destination)? {
        return Ok(Converted {
            pixels: pixels.clone(),
            source_profile,
            profile_fallback,
        });
    }
    // Disable lookup-table optimization: preserve the 16-bit path's precision.
    // Alpha is explicitly copied after the transform, not entrusted to ICC behavior.
    let converted = match pixels {
        PixelBuffer::Rgba8(src) => {
            let transform = Transform::<[u8; 4], [u8; 4]>::new_flags(
                &source,
                PixelFormat::RGBA_8,
                destination,
                PixelFormat::RGBA_8,
                Intent::RelativeColorimetric,
                Flags::NO_OPTIMIZE | Flags::COPY_ALPHA,
            )
            .map_err(transform_error)?;
            let mut dst = vec![0u8; src.len()];
            transform.transform_pixels(
                bytemuck::cast_slice(src),
                bytemuck::cast_slice_mut(&mut dst),
            );
            for (input, output) in src.chunks_exact(4).zip(dst.chunks_exact_mut(4)) {
                output[3] = input[3];
            }
            PixelBuffer::Rgba8(dst)
        }
        PixelBuffer::Rgba16(src) => {
            let transform = Transform::<[u16; 4], [u16; 4]>::new_flags(
                &source,
                PixelFormat::RGBA_16,
                destination,
                PixelFormat::RGBA_16,
                Intent::RelativeColorimetric,
                Flags::NO_OPTIMIZE | Flags::COPY_ALPHA,
            )
            .map_err(transform_error)?;
            let mut dst = vec![0u16; src.len()];
            transform.transform_pixels(
                bytemuck::cast_slice(src),
                bytemuck::cast_slice_mut(&mut dst),
            );
            for (input, output) in src.chunks_exact(4).zip(dst.chunks_exact_mut(4)) {
                output[3] = input[3];
            }
            PixelBuffer::Rgba16(dst)
        }
    };
    Ok(Converted {
        pixels: converted,
        source_profile,
        profile_fallback,
    })
}

fn equivalent_profile_bytes(a: &Profile, b: &Profile) -> Result<bool, ColorError> {
    let normalize = |profile: &Profile| -> Result<Vec<u8>, ColorError> {
        let mut bytes = profile.icc().map_err(transform_error)?;
        // Ignore creation date and profile checksum only, not color tags or names.
        if bytes.len() >= 128 {
            bytes[24..36].fill(0);
            bytes[84..100].fill(0);
        }
        Ok(bytes)
    };
    Ok(normalize(a)? == normalize(b)?)
}

#[cfg(test)]
mod tests;
