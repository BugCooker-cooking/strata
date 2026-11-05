use crate::LayerId;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use strata_pixels::{MaskLayout, TiledMask, TiledPixels};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum LayerKind {
    #[default]
    Pixel,
    Exposure {
        stops: f64,
    },
    Group,
    Solid {
        color: [u16; 4],
    },
    Gradient {
        start: [u16; 4],
        end: [u16; 4],
        angle: f64,
        scale: f64,
        #[serde(default)]
        radial: bool,
    },
    HueSaturation {
        hue: f64,
        saturation: f64,
        lightness: f64,
    },
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LayerMaskInfo {
    pub enabled: bool,
    pub layout: MaskLayout,
    /// Canvas anchor when this mask was attached. Layer movement remains linked.
    pub anchor_x: i64,
    pub anchor_y: i64,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LayerLocks {
    #[serde(default)]
    pub transparency: bool,
    #[serde(default)]
    pub pixels: bool,
    #[serde(default)]
    pub position: bool,
    #[serde(default)]
    pub all: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ColorLabel {
    Red,
    Orange,
    Yellow,
    Green,
    Blue,
    Violet,
    Gray,
}
pub(crate) fn full_fill_opacity() -> f64 {
    1.0
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BlendMode {
    #[default]
    Normal,
    Multiply,
    Screen,
    Overlay,
    Dissolve,
    Darken,
    ColorBurn,
    LinearBurn,
    DarkerColor,
    Lighten,
    ColorDodge,
    LinearDodge,
    LighterColor,
    SoftLight,
    HardLight,
    VividLight,
    LinearLight,
    PinLight,
    HardMix,
    Difference,
    Exclusion,
    Subtract,
    Divide,
    Hue,
    Saturation,
    Color,
    Luminosity,
}
impl BlendMode {
    pub const ALL: [Self; 27] = [
        Self::Normal,
        Self::Dissolve,
        Self::Darken,
        Self::Multiply,
        Self::ColorBurn,
        Self::LinearBurn,
        Self::DarkerColor,
        Self::Lighten,
        Self::Screen,
        Self::ColorDodge,
        Self::LinearDodge,
        Self::LighterColor,
        Self::Overlay,
        Self::SoftLight,
        Self::HardLight,
        Self::VividLight,
        Self::LinearLight,
        Self::PinLight,
        Self::HardMix,
        Self::Difference,
        Self::Exclusion,
        Self::Subtract,
        Self::Divide,
        Self::Hue,
        Self::Saturation,
        Self::Color,
        Self::Luminosity,
    ];
}

/// Stable ID preferred. Duplicate names return candidates instead of guessing.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LayerRef {
    Id(LayerId),
    Name(String),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LayerInfo {
    #[serde(default)]
    pub styles: crate::styles::LayerStyles,
    #[serde(default)]
    pub parent: Option<LayerId>,
    #[serde(default)]
    pub clipped: bool,
    #[serde(default)]
    pub generated_size: Option<[u32; 2]>,
    #[serde(default)]
    pub pass_through: bool,
    #[serde(default = "full_fill_opacity")]
    pub fill_opacity: f64,
    #[serde(default)]
    pub locks: LayerLocks,
    #[serde(default)]
    pub color_label: Option<ColorLabel>,
    #[serde(default)]
    pub kind: LayerKind,
    #[serde(default)]
    pub mask: Option<LayerMaskInfo>,
    pub id: LayerId,
    pub name: String,
    pub visible: bool,
    pub opacity: f64,
    pub blend_mode: BlendMode,
    pub x: i64,
    pub y: i64,
}

#[derive(Clone, Debug)]
pub struct Layer {
    pub(crate) info: LayerInfo,
    pub(crate) pixels: TiledPixels,
    pub(crate) mask: Option<TiledMask>,
}
impl Layer {
    pub fn id(&self) -> LayerId {
        self.info.id
    }
    pub fn info(&self) -> &LayerInfo {
        &self.info
    }
    pub fn pixels(&self) -> &TiledPixels {
        &self.pixels
    }
    pub fn mask(&self) -> Option<&TiledMask> {
        self.mask.as_ref()
    }
    pub(crate) fn new(name: String, pixels: TiledPixels) -> Self {
        Self {
            info: LayerInfo {
                styles: crate::styles::LayerStyles::default(),
                parent: None,
                clipped: false,
                generated_size: None,
                pass_through: false,
                fill_opacity: 1.0,
                locks: LayerLocks::default(),
                color_label: None,
                kind: LayerKind::Pixel,
                mask: None,
                id: LayerId::fresh(),
                name,
                visible: true,
                opacity: 1.0,
                blend_mode: BlendMode::Normal,
                x: 0,
                y: 0,
            },
            pixels,
            mask: None,
        }
    }
}
