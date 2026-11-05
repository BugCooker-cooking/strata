use super::TonalAdjustment as A;
use crate::DocumentError;
fn range(values: impl IntoIterator<Item = f64>, low: f64, high: f64) -> bool {
    values
        .into_iter()
        .all(|v| v.is_finite() && (low..=high).contains(&v))
}
pub(super) fn validate(settings: &A) -> Result<(), DocumentError> {
    let valid = match settings {
        A::WhiteBalance {
            temperature, tint, ..
        } => range([*temperature, *tint], -100.0, 100.0),
        A::Vibrance {
            vibrance,
            saturation,
        } => range([*vibrance, *saturation], -100.0, 100.0),
        A::ColorBalance {
            shadows,
            midtones,
            highlights,
            ..
        } => range(
            shadows.iter().chain(midtones).chain(highlights).copied(),
            -100.0,
            100.0,
        ),
        A::BlackWhite {
            weights,
            tint,
            tint_amount,
        } => {
            range(*weights, -200.0, 300.0)
                && range([*tint_amount], 0.0, 100.0)
                && tint.is_none_or(|v| range(v, 0.0, 1.0))
        }
        A::PhotoFilter { color, density, .. } => {
            range(*color, 0.0, 1.0) && range([*density], 0.0, 100.0)
        }
        A::ChannelMixer { matrix, gray, .. } => {
            range(matrix.iter().flatten().chain(gray).copied(), -2.0, 2.0)
        }
        A::Posterize { levels } => (2..=65536).contains(levels),
        A::Threshold { level } => range([*level], 0.0, 1.0),
        A::GradientMap { stops, .. } => {
            (2..=64).contains(&stops.len())
                && stops.first().unwrap().at == 0.0
                && stops.last().unwrap().at == 1.0
                && stops
                    .iter()
                    .all(|s| range([s.at], 0.0, 1.0) && range(s.color, 0.0, 1.0))
                && stops.windows(2).all(|p| p[1].at - p[0].at >= 1e-6)
        }
        A::SelectiveColor { ranges, .. } => range(ranges.iter().flatten().copied(), -100.0, 100.0),
        A::Invert {} | A::Desaturate {} => true,
        _ => unreachable!("tonal parameters validated in the parent module"),
    };
    if valid {
        Ok(())
    } else {
        Err(DocumentError::Invalid(
            "point adjustment parameters are outside their documented ranges".into(),
        ))
    }
}
pub(super) fn is_identity(settings: &A) -> bool {
    match settings {
        A::WhiteBalance {
            temperature, tint, ..
        } => *temperature == 0.0 && *tint == 0.0,
        A::Vibrance {
            vibrance,
            saturation,
        } => *vibrance == 0.0 && *saturation == 0.0,
        A::ColorBalance {
            shadows,
            midtones,
            highlights,
            ..
        } => shadows
            .iter()
            .chain(midtones)
            .chain(highlights)
            .all(|v| *v == 0.0),
        A::PhotoFilter { density, .. } => *density == 0.0,
        A::ChannelMixer {
            matrix, monochrome, ..
        } => !*monochrome && *matrix == super::mixer_identity(),
        A::SelectiveColor { ranges, .. } => ranges.iter().flatten().all(|v| *v == 0.0),
        _ => false,
    }
}
pub(super) fn white_balance(
    source: &[[f32; 4]],
    temperature: f64,
    tint: f64,
    preserve: bool,
) -> Vec<[f32; 4]> {
    let multiply =
        |m: [[f64; 3]; 3], p: [f64; 3]| m.map(|r| r[0] * p[0] + r[1] * p[1] + r[2] * p[2]);
    let planck = |t: f64| {
        let x = if t <= 4000.0 {
            -0.2661239e9 / t.powi(3) - 0.2343580e6 / t.powi(2) + 0.8776956e3 / t + 0.179910
        } else {
            -3.0258469e9 / t.powi(3) + 2.1070379e6 / t.powi(2) + 0.2226347e3 / t + 0.240390
        };
        let y = if t <= 4000.0 {
            -0.9549476 * x.powi(3) - 1.37418593 * x.powi(2) + 2.09137015 * x - 0.16748867
        } else {
            3.0817580 * x.powi(3) - 5.8733867 * x.powi(2) + 3.75112997 * x - 0.37001483
        };
        [x, y]
    };
    let base = planck(6504.0);
    let cct = 1.0 / (1.0 / 6504.0 + temperature * 1e-6);
    let target = planck(cct);
    let x = target[0] + 0.3127 - base[0];
    let y = target[1] + 0.3290 - base[1];
    let xyz_white = [0.3127 / 0.3290, 1.0, (1.0 - 0.3127 - 0.3290) / 0.3290];
    let xyz_target = [x / y, 1.0, (1.0 - x - y) / y];
    let bradford = [
        [0.8951, 0.2664, -0.1614],
        [-0.7502, 1.7135, 0.0367],
        [0.0389, -0.0685, 1.0296],
    ];
    let inverse = [
        [0.9869929, -0.1470543, 0.1599627],
        [0.4323053, 0.5183603, 0.0492912],
        [-0.0085287, 0.0400428, 0.9684867],
    ];
    let input = multiply(bradford, xyz_white);
    let output = multiply(bradford, xyz_target);
    let tint_gain = [
        2.0f64.powf(tint / 400.0),
        2.0f64.powf(-tint / 200.0),
        2.0f64.powf(tint / 400.0),
    ];
    let gains: [f64; 3] = std::array::from_fn(|c| output[c] / input[c] * tint_gain[c]);
    let p3_xyz = [
        [0.4865709486482162, 0.2656676931690931, 0.1982172852343625],
        [0.2289745640697488, 0.6917385218365064, 0.0792869140937450],
        [0.0, 0.0451133818589026, 1.043944368900976],
    ];
    let xyz_p3 = [
        [2.493496911941425, -0.931383617919124, -0.402710784450717],
        [-0.829488969561575, 1.762664060318346, 0.023624685841944],
        [0.035845830243784, -0.076172389268042, 0.956884524007687],
    ];
    let transform = |p: [f64; 3]| {
        let lms = multiply(bradford, multiply(p3_xyz, p));
        multiply(
            xyz_p3,
            multiply(inverse, std::array::from_fn(|c| lms[c] * gains[c])),
        )
    };
    let columns = [
        transform([1.0, 0.0, 0.0]),
        transform([0.0, 1.0, 0.0]),
        transform([0.0, 0.0, 1.0]),
    ];
    let matrix = std::array::from_fn(|row| std::array::from_fn(|column| columns[column][row]));
    source
        .iter()
        .map(|p| {
            let rgb = [f64::from(p[0]), f64::from(p[1]), f64::from(p[2])];
            let mut out = multiply(matrix, rgb);
            let luma =
                |v: [f64; 3]| v[0] * p3_xyz[1][0] + v[1] * p3_xyz[1][1] + v[2] * p3_xyz[1][2];
            if preserve {
                let before = luma(rgb);
                let after = luma(out);
                if after.abs() > 1e-12 {
                    for v in &mut out {
                        *v *= before / after;
                    }
                }
            }
            [out[0] as f32, out[1] as f32, out[2] as f32, p[3]]
        })
        .collect()
}
fn lum(rgb: [f64; 3]) -> f64 {
    rgb[0] * 0.2126 + rgb[1] * 0.7152 + rgb[2] * 0.0722
}
fn extent(rgb: [f64; 3]) -> (f64, f64) {
    (
        rgb.into_iter().fold(f64::INFINITY, f64::min),
        rgb.into_iter().fold(f64::NEG_INFINITY, f64::max),
    )
}
fn set_lum(mut rgb: [f64; 3], target: f64) -> [f64; 3] {
    let offset = target - lum(rgb);
    for v in &mut rgb {
        *v += offset;
    }
    let l = lum(rgb);
    let (low, _) = extent(rgb);
    if low < 0.0 {
        for v in &mut rgb {
            *v = l + (*v - l) * l / (l - low);
        }
    }
    let high = extent(rgb).1;
    if high > 1.0 {
        for v in &mut rgb {
            *v = l + (*v - l) * (1.0 - l) / (high - l);
        }
    }
    rgb.map(|v| v.clamp(0.0, 1.0))
}
fn hue(rgb: [f64; 3], min: f64, max: f64) -> f64 {
    let chroma = max - min;
    if chroma <= 1e-12 {
        return 0.0;
    }
    let h = if max == rgb[0] {
        (rgb[1] - rgb[2]) / chroma
    } else if max == rgb[1] {
        (rgb[2] - rgb[0]) / chroma + 2.0
    } else {
        (rgb[0] - rgb[1]) / chroma + 4.0
    };
    h.rem_euclid(6.0)
}
pub(super) fn apply(settings: &A, rgb: [f64; 3]) -> [f64; 3] {
    let rgb = rgb.map(|v| v.clamp(0.0, 1.0));
    let (min, max) = extent(rgb);
    let y = lum(rgb);
    match settings {
        A::Vibrance {
            vibrance,
            saturation,
        } => {
            let saturation_now = if max > 0.0 { (max - min) / max } else { 0.0 };
            let hue = hue(rgb, min, max) * 60.0;
            let skin =
                (1.0 - ((hue - 30.0 + 180.0).rem_euclid(360.0) - 180.0).abs() / 35.0).max(0.0);
            let gain = (1.0 + saturation / 100.0)
                * (1.0 + vibrance / 100.0 * (1.0 - saturation_now) * (1.0 - 0.5 * skin));
            set_lum(rgb.map(|v| y + (v - y) * gain), y)
        }
        A::ColorBalance {
            shadows,
            midtones,
            highlights,
            preserve_luminance,
        } => {
            let shadow = (1.0 - y * 2.0).max(0.0);
            let high = (y * 2.0 - 1.0).max(0.0);
            let mid = 1.0 - shadow - high;
            let out = std::array::from_fn(|c| {
                rgb[c] + (shadows[c] * shadow + midtones[c] * mid + highlights[c] * high) / 400.0
            });
            if *preserve_luminance {
                set_lum(out, y)
            } else {
                out.map(|v| v.clamp(0.0, 1.0))
            }
        }
        A::BlackWhite {
            weights,
            tint,
            tint_amount,
        } => {
            let h = hue(rgb, min, max);
            let index = h.floor() as usize;
            let t = h - index as f64;
            let sensitivity = (weights[index] * (1.0 - t) + weights[(index + 1) % 6] * t) / 100.0;
            let gray = (min + (max - min) * sensitivity).clamp(0.0, 1.0);
            if let Some(tint) = tint {
                let normal = tint.iter().copied().fold(0.0, f64::max);
                if normal == 0.0 {
                    return [gray; 3];
                }
                std::array::from_fn(|c| {
                    gray * (1.0 - tint_amount / 100.0 + tint[c] / normal * tint_amount / 100.0)
                })
            } else {
                [gray; 3]
            }
        }
        A::PhotoFilter {
            color,
            density,
            preserve_luminance,
        } => {
            let amount = density / 100.0;
            let out = std::array::from_fn(|c| rgb[c] * (1.0 - amount + amount * color[c]));
            if *preserve_luminance {
                set_lum(out, y)
            } else {
                out
            }
        }
        A::ChannelMixer {
            matrix,
            monochrome,
            gray,
        } => {
            let mix = |row: [f64; 4]| {
                (rgb[0] * row[0] + rgb[1] * row[1] + rgb[2] * row[2] + row[3]).clamp(0.0, 1.0)
            };
            if *monochrome {
                [mix(*gray); 3]
            } else {
                matrix.map(mix)
            }
        }
        A::Invert {} => rgb.map(|v| 1.0 - v),
        A::Posterize { levels } => {
            let steps = f64::from(*levels - 1);
            rgb.map(|v| (v * steps).round() / steps)
        }
        A::Threshold { level } => [if y >= *level { 1.0 } else { 0.0 }; 3],
        A::GradientMap { stops, reverse } => {
            let v = if *reverse { 1.0 - y } else { y };
            let i = (stops.partition_point(|s| s.at <= v).saturating_sub(1)).min(stops.len() - 2);
            let a = &stops[i];
            let b = &stops[i + 1];
            let t = ((v - a.at) / (b.at - a.at)).clamp(0.0, 1.0);
            std::array::from_fn(|c| a.color[c] + (b.color[c] - a.color[c]) * t)
        }
        A::SelectiveColor { ranges, relative } => {
            let [r, g, b] = rgb;
            let weights = [
                (r - g.max(b)).max(0.0),
                (r.min(g) - b).max(0.0),
                (g - r.max(b)).max(0.0),
                (g.min(b) - r).max(0.0),
                (b - r.max(g)).max(0.0),
                (r.min(b) - g).max(0.0),
                min,
                1.0 - (max + min - 1.0).abs(),
                1.0 - max,
            ];
            std::array::from_fn(|c| {
                let change = ranges
                    .iter()
                    .zip(weights)
                    .map(|(range, weight)| {
                        [range[c], range[3]]
                            .into_iter()
                            .map(|delta| {
                                let scale = if *relative {
                                    if delta >= 0.0 { rgb[c] } else { 1.0 - rgb[c] }
                                } else {
                                    1.0
                                };
                                -weight * delta / 100.0 * scale
                            })
                            .sum::<f64>()
                    })
                    .sum::<f64>();
                (rgb[c] + change).clamp(0.0, 1.0)
            })
        }
        A::Desaturate {} => [(min + max) / 2.0; 3],
        _ => unreachable!("tonal operations evaluated in the parent module"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn settings(value: serde_json::Value) -> A {
        serde_json::from_value(value).unwrap()
    }
    fn close(a: [f64; 3], b: [f64; 3]) {
        for c in 0..3 {
            assert!((a[c] - b[c]).abs() < 1e-10, "{a:?} != {b:?}");
        }
    }
    #[test]
    fn channel_operations_match_fixed_rgb_references() {
        let rgb = [0.2, 0.5, 0.8];
        close(apply(&A::Invert {}, rgb), [0.8, 0.5, 0.2]);
        close(apply(&A::Desaturate {}, rgb), [0.5; 3]);
        close(
            apply(&A::Posterize { levels: 4 }, rgb),
            [1.0 / 3.0, 2.0 / 3.0, 2.0 / 3.0],
        );
        close(apply(&A::Threshold { level: 0.5 }, [0.5; 3]), [1.0; 3]);
        close(apply(&A::Threshold { level: 0.5 }, [0.499; 3]), [0.0; 3]);
        let mixer =
            settings(json!({"type":"channel_mixer","matrix":[[0,0,1,0],[1,0,0,0],[0,1,0,0]]}));
        close(apply(&mixer, rgb), [0.8, 0.2, 0.5]);
        let mono = settings(json!({"type":"channel_mixer","monochrome":true,"gray":[0.5,0.5,0,0]}));
        close(apply(&mono, rgb), [0.35; 3]);
        let map = settings(
            json!({"type":"gradient_map","stops":[{"at":0,"color":[1,0,0]},{"at":1,"color":[0,0,1]}]}),
        );
        close(apply(&map, [0.5; 3]), [0.5, 0.0, 0.5]);
    }
    #[test]
    fn bw_hue_weights_neutral_gray_and_tint_are_distinct_from_desaturate() {
        let bw = settings(json!({"type":"black_white"}));
        for (rgb, gray) in [
            ([1.0, 0.0, 0.0], 0.4),
            ([1.0, 1.0, 0.0], 0.6),
            ([0.0, 1.0, 0.0], 0.4),
            ([0.0, 1.0, 1.0], 0.6),
            ([0.0, 0.0, 1.0], 0.2),
            ([1.0, 0.0, 1.0], 0.8),
            ([0.37; 3], 0.37),
        ] {
            close(apply(&bw, rgb), [gray; 3]);
        }
        let tint = settings(json!({"type":"black_white","tint":[1,0.5,0.25],"tint_amount":100}));
        close(apply(&tint, [0.5; 3]), [0.5, 0.25, 0.125]);
    }
    #[test]
    fn vibrance_preserves_neutrals_saturated_colors_and_luminance() {
        let v = settings(json!({"type":"vibrance","vibrance":100}));
        close(apply(&v, [0.4; 3]), [0.4; 3]);
        close(apply(&v, [1.0, 0.0, 0.0]), [1.0, 0.0, 0.0]);
        let rgb = [0.4, 0.5, 0.6];
        let changed = apply(&v, rgb);
        assert!(changed[2] - changed[0] > rgb[2] - rgb[0]);
        assert!((lum(changed) - lum(rgb)).abs() < 1e-10);
        let gray = settings(json!({"type":"vibrance","saturation":-100}));
        close(apply(&gray, rgb), [lum(rgb); 3]);
    }
    #[test]
    fn balance_and_filter_luminance_protection_stay_finite_at_gamut_edges() {
        let balance = settings(
            json!({"type":"color_balance","shadows":[100,-100,100],"midtones":[-100,100,100],"highlights":[100,100,-100]}),
        );
        let filter = settings(json!({"type":"photo_filter","color":[1,0,0],"density":100}));
        for setting in [balance, filter] {
            for r in [0.0, 0.4, 1.0] {
                for g in [0.0, 0.4, 1.0] {
                    for b in [0.0, 0.4, 1.0] {
                        let rgb = [r, g, b];
                        let out = apply(&setting, rgb);
                        assert!(out.iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v)));
                        assert!((lum(out) - lum(rgb)).abs() < 1e-8);
                    }
                }
            }
        }
    }
    #[test]
    fn selective_color_separates_hue_range_and_relative_from_absolute() {
        let mut ranges = [[0.0; 4]; 9];
        ranges[0][0] = 50.0;
        let relative = A::SelectiveColor {
            ranges: Box::new(ranges),
            relative: true,
        };
        let absolute = A::SelectiveColor {
            ranges: Box::new(ranges),
            relative: false,
        };
        close(apply(&relative, [1.0, 0.0, 0.0]), [0.5, 0.0, 0.0]);
        close(apply(&relative, [0.0, 0.0, 1.0]), [0.0, 0.0, 1.0]);
        assert!(apply(&relative, [0.5, 0.2, 0.2])[0] > apply(&absolute, [0.5, 0.2, 0.2])[0]);
    }
    #[test]
    fn validation_rejects_bad_stops_coefficients_and_precision_limits() {
        for value in [
            json!({"type":"invert","unexpected":1}),
            json!({"type":"desaturate","amount":10}),
            json!({"type":"white_balance","extra":true}),
        ] {
            assert!(
                serde_json::from_value::<A>(value).is_err(),
                "unknown nested parameters must be rejected"
            );
        }
        for value in [
            json!({"type":"vibrance","vibrance":101}),
            json!({"type":"posterize","levels":1}),
            json!({"type":"threshold","level":-0.1}),
            json!({"type":"gradient_map","stops":[{"at":0,"color":[0,0,0]},{"at":0,"color":[1,1,1]}]}),
            json!({"type":"channel_mixer","gray":[3,0,0,0]}),
        ] {
            assert!(settings(value).validate().is_err());
        }
    }
    #[test]
    fn bradford_temperature_and_tint_have_expected_direction_and_preserve_linear_luma() {
        let samples = [
            [0.3, 0.3, 0.3, 0.42],
            [0.0, 0.0, 0.0, 0.0],
            [0.15, 0.3, 0.6, 1.0],
        ];
        let warm = white_balance(&samples, 50.0, 0.0, true);
        let cool = white_balance(&samples, -50.0, 0.0, true);
        let magenta = white_balance(&samples, 0.0, 50.0, true);
        assert!(warm[0][0] > warm[0][2]);
        assert!(cool[0][0] < cool[0][2]);
        assert!(magenta[0][1] < magenta[0][0] && magenta[0][1] < magenta[0][2]);
        for result in [warm, cool, magenta] {
            for (p, original) in result.iter().zip(samples) {
                assert_eq!(p[3], original[3]);
                assert!(p.iter().all(|v| v.is_finite()));
                let y = |v: [f32; 4]| v[0] * 0.22897457 + v[1] * 0.69173855 + v[2] * 0.07928691;
                assert!((y(*p) - y(original)).abs() < 1e-6);
            }
        }
        let neutral = white_balance(&samples, 0.0, 0.0, true);
        for (a, b) in neutral.iter().zip(samples) {
            for c in 0..3 {
                assert!((a[c] - b[c]).abs() < 2e-7);
            }
        }
    }
}
