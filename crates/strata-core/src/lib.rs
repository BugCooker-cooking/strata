//! Portable document truth, command execution, history and failure-safe files.
pub mod commands;
pub mod composite;
mod document;
mod ids;
mod layer;
mod project;
pub use commands::{Editor, Source};
pub use document::{
    Document, DocumentCandidate, DocumentError, Frame, MAX_DOCUMENT_BYTES, MAX_LAYERS,
};
pub use ids::{DocumentId, LayerId, StateId};
pub use layer::{BlendMode, Layer, LayerInfo, LayerRef};
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
