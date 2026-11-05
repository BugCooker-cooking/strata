//! Point adjustments operate in floating encoded document working RGB. The
//! compositor converts only its bounded tile copy; source samples never change.
use crate::DocumentError;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
mod point_color;
pub(crate) mod spatial;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct LevelRange {
    pub input_black: f64,
    pub input_white: f64,
    pub gamma: f64,
    pub output_black: f64,
    pub output_white: f64,
}
impl Default for LevelRange {
    fn default() -> Self {
        Self {
            input_black: 0.0,
            input_white: 1.0,
            gamma: 1.0,
            output_black: 0.0,
            output_white: 1.0,
        }
    }
}
fn identity_curve() -> Vec<[f64; 2]> {
    vec![[0.0, 0.0], [1.0, 1.0]]
}
fn identity_channels() -> [Vec<[f64; 2]>; 3] {
    std::array::from_fn(|_| identity_curve())
}
fn yes() -> bool {
    true
}
fn density() -> f64 {
    25.0
}
fn half() -> f64 {
    0.5
}
fn local_radius() -> f64 {
    30.0
}
fn poster_levels() -> u32 {
    6
}
fn warm_filter() -> [f64; 3] {
    [1.0, 0.65, 0.3]
}
fn bw_weights() -> [f64; 6] {
    [40.0, 60.0, 40.0, 60.0, 20.0, 80.0]
}
fn mixer_identity() -> [[f64; 4]; 3] {
    [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ]
}
fn gray_mix() -> [f64; 4] {
    [0.3, 0.59, 0.11, 0.0]
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GradientMapStop {
    pub at: f64,
    pub color: [f64; 3],
}
fn default_gradient() -> Vec<GradientMapStop> {
    vec![
        GradientMapStop {
            at: 0.0,
            color: [0.0; 3],
        },
        GradientMapStop {
            at: 1.0,
            color: [1.0; 3],
        },
    ]
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum TonalAdjustment {
    BrightnessContrast {
        #[serde(default)]
        brightness: f64,
        #[serde(default)]
        contrast: f64,
    },
    Levels {
        #[serde(default)]
        master: LevelRange,
        #[serde(default)]
        channels: [LevelRange; 3],
    },
    Curves {
        #[serde(default = "identity_curve")]
        master: Vec<[f64; 2]>,
        #[serde(default = "identity_channels")]
        channels: [Vec<[f64; 2]>; 3],
    },
    Vibrance {
        #[serde(default)]
        vibrance: f64,
        #[serde(default)]
        saturation: f64,
    },
    WhiteBalance {
        #[serde(default)]
        temperature: f64,
        #[serde(default)]
        tint: f64,
        #[serde(default = "yes")]
        preserve_luminance: bool,
    },
    ShadowsHighlights {
        #[serde(default)]
        shadows: f64,
        #[serde(default)]
        highlights: f64,
        #[serde(default = "half")]
        shadows_width: f64,
        #[serde(default = "half")]
        highlights_width: f64,
        #[serde(default = "local_radius")]
        radius: f64,
    },
    ColorBalance {
        #[serde(default)]
        shadows: [f64; 3],
        #[serde(default)]
        midtones: [f64; 3],
        #[serde(default)]
        highlights: [f64; 3],
        #[serde(default = "yes")]
        preserve_luminance: bool,
    },
    BlackWhite {
        #[serde(default = "bw_weights")]
        weights: [f64; 6],
        #[serde(default)]
        tint: Option<[f64; 3]>,
        #[serde(default = "density")]
        tint_amount: f64,
    },
    PhotoFilter {
        #[serde(default = "warm_filter")]
        color: [f64; 3],
        #[serde(default = "density")]
        density: f64,
        #[serde(default = "yes")]
        preserve_luminance: bool,
    },
    ChannelMixer {
        #[serde(default = "mixer_identity")]
        matrix: [[f64; 4]; 3],
        #[serde(default)]
        monochrome: bool,
        #[serde(default = "gray_mix")]
        gray: [f64; 4],
    },
    Invert {},
    Posterize {
        #[serde(default = "poster_levels")]
        levels: u32,
    },
    Threshold {
        #[serde(default = "half")]
        level: f64,
    },
    GradientMap {
        #[serde(default = "default_gradient")]
        stops: Vec<GradientMapStop>,
        #[serde(default)]
        reverse: bool,
    },
    SelectiveColor {
        #[serde(default)]
        ranges: Box<[[f64; 4]; 9]>,
        #[serde(default = "yes")]
        relative: bool,
    },
    Desaturate {},
}
impl TonalAdjustment {
    pub(crate) fn transform(
        &self,
        source: &[[f32; 4]],
        icc: &[u8],
    ) -> Result<Vec<[f32; 4]>, strata_color::ColorError> {
        if matches!(self, Self::ShadowsHighlights { .. }) {
            return Err(strata_color::ColorError::Transform(
                "local tone needs the neighboring composition input".into(),
            ));
        }
        if let Self::WhiteBalance {
            temperature,
            tint,
            preserve_luminance,
        } = self
        {
            return Ok(point_color::white_balance(
                source,
                *temperature,
                *tint,
                *preserve_luminance,
            ));
        }
        let mut working = strata_color::linear_p3_to_working(source, icc)?;
        let prepared = self.prepare();
        for p in &mut working {
            let value = prepared.color([f64::from(p[0]), f64::from(p[1]), f64::from(p[2])]);
            for c in 0..3 {
                p[c] = value[c] as f32;
            }
        }
        strata_color::working_to_linear_p3(&working, icc)
    }
    pub fn curve_samples(points: &[[f64; 2]], count: usize) -> Result<Vec<f32>, DocumentError> {
        if !(2..=1025).contains(&count) {
            return Err(DocumentError::Invalid(
                "curve preview sample count 2..1025".into(),
            ));
        }
        Self::Curves {
            master: points.to_vec(),
            channels: identity_channels(),
        }
        .validate()?;
        let curve = Curve::new(points);
        Ok((0..count)
            .map(|i| curve.sample(i as f64 / (count - 1) as f64) as f32)
            .collect())
    }
    pub fn validate(&self) -> Result<(), DocumentError> {
        let invalid = |message: &str| Err(DocumentError::Invalid(message.into()));
        match self {
            Self::ShadowsHighlights {
                shadows,
                highlights,
                shadows_width,
                highlights_width,
                radius,
            } => {
                if [shadows, highlights]
                    .iter()
                    .any(|v| !v.is_finite() || !(0.0..=100.0).contains(*v))
                    || [shadows_width, highlights_width]
                        .iter()
                        .any(|v| !v.is_finite() || !(0.01..=1.0).contains(*v))
                    || !radius.is_finite()
                    || !(0.0..=256.0).contains(radius)
                {
                    return invalid(
                        "local tone needs strengths 0..100, widths 0.01..1 and radius 0..256",
                    );
                }
            }
            Self::BrightnessContrast {
                brightness,
                contrast,
            } => {
                if [brightness, contrast]
                    .iter()
                    .any(|v| !v.is_finite() || !(-100.0..=100.0).contains(*v))
                {
                    return invalid("brightness/contrast must be finite in -100..100");
                }
            }
            Self::Levels { master, channels } => {
                for p in std::iter::once(master).chain(channels) {
                    if [p.input_black, p.input_white, p.output_black, p.output_white]
                        .into_iter()
                        .any(|v| !v.is_finite() || !(0.0..=1.0).contains(&v))
                        || p.input_black >= p.input_white
                        || !p.gamma.is_finite()
                        || !(0.1..=10.0).contains(&p.gamma)
                    {
                        return invalid(
                            "levels require input black < white in 0..1, outputs 0..1 and gamma 0.1..10",
                        );
                    }
                }
            }
            Self::Curves { master, channels } => {
                for points in std::iter::once(master).chain(channels) {
                    if !(2..=64).contains(&points.len())
                        || points.first().unwrap()[0] != 0.0
                        || points.last().unwrap()[0] != 1.0
                        || points
                            .iter()
                            .flatten()
                            .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
                        || points.windows(2).any(|p| p[1][0] - p[0][0] < 1e-6)
                    {
                        return invalid(
                            "curves require 2..64 points in 0..1 with strictly increasing x, first x=0 and last x=1",
                        );
                    }
                }
            }
            _ => point_color::validate(self)?,
        }
        Ok(())
    }
    pub fn is_identity(&self) -> bool {
        match self {
            Self::ShadowsHighlights {
                shadows,
                highlights,
                ..
            } => *shadows == 0.0 && *highlights == 0.0,
            Self::BrightnessContrast {
                brightness,
                contrast,
            } => *brightness == 0.0 && *contrast == 0.0,
            Self::Levels { master, channels } => {
                *master == LevelRange::default()
                    && channels.iter().all(|p| *p == LevelRange::default())
            }
            Self::Curves { master, channels } => std::iter::once(master)
                .chain(channels)
                .all(|points| points.iter().all(|p| p[0] == p[1])),
            _ => point_color::is_identity(self),
        }
    }
    pub(crate) fn prepare(&self) -> PreparedTonal {
        match self {
            Self::Curves { master, channels } => PreparedTonal::Curves {
                master: Curve::new(master),
                channels: std::array::from_fn(|c| Curve::new(&channels[c])),
            },
            _ => PreparedTonal::Other(self.clone()),
        }
    }
}
pub(crate) enum PreparedTonal {
    Curves { master: Curve, channels: [Curve; 3] },
    Other(TonalAdjustment),
}
impl PreparedTonal {
    pub(crate) fn color(&self, rgb: [f64; 3]) -> [f64; 3] {
        match self {
            Self::Curves { master, channels } => {
                std::array::from_fn(|c| channels[c].sample(master.sample(rgb[c])))
            }
            Self::Other(TonalAdjustment::BrightnessContrast {
                brightness,
                contrast,
            }) => rgb.map(|v| {
                let v = v + brightness / 100.0;
                if *contrast == 100.0 {
                    return if v >= 0.5 { 1.0 } else { 0.0 };
                }
                let c = contrast / 100.0;
                let gain = if c >= 0.0 { 1.0 / (1.0 - c) } else { 1.0 + c };
                ((v - 0.5) * gain + 0.5).clamp(0.0, 1.0)
            }),
            Self::Other(TonalAdjustment::Levels { master, channels }) => {
                std::array::from_fn(|c| level(level(rgb[c], master), &channels[c]))
            }
            Self::Other(settings) => point_color::apply(settings, rgb),
        }
    }
}
fn level(value: f64, p: &LevelRange) -> f64 {
    let value = ((value - p.input_black) / (p.input_white - p.input_black))
        .clamp(0.0, 1.0)
        .powf(1.0 / p.gamma);
    p.output_black + (p.output_white - p.output_black) * value
}
pub(crate) struct Curve {
    points: Vec<[f64; 2]>,
    slopes: Vec<f64>,
}
impl Curve {
    fn new(points: &[[f64; 2]]) -> Self {
        let h = points
            .windows(2)
            .map(|p| p[1][0] - p[0][0])
            .collect::<Vec<_>>();
        let delta = points
            .windows(2)
            .zip(&h)
            .map(|(p, h)| (p[1][1] - p[0][1]) / h)
            .collect::<Vec<_>>();
        let n = points.len();
        let mut slopes = vec![0.0; n];
        if n == 2 {
            slopes.fill(delta[0]);
        } else {
            let endpoint = |h0: f64, h1: f64, d0: f64, d1: f64| {
                let s = ((2.0 * h0 + h1) * d0 - h0 * d1) / (h0 + h1);
                if s * d0 <= 0.0 {
                    0.0
                } else if d0 * d1 < 0.0 && s.abs() > 3.0 * d0.abs() {
                    3.0 * d0
                } else {
                    s
                }
            };
            slopes[0] = endpoint(h[0], h[1], delta[0], delta[1]);
            slopes[n - 1] = endpoint(h[n - 2], h[n - 3], delta[n - 2], delta[n - 3]);
            for i in 1..n - 1 {
                if delta[i - 1] * delta[i] > 0.0 {
                    let w1 = 2.0 * h[i] + h[i - 1];
                    let w2 = h[i] + 2.0 * h[i - 1];
                    slopes[i] = (w1 + w2) / (w1 / delta[i - 1] + w2 / delta[i]);
                }
            }
        }
        Self {
            points: points.to_vec(),
            slopes,
        }
    }
    fn sample(&self, value: f64) -> f64 {
        if value <= 0.0 {
            return self.points[0][1];
        }
        if value >= 1.0 {
            return self.points.last().unwrap()[1];
        }
        let i = self.points.partition_point(|p| p[0] <= value) - 1;
        let [x0, y0] = self.points[i];
        let [x1, y1] = self.points[i + 1];
        let h = x1 - x0;
        let t = (value - x0) / h;
        let t2 = t * t;
        let t3 = t2 * t;
        ((2.0 * t3 - 3.0 * t2 + 1.0) * y0
            + (t3 - 2.0 * t2 + t) * h * self.slopes[i]
            + (-2.0 * t3 + 3.0 * t2) * y1
            + (t3 - t2) * h * self.slopes[i + 1])
            .clamp(0.0, 1.0)
    }
}
