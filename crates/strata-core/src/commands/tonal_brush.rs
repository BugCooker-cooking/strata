use super::*;
use crate::{Document, DocumentError};

fn smooth(v: f64, a: f64, b: f64) -> f64 {
    let t = ((v - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}
fn luminance(rgb: [f64; 3]) -> f64 {
    0.22897456 * rgb[0] + 0.69173852 * rgb[1] + 0.07928692 * rgb[2]
}
fn lightness(y: f64) -> f64 {
    let y = y.clamp(0.0, 1.0);
    if y > 216.0 / 24389.0 {
        (116.0 * y.cbrt() - 16.0) / 100.0
    } else {
        y * (24389.0 / 27.0) / 100.0
    }
}
fn range_weight(y: f64, range: BrushTonalRange) -> f64 {
    let l = lightness(y);
    match range {
        BrushTonalRange::Shadows => 1.0 - smooth(l, 0.05, 0.65),
        BrushTonalRange::Midtones => 4.0 * l * (1.0 - l),
        BrushTonalRange::Highlights => smooth(l, 0.35, 0.95),
    }
}
pub(super) fn mutate(
    doc: &mut Document,
    id: &str,
    params: Value,
    cancel: &dyn Fn() -> bool,
) -> Result<Option<Value>, CommandError> {
    let (stroke, amount, range, protect, sponge) = match id {
        "retouch.dodge" | "retouch.burn" => {
            let p: TonalBrushStroke = parse(params)?;
            (p.stroke, p.exposure, p.range, p.protect_tones, None)
        }
        "retouch.sponge" => {
            let p: SpongeStroke = parse(params)?;
            (
                p.stroke,
                p.amount,
                BrushTonalRange::Midtones,
                true,
                Some((p.mode, p.vibrance)),
            )
        }
        _ => return Ok(None),
    };
    if !amount.is_finite() || !(0.0..=1.0).contains(&amount) || stroke.clone_source.is_some() {
        return Err(CommandError::Invalid(
            "tonal brush amount/exposure must be 0..1 and cannot contain clone_source".into(),
        ));
    }
    let (_, coverage) = super::heal::footprint(doc, &stroke, cancel)?;
    let _profile_budget = doc
        .memory_pool()
        .reserve(
            strata_pixels::memory::MemoryKind::Transient,
            doc.working_icc().len() as u64 + 4096,
        )
        .map_err(DocumentError::from)?;
    let gamut = strata_color::WorkingGamut::new(doc.working_icc()).map_err(DocumentError::from)?;
    let transform = |pixel: [f32; 4], _: i64, _: i64| {
        if pixel[3] == 0.0 || amount == 0.0 {
            return Ok(pixel);
        }
        let rgb = [
            f64::from(pixel[0]),
            f64::from(pixel[1]),
            f64::from(pixel[2]),
        ];
        let y = luminance(rgb).clamp(0.0, 1.0);
        let rgb = if let Some((mode, vibrance)) = sponge {
            let chroma = rgb.iter().map(|v| (v - y).abs()).fold(0.0, f64::max);
            let strength = amount
                * if vibrance {
                    1.0 / (1.0 + 4.0 * chroma)
                } else {
                    1.0
                };
            let scale = match mode {
                SpongeMode::Saturate => 1.0 + strength,
                SpongeMode::Desaturate => 1.0 - strength,
            };
            gamut
                .compress(rgb.map(|v| y + (v - y) * scale), y)
                .map_err(DocumentError::from)?
        } else {
            let strength = amount * range_weight(y, range);
            if strength == 0.0 {
                return Ok(pixel);
            }
            let gain = 2.0f64.powf(if id == "retouch.dodge" {
                2.0 * strength
            } else {
                -2.0 * strength
            });
            if protect {
                if y <= 1e-12 {
                    return Ok(pixel);
                }
                let target = y / (y + (1.0 - y) / gain);
                gamut
                    .compress(rgb.map(|v| v * target / y), target)
                    .map_err(DocumentError::from)?
            } else {
                rgb.map(|v| (v * gain).clamp(0.0, 1.0))
            }
        };
        Ok([rgb[0] as f32, rgb[1] as f32, rgb[2] as f32, pixel[3]])
    };
    Ok(Some(super::pixel_transform::apply(
        doc,
        stroke.layer,
        &coverage,
        transform,
        cancel,
    )?))
}
