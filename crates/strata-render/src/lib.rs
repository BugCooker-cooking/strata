//! Portable canvas transforms and GPU display rendering.

mod view;
pub use view::*;
mod renderer;
pub use renderer::{CANVAS_BG_SRGB, FrameStats, PreparedImage, Renderer};

#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error("no GPU adapter available")]
    NoAdapter,
    #[error("GPU device request failed: {0}")]
    RequestDevice(String),
    #[error("GPU surface error: {0}")]
    Surface(String),
    #[error("image {width}x{height} exceeds maximum texture dimension {max}")]
    ImageTooLarge { width: u32, height: u32, max: u32 },
    #[error(transparent)]
    Pixels(#[from] strata_pixels::PixelError),
    #[error("invalid viewport or view transform")]
    InvalidView,
    #[error("operation requires an offscreen target")]
    NotOffscreen,
    #[error("GPU operation failed: {0}")]
    Gpu(String),
}

pub fn check_texture_fits(width: u32, height: u32, max: u32) -> Result<(), RenderError> {
    if width == 0 || height == 0 {
        return Err(strata_pixels::PixelError::ZeroDimensions.into());
    }
    if width > max || height > max {
        return Err(RenderError::ImageTooLarge { width, height, max });
    }
    Ok(())
}

#[cfg(test)]
mod tests;
