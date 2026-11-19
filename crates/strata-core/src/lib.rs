//! Portable document truth, command execution, history and failure-safe files.
mod adjustments;
mod blending;
pub mod commands;
pub mod composite;
mod document;
mod fill_geometry;
pub mod geometry;
pub mod geometry_masks;
pub mod geometry_pixels;
mod geometry_resize;
mod guides;
mod ids;
mod layer;
mod project;
mod psd;
mod recovery;
pub mod styles;
mod subject;
pub mod typography;
pub use adjustments::{GradientMapStop, LevelRange, TonalAdjustment};
pub use commands::{ClipboardImage, Editor, Source};
pub use document::{
    Document, DocumentCandidate, DocumentError, Frame, MAX_DOCUMENT_BYTES, MAX_FLAT_IMAGE_BYTES,
    MAX_LAYERS,
};
pub use fill_geometry::GeneratedGeometry;
pub use guides::{Guide, GuideAxis, GuideRef, MAX_GUIDES, Presentation, RulerMetrics, RulerUnit};
pub use ids::{DocumentId, GuideId, LayerId, StateId};
pub use layer::{
    BlendMode, ColorLabel, GeneratedCrop, Layer, LayerInfo, LayerKind, LayerLocks, LayerMaskInfo,
    LayerRef,
};
pub use subject::{CanvasPreview, SubjectParams, SubjectSelection, SubjectSelector};
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
