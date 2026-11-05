mod arrange;
mod executor;
mod export;
mod generated;
mod local;
mod new;
mod raster;
mod registry;
mod resample;
mod sample_selection;
mod selection;
mod tree;
pub use executor::{
    ClipboardImage, CommandError, Control, Editor, ExposureInteraction, HistoryInfo,
    MarqueeInteraction, MoveInteraction, PreparedCommand, Request, Snapshot, Source,
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
pub struct SelectionResult {
    pub selection: Option<strata_pixels::MaskLayout>,
}
#[derive(Serialize, JsonSchema)]
pub struct SubjectResult {
    pub selection: Option<strata_pixels::MaskLayout>,
    pub diagnostics: Value,
}
#[derive(Serialize, JsonSchema)]
pub struct HistogramResult {
    pub luminance: Vec<u64>,
    pub sampled_pixels: usize,
    pub nontransparent_pixels: usize,
    pub method: String,
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
pub struct RecentEntry {
    pub id: String,
    pub name: String,
    pub path: Option<String>,
    pub available: bool,
    pub read_resource: Option<String>,
}
#[derive(Serialize, JsonSchema)]
pub struct RecentListResult {
    pub scope: String,
    pub files: Vec<RecentEntry>,
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
    webp_lossless: Option<bool>,
    #[serde(default)]
    width: Option<u32>,
    #[serde(default)]
    height: Option<u32>,
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
    #[serde(default)]
    width: Option<u32>,
    #[serde(default)]
    height: Option<u32>,
    bit_depth: u8,
    #[serde(default)]
    preset: Option<String>,
    #[serde(default)]
    resolution_dpi: Option<u32>,
    #[serde(default)]
    background: Option<[u16;4]>,
    #[serde(default)]
    working_space: Option<String>,
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

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SelectionMode {
    #[default]
    Replace,
    Add,
    Subtract,
    Intersect,
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MaskPreset {
    #[default]
    White,
    Black,
    FromSelection,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AdjustmentType {
    Exposure,
}
params!(Marquee {
    x: i64,
    y: i64,
    width: u32,
    height: u32,
    #[serde(default)]
    mode: SelectionMode
});
params!(PolygonSelection{points:Vec<[f64;2]>,#[serde(default)]mode:SelectionMode});
params!(SelectionRadius { radius: f64 });
params!(TransformSelection { matrix: [f64; 6] });
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SelectionSample {
    #[default]
    Merged,
    CurrentLayer,
}
fn wand_tolerance() -> u8 {
    32
}
fn enabled() -> bool {
    true
}
fn brush_radius() -> f64 {
    8.0
}
fn selection_smoothness() -> f64 {
    8.0
}
params!(WandSelection {
    x: i64,
    y: i64,
    #[serde(default = "wand_tolerance")]
    tolerance: u8,
    #[serde(default = "enabled")]
    contiguous: bool,
    #[serde(default)]
    sample: SelectionSample,
    #[serde(default)]
    mode: SelectionMode
});
params!(QuickSelection {
    foreground: Vec<[f64; 2]>,
    #[serde(default)] background: Vec<[f64; 2]>,
    #[serde(default = "brush_radius")] radius: f64,
    #[serde(default = "selection_smoothness")] smoothness: f64,
    #[serde(default)] sample: SelectionSample,
    #[serde(default)] mode: SelectionMode
});
params!(AddMask {
    layer: LayerRef,
    #[serde(default)]
    source: MaskPreset
});
params!(EnableMask {
    layer: LayerRef,
    enabled: bool
});
params!(AddAdjustment {
    name: String,
    #[serde(rename = "type")]
    adjustment_type: AdjustmentType,
    #[serde(default)]
    stops: f64,
    #[serde(default)]
    mask: MaskPreset
});
params!(SetExposure {
    layer: LayerRef,
    stops: f64
});
params!(FillOpacity {
    layer: LayerRef,
    fill_opacity: f64
});
params!(SetLocks {
    layer: LayerRef,
    locks: crate::LayerLocks
});
params!(Clipping {
    layer: LayerRef,
    clipped: bool
});
params!(PassThrough {
    layer: LayerRef,
    pass_through: bool
});
params!(SetStyles {
    layer: LayerRef,
    styles: crate::styles::LayerStyles
});
params!(RasterizeTarget {
    layer: LayerRef,
    #[serde(default)]
    bake_backdrop: bool
});
params!(PixelTarget{#[serde(default)]layer:Option<LayerRef>});
params!(ClipboardResource { resource: String });
#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AlignMode {
    Left,
    HorizontalCenter,
    Right,
    Top,
    VerticalCenter,
    Bottom,
}
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AlignReference {
    #[default]
    Selection,
    Artboard,
}
params!(AlignLayers{layers:Vec<LayerRef>,mode:AlignMode,#[serde(default)] reference:AlignReference});
#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Distribution {
    HorizontalCenters,
    VerticalCenters,
    HorizontalSpacing,
    VerticalSpacing,
}
params!(DistributeLayers{layers:Vec<LayerRef>,mode:Distribution});
#[derive(Serialize, JsonSchema)]
pub struct MultiLayerResult {
    pub layer_ids: Vec<crate::LayerId>,
}
#[derive(Serialize, JsonSchema)]
pub struct ClipboardCopyResult {
    pub clipboard_resource: String,
    pub status: String,
    pub undoable: bool,
}
#[derive(Serialize, JsonSchema)]
pub struct ClipboardWriteResult {
    pub status: String,
    pub undoable: bool,
}
params!(SetColorLabel{layer:LayerRef,color_label:Option<crate::ColorLabel>});
params!(GroupLayers{layers:Vec<LayerRef>,name:String});
params!(CreateGroup { name: String });
params!(SolidFill {
    name: String,
    color: [u16; 4]
});
params!(GradientFill {
    name: String,
    start: [u16; 4],
    end: [u16; 4],
    #[serde(default)]
    angle: f64,
    #[serde(default = "unit_scale")]
    scale: f64,
    #[serde(default)]
    radial: bool
});
fn unit_scale() -> f64 {
    1.0
}
params!(SetSolidFill {
    layer: LayerRef,
    color: [u16; 4]
});
params!(SetGradientFill {
    layer: LayerRef,
    start: [u16; 4],
    end: [u16; 4],
    angle: f64,
    scale: f64,
    #[serde(default)]
    radial: bool
});
params!(HueSaturationParams {
    layer: LayerRef,
    hue: f64,
    saturation: f64,
    lightness: f64
});
params!(Reparent{layer:LayerRef,parent:Option<LayerRef>,#[serde(default)] index:Option<usize>});

fn parse<T: serde::de::DeserializeOwned>(params: Value) -> Result<T, CommandError> {
    serde_json::from_value(params).map_err(|e| CommandError::Invalid(e.to_string()))
}
