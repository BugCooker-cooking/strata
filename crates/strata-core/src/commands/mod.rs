mod executor;
mod export;
mod registry;
pub use executor::{
    CommandError, Control, Editor, HistoryInfo, MoveInteraction, PreparedCommand, Request,
    Snapshot, Source,
};
pub use registry::{CommandKind, CommandSpec, catalog};

use crate::{BlendMode, LayerRef};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

macro_rules! params {
    ($name:ident { $($(#[$attr:meta])* $field:ident : $ty:ty),* $(,)? }) => {
        #[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
        #[serde(deny_unknown_fields)]
        pub struct $name { $( $(#[$attr])* pub $field: $ty ),* }
    };
}
params!(Empty {});
#[derive(Serialize, JsonSchema)]
pub struct ColorResult {
    pub working_space: String,
}
params!(ConvertColor { space: String });
#[derive(Serialize, JsonSchema)]
pub struct LayerResult {
    pub layer_id: crate::LayerId,
}
#[derive(Serialize, JsonSchema)]
pub struct LayerListResult {
    pub layers: Vec<crate::LayerInfo>,
}
#[derive(Serialize, JsonSchema)]
pub struct HistoryResult {
    pub history_cursor: usize,
}
#[derive(Serialize, JsonSchema)]
pub struct ExternalResult {
    pub status: String,
    pub command: String,
}
#[derive(Serialize, JsonSchema)]
pub struct NewResult {
    pub document_id: crate::DocumentId,
    pub layer_id: Option<crate::LayerId>,
}
#[derive(Serialize, JsonSchema)]
pub struct BatchResult {
    pub results: Vec<Value>,
}
#[derive(Serialize, JsonSchema)]
pub struct ViewResult {
    pub view: String,
    pub params: Value,
}
params!(OpenFile {
    resource: String,
    #[serde(default)]
    discard_changes: bool
});
params!(SaveFile { resource: String });
params!(PlaceFile {
    resource: String,
    #[serde(default)]
    x: i64,
    #[serde(default)]
    y: i64
});
params!(ExportFile {
    resource: String,
    format: String,
    #[serde(default)]
    target_space: Option<String>,
    #[serde(default)]
    quality: Option<u8>,
    #[serde(default)]
    background: Option<[u8; 3]>,
    #[serde(default)]
    allow_quantization: bool
});
params!(NewDocument {
    width: u32,
    height: u32,
    bit_depth: u8,
    #[serde(default)]
    discard_changes: bool
});
params!(CreateLayer { name:String, #[serde(default)] fill:Option<[u16;4]> });
params!(Target { layer: LayerRef });
params!(Rename {
    layer: LayerRef,
    name: String
});
params!(Visibility {
    layer: LayerRef,
    visible: bool
});
params!(Opacity {
    layer: LayerRef,
    opacity: f64
});
params!(Blend {
    layer: LayerRef,
    blend_mode: BlendMode
});
params!(Position {
    layer: LayerRef,
    x: i64,
    y: i64
});
params!(Order {
    layer: LayerRef,
    index: usize
});
params!(BatchStep {
    command: String,
    params: Value
});
params!(Batch { steps:Vec<BatchStep> });
params!(Pan { dx: f32, dy: f32 });
params!(Zoom {
    x: f32,
    y: f32,
    factor: f32
});
params!(UndoTurn {
    turn_id: uuid::Uuid
});

fn parse<T: serde::de::DeserializeOwned>(params: Value) -> Result<T, CommandError> {
    serde_json::from_value(params).map_err(|e| CommandError::Invalid(e.to_string()))
}
