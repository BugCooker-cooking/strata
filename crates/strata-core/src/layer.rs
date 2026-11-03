use crate::LayerId;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use strata_pixels::TiledPixels;

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
    pub(crate) fn new(name: String, pixels: TiledPixels) -> Self {
        Self {
            info: LayerInfo {
                id: LayerId::fresh(),
                name,
                visible: true,
                opacity: 1.0,
                blend_mode: BlendMode::Normal,
                x: 0,
                y: 0,
            },
            pixels,
        }
    }
}
