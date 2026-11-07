//! Portable document truth, command execution, history and failure-safe files.
mod adjustments;
mod blending;
pub mod commands;
pub mod composite;
mod document;
pub mod geometry;
pub mod geometry_masks;
pub mod geometry_pixels;
mod geometry_resize;
mod ids;
mod layer;
mod project;
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
pub use ids::{DocumentId, LayerId, StateId};
pub use layer::{
    BlendMode, ColorLabel, GeneratedCrop, Layer, LayerInfo, LayerKind, LayerLocks, LayerMaskInfo,
    LayerRef,
};
pub use subject::{CanvasPreview, SubjectParams, SubjectSelection, SubjectSelector};
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
