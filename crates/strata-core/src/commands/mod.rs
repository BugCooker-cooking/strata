mod arrange;
mod brush;
mod cancellation;
mod canvas;
mod clone_source;
mod crop_delete;
mod executor;
mod export;
mod filters;
mod generated;
mod heal;
mod local;
mod new;
mod orthogonal;
mod paint;
mod pixel_transform;
mod profiles;
mod raster;
mod registry;
mod resample;
mod resize_image;
mod rotate_image;
mod sample_selection;
mod selection;
mod spot_heal;
mod straighten;
mod tonal;
mod tonal_brush;
mod transform;
mod transform_selection;
mod tree;
mod trim;
pub use cancellation::WorkCancellation;
pub use executor::{
    BrushInteraction, ClipboardImage, CommandError, Control, CropInteraction, Editor,
    ExposureInteraction, GeometryInteraction, HistoryInfo, MarqueeInteraction, MoveInteraction,
    PaintInteraction, PreparedCommand, Request, Snapshot, Source, StraightenInteraction,
    TonalInteraction,
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
#[derive(Clone, Debug, Serialize, JsonSchema, PartialEq)]
pub struct PaintColors {
    pub foreground: [f64; 4],
    pub background: [f64; 4],
    pub version: u64,
}
impl Default for PaintColors {
    fn default() -> Self {
        Self {
            foreground: [0., 0., 0., 1.],
            background: [1., 1., 1., 1.],
            version: 0,
        }
    }
}
params!(SetPaintColors {
    #[serde(default)]
    foreground: Option<[f64;4]>,
    #[serde(default)]
    background: Option<[f64;4]>,
    #[serde(default)]
    expected_version: Option<u64>
});
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CanvasAnchor {
    TopLeft,
    Top,
    TopRight,
    Left,
    #[default]
    Center,
    Right,
    BottomLeft,
    Bottom,
    BottomRight,
}
params!(CanvasSize {
    #[schemars(range(min = 1, max = 100000))]
    width: u32,
    #[schemars(range(min = 1, max = 100000))]
    height: u32,
    #[serde(default)]
    anchor: CanvasAnchor
});
fn trim_side() -> bool {
    true
}
params!(TrimCanvas {
    #[serde(default = "trim_side")]
    top: bool,
    #[serde(default = "trim_side")]
    bottom: bool,
    #[serde(default = "trim_side")]
    left: bool,
    #[serde(default = "trim_side")]
    right: bool
});
#[derive(Serialize, JsonSchema)]
pub struct TrimCanvasResult {
    pub previous_frame: crate::Frame,
    pub frame: crate::Frame,
    pub changed: bool,
    pub preserved_external_content: bool,
    pub selection_rebound: bool,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CropPixels {
    Preserve,
    Delete,
}
params!(CropCanvas {
    rect: strata_pixels::CoverageRect,
    pixels: CropPixels,
    #[serde(default)]
    aspect_ratio: Option<[u32;2]>
});
#[derive(Serialize, JsonSchema)]
pub struct CanvasGeometryResult {
    pub previous_frame: crate::Frame,
    pub frame: crate::Frame,
    pub preserved_external_content: bool,
    pub selection_rebound: bool,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, JsonSchema)]
pub enum ImageOrientation {
    #[serde(rename = "rotate_90_cw")]
    Rotate90Cw,
    #[serde(rename = "rotate_90_ccw")]
    Rotate90Ccw,
    #[serde(rename = "rotate_180")]
    Rotate180,
    #[serde(rename = "flip_horizontal")]
    FlipHorizontal,
    #[serde(rename = "flip_vertical")]
    FlipVertical,
}
params!(OrientImage {
    operation: ImageOrientation
});
#[derive(Serialize, JsonSchema)]
pub struct OrientImageResult {
    pub operation: ImageOrientation,
    pub previous_frame: crate::Frame,
    pub frame: crate::Frame,
    pub all_stored_content_transformed: bool,
    pub preserved_layer_structure: bool,
}
params!(RotateImage {
    #[schemars(range(min=-360000,max=360000))]
    angle_degrees: f64,
    #[serde(default)]
    interpolation: TransformInterpolation
});
#[derive(Serialize, JsonSchema)]
pub struct RotateImageResult {
    pub angle_degrees: f64,
    pub previous_frame: crate::Frame,
    pub frame: crate::Frame,
    pub changed: bool,
    pub source_artboard_clipped: bool,
    pub preserved_layer_structure: bool,
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TransformInterpolation {
    Nearest,
    #[default]
    Bilinear,
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ResizeInterpolation {
    Nearest,
    Bilinear,
    #[default]
    Area,
}
params!(ResizeImage {
    #[schemars(range(min = 1, max = 100000))]
    width: u32,
    #[schemars(range(min = 1, max = 100000))]
    height: u32,
    #[serde(default)]
    interpolation: ResizeInterpolation
});
#[derive(Serialize, JsonSchema)]
pub struct ResizeImageResult {
    pub previous_frame: crate::Frame,
    pub frame: crate::Frame,
    pub interpolation: ResizeInterpolation,
    pub changed: bool,
    pub source_artboard_clipped: bool,
    pub preserved_layer_structure: bool,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum StraightenAxis {
    Horizontal,
    Vertical,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum StraightenReference {
    Angle {
        #[schemars(range(min=-45,max=45))]
        angle_degrees: f64,
    },
    Line {
        start: [f64; 2],
        end: [f64; 2],
        align: StraightenAxis,
    },
}
fn straighten_crop() -> bool {
    true
}
params!(StraightenImage {
    reference: StraightenReference,
    #[serde(default = "straighten_crop")]
    crop: bool,
    #[serde(default)]
    interpolation: TransformInterpolation
});
#[derive(Serialize, JsonSchema)]
pub struct StraightenImageResult {
    pub angle_degrees: f64,
    pub previous_frame: crate::Frame,
    pub expanded_frame: crate::Frame,
    pub frame: crate::Frame,
    pub cropped: bool,
    pub changed: bool,
    pub preserved_external_content: bool,
    pub preserved_layer_structure: bool,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TransformScope {
    WholeLayer,
    Selection,
}
fn transform_scale() -> [f64; 2] {
    [1.0; 2]
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum TransformGeometry {
    Matrix {
        values: [f64; 9],
    },
    Affine {
        values: [f64; 6],
    },
    Corners {
        source: [[f64; 2]; 4],
        destination: [[f64; 2]; 4],
    },
    Components {
        #[serde(default)]
        translation: [f64; 2],
        #[serde(default = "transform_scale")]
        scale: [f64; 2],
        #[serde(default)]
        rotation_degrees: f64,
        #[serde(default)]
        shear_degrees: [f64; 2],
        #[serde(default)]
        pivot: [f64; 2],
    },
}
params!(TransformApply {
    layer: LayerRef,
    scope: TransformScope,
    geometry: TransformGeometry,
    #[serde(default)]
    interpolation: TransformInterpolation
});
#[derive(Serialize, JsonSchema)]
pub struct TransformResult {
    pub layer_id: crate::LayerId,
    pub scope: TransformScope,
    pub output_bounds: Option<strata_pixels::CoverageRect>,
    pub source_artboard_clipped: bool,
    pub mask_transformed: bool,
    pub selection_transformed: bool,
}
#[derive(Serialize, JsonSchema)]
pub struct LayerBoundsResult {
    pub layer_id: crate::LayerId,
    pub bounds: Option<strata_pixels::CoverageRect>,
}
fn filter_sigma() -> f64 {
    2.0
}
fn unsharp_amount() -> f64 {
    1.0
}
fn noise_amount() -> f64 {
    0.02
}
params!(GaussianBlur {
    layer: LayerRef,
    #[serde(default = "filter_sigma")]
    #[schemars(range(min = 0.1, max = 64.0))]
    sigma: f64
});
params!(UnsharpMask {
    layer: LayerRef,
    #[serde(default = "filter_sigma")]
    #[schemars(range(min = 0.1, max = 64.0))]
    sigma: f64,
    #[serde(default = "unsharp_amount")]
    #[schemars(range(min = 0.0, max = 5.0))]
    amount: f64,
    #[serde(default)]
    #[schemars(range(min = 0.0, max = 1.0))]
    threshold: f64
});
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum NoiseDistribution {
    #[default]
    Uniform,
    Gaussian,
}
params!(AddNoise {
    layer: LayerRef,
    #[serde(default = "noise_amount")]
    #[schemars(range(min = 0.0, max = 1.0))]
    amount: f64,
    #[serde(default)]
    distribution: NoiseDistribution,
    #[serde(default)]
    monochrome: bool,
    #[serde(default)]
    seed: u32
});
#[derive(Serialize, JsonSchema)]
pub struct ColorResult {
    pub working_space: String,
}
params!(ConvertColor {
    #[schemars(with = "ProfileSpace")]
    space: String
});
#[derive(Clone, Copy, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProfileSpace {
    Srgb,
    DisplayP3,
}
params!(AssignProfile {
    space: ProfileSpace
});
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
params!(AddTonal {
    name: String,
    adjustment: crate::TonalAdjustment
});
params!(SetTonal {
    layer: LayerRef,
    adjustment: crate::TonalAdjustment
});
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BrushTarget {
    #[default]
    Pixels,
    Mask,
}
fn brush_size() -> f64 {
    24.0
}
fn brush_hardness() -> f64 {
    0.8
}
fn brush_spacing() -> f64 {
    0.2
}
fn brush_full() -> f64 {
    1.0
}
fn brush_color() -> [f64; 4] {
    [0.0, 0.0, 0.0, 1.0]
}
params!(BrushPoint {
    x: f64,
    y: f64,
    #[serde(default = "brush_full")]
    pressure: f64
});
params!(BrushStroke{
    layer:LayerRef,
    #[serde(default)]target:BrushTarget,
    points:Vec<BrushPoint>,
    #[serde(default="brush_size")]size:f64,
    #[serde(default="brush_hardness")]hardness:f64,
    #[serde(default="brush_full")]opacity:f64,
    #[serde(default="brush_full")]flow:f64,
    #[serde(default="brush_spacing")]spacing:f64,
    #[serde(default="brush_color")]color:[f64;4],
    #[serde(default="brush_full")]value:f64,
    #[serde(default="enabled")]pressure_size:bool,
    #[serde(default="enabled")]pressure_opacity:bool
    ,#[serde(default)]clone_source:Option<CloneSource>
});
fn sample_current() -> SelectionSample {
    SelectionSample::CurrentLayer
}
params!(CloneSource{source:[f64;2],#[serde(default)]destination:Option<[f64;2]>,#[serde(default="sample_current")]sample:SelectionSample});
fn heal_iterations() -> u32 {
    512
}
fn heal_tolerance() -> f64 {
    0.00005
}
params!(HealStroke {
    stroke: BrushStroke,
    #[serde(default = "heal_iterations")]
    max_iterations: u32,
    #[serde(default = "heal_tolerance")]
    tolerance: f64
});
#[derive(Serialize, JsonSchema)]
pub struct HealResult {
    pub layer_id: crate::LayerId,
    pub domain_pixels: usize,
    pub iterations: u32,
    pub residual_rms: f64,
    pub method: String,
}
fn spot_radius() -> u32 {
    64
}
params!(SpotHealStroke {
    stroke: BrushStroke,
    #[serde(default = "sample_current")]
    sample: SelectionSample,
    #[serde(default = "spot_radius")]
    search_radius: u32,
    #[serde(default = "heal_iterations")]
    max_iterations: u32,
    #[serde(default = "heal_tolerance")]
    tolerance: f64
});
#[derive(Serialize, JsonSchema)]
pub struct SpotHealResult {
    pub layer_id: crate::LayerId,
    pub source: Option<[f64; 2]>,
    pub candidates: usize,
    pub boundary_error: f64,
    pub healing: Option<HealResult>,
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BrushTonalRange {
    Shadows,
    #[default]
    Midtones,
    Highlights,
}
fn tonal_exposure() -> f64 {
    0.5
}
params!(TonalBrushStroke {
    stroke: BrushStroke,
    #[serde(default)]
    range: BrushTonalRange,
    #[serde(default = "tonal_exposure")]
    exposure: f64,
    #[serde(default = "enabled")]
    protect_tones: bool
});
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SpongeMode {
    Saturate,
    #[default]
    Desaturate,
}
params!(SpongeStroke {
    stroke: BrushStroke,
    #[serde(default)]
    mode: SpongeMode,
    #[serde(default = "tonal_exposure")]
    amount: f64,
    #[serde(default = "enabled")]
    vibrance: bool
});
#[derive(Serialize, JsonSchema)]
pub struct BrushResult {
    pub layer_id: crate::LayerId,
    pub target: BrushTarget,
    pub touched_tiles: usize,
}
params!(SampleColor {
    x: i64,
    y: i64,
    #[serde(default)]
    radius: u32,
    #[serde(default)]
    sample: SelectionSample
});
#[derive(Serialize, JsonSchema)]
pub struct SampleColorResult {
    pub color: [f64; 4],
    pub bit_depth: u8,
    pub profile: String,
    pub sampled_pixels: usize,
}
params!(FillPixels {
    layer: LayerRef,
    color: [f64; 4],
    #[serde(default = "brush_full")]
    opacity: f64
});
params!(BucketPixels {
    layer: LayerRef,
    x: i64,
    y: i64,
    color: [f64; 4],
    #[serde(default = "brush_full")]
    opacity: f64,
    #[serde(default = "wand_tolerance")]
    tolerance: u8,
    #[serde(default = "enabled")]
    contiguous: bool,
    #[serde(default)]
    sample: SelectionSample
});
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PaintGradientShape {
    #[default]
    Linear,
    Radial,
    Angle,
    Reflected,
    Diamond,
}
params!(PaintGradientStop {
    at: f64,
    color: [f64; 4]
});
params!(GradientPixels{layer:LayerRef,start:[f64;2],end:[f64;2],stops:Vec<PaintGradientStop>,#[serde(default)]shape:PaintGradientShape,#[serde(default)]reverse:bool,#[serde(default="brush_full")]opacity:f64});
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
