//! Palette formats encode the current working RGB; no document samples or ICC
//! conversion are involved. Native numeric controls and AI share this parser.
use super::*;
fn invalid(message: &str) -> CommandError {
    CommandError::Invalid(message.into())
}
fn alpha(value: f64) -> Result<f64, CommandError> {
    if value.is_finite() && (0.0..=1.0).contains(&value) {
        Ok(value)
    } else {
        Err(invalid("color alpha must be finite inside 0..1"))
    }
}
pub(super) fn parse_color(color: PaintColorInput) -> Result<[f64; 4], CommandError> {
    let value = match color {
        PaintColorInput::Rgb {
            red,
            green,
            blue,
            alpha,
        } => [red, green, blue, alpha],
        PaintColorInput::Rgb8 {
            red,
            green,
            blue,
            alpha,
        } => [
            f64::from(red) / 255.,
            f64::from(green) / 255.,
            f64::from(blue) / 255.,
            alpha,
        ],
        PaintColorInput::Rgb16 {
            red,
            green,
            blue,
            alpha,
        } => [
            f64::from(red) / 65535.,
            f64::from(green) / 65535.,
            f64::from(blue) / 65535.,
            alpha,
        ],
        PaintColorInput::Hex { value } => {
            let text = value.strip_prefix('#').unwrap_or(&value);
            if !matches!(text.len(), 6 | 8) || !text.bytes().all(|c| c.is_ascii_hexdigit()) {
                return Err(invalid(
                    "hex color needs exactly RRGGBB or RRGGBBAA, with optional #",
                ));
            }
            let mut result = [0., 0., 0., 1.];
            for (index, chunk) in text.as_bytes().chunks(2).enumerate() {
                let digits = std::str::from_utf8(chunk)
                    .map_err(|_| invalid("hex color must use ASCII digits"))?;
                result[index] = f64::from(
                    u8::from_str_radix(digits, 16).map_err(|_| invalid("invalid hex color"))?,
                ) / 255.;
            }
            result
        }
        PaintColorInput::Hsb {
            hue,
            saturation,
            brightness,
            alpha: a,
        } => {
            if !hue.is_finite()
                || !(0.0..=360.0).contains(&hue)
                || !saturation.is_finite()
                || !(0.0..=100.0).contains(&saturation)
                || !brightness.is_finite()
                || !(0.0..=100.0).contains(&brightness)
            {
                return Err(invalid(
                    "HSB requires finite hue 0..360 and saturation/brightness 0..100",
                ));
            }
            alpha(a)?;
            let h = hue.rem_euclid(360.) / 60.;
            let s = saturation / 100.;
            let v = brightness / 100.;
            let c = v * s;
            let x = c * (1. - (h.rem_euclid(2.) - 1.).abs());
            let m = v - c;
            let rgb = match h.floor() as u8 {
                0 => [c, x, 0.],
                1 => [x, c, 0.],
                2 => [0., c, x],
                3 => [0., x, c],
                4 => [x, 0., c],
                _ => [c, 0., x],
            };
            [rgb[0] + m, rgb[1] + m, rgb[2] + m, a]
        }
    };
    if value
        .iter()
        .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
    {
        return Err(invalid(
            "working RGB components and alpha must be finite inside 0..1",
        ));
    }
    Ok(value)
}
fn formats(rgba: [f64; 4]) -> PaintColorFormats {
    let max = rgba[0].max(rgba[1]).max(rgba[2]);
    let min = rgba[0].min(rgba[1]).min(rgba[2]);
    let d = max - min;
    let hue = if d == 0. {
        0.
    } else if max == rgba[0] {
        60. * ((rgba[1] - rgba[2]) / d).rem_euclid(6.)
    } else if max == rgba[1] {
        60. * ((rgba[2] - rgba[0]) / d + 2.)
    } else {
        60. * ((rgba[0] - rgba[1]) / d + 4.)
    };
    let hsb = [hue, if max == 0. { 0. } else { d / max * 100. }, max * 100.];
    let rgb8 = rgba.map(|v| (v * 255.).round() as u8);
    let rgb16 = rgba.map(|v| (v * 65535.).round() as u16);
    PaintColorFormats {
        rgba,
        rgb8,
        rgb16,
        hsb,
        hex: format!(
            "#{:02X}{:02X}{:02X}{:02X}",
            rgb8[0], rgb8[1], rgb8[2], rgb8[3]
        ),
    }
}
pub(super) fn describe(colors: &PaintColors) -> PaintColorDescription {
    PaintColorDescription {
        foreground: formats(colors.foreground),
        background: formats(colors.background),
        version: colors.version,
    }
}
