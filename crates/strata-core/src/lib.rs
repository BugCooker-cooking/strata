//! Portable document truth, command execution, history and failure-safe files.
pub mod commands;
pub mod composite;
mod document;
mod ids;
mod layer;
mod project;
mod recovery;
mod subject;
pub use commands::{Editor, Source};
pub use document::{
    Document, DocumentCandidate, DocumentError, Frame, MAX_DOCUMENT_BYTES, MAX_FLAT_IMAGE_BYTES,
    MAX_LAYERS,
};
pub use ids::{DocumentId, LayerId, StateId};
pub use layer::{BlendMode, Layer, LayerInfo, LayerKind, LayerMaskInfo, LayerRef};
pub use subject::{CanvasPreview, SubjectParams, SubjectSelection, SubjectSelector};
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
