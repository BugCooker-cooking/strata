//! Fixed, verified subject segmentation assets and portable ONNX inference.
//! No automatic network access; the host explicitly downloads and authorizes.
pub mod assets;
mod inference;
pub use inference::{ExecutionBackend, Segmentation, SubjectModel};

#[derive(Debug, thiserror::Error)]
pub enum MlError {
    #[error("subject model is not installed; approve its download in the model panel")]
    Missing,
    #[error("subject model operation cancelled")]
    Cancelled,
    #[error("invalid subject model input or output: {0}")]
    Invalid(String),
    #[error("model asset checksum mismatch")]
    Checksum,
    #[error("subject model runtime: {0}")]
    Runtime(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}
