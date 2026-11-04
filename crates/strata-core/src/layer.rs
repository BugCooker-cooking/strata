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
#[serde(rename_all = "snake_case")]
pub enum BlendMode {
    #[default]
    Normal,
    Multiply,
    Screen,
    Overlay,
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
