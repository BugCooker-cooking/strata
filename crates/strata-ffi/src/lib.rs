//! UniFFI host facade. Private bootstrap APIs are replaced by commands in M1.

use std::{
    collections::VecDeque,
    path::Path,
    sync::{Arc, Mutex},
};
use strata_core::{DocumentInfo, Engine, ImageDocument};
use strata_pixels::PixelBuffer;
use strata_render::Renderer;
use strata_render::Viewport;

uniffi::setup_scaffolding!();

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct ImageInfo {
    pub width: u32,
    pub height: u32,
    pub bit_depth: u8,
    pub source_profile: String,
    pub profile_fallback: bool,
}
impl From<&DocumentInfo> for ImageInfo {
    fn from(info: &DocumentInfo) -> Self {
        Self {
            width: info.width,
            height: info.height,
            bit_depth: info.bit_depth,
            source_profile: info.source_profile.clone(),
            profile_fallback: info.profile_fallback,
        }
    }
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct FrameInfo {
    pub cpu_ms: f32,
    pub skipped: bool,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct CanvasViewport {
    pub width: u32,
    pub height: u32,
    pub scale: f32,
}
impl From<CanvasViewport> for Viewport {
    fn from(v: CanvasViewport) -> Self {
        Self {
            width: v.width,
            height: v.height,
            scale: v.scale,
        }
    }
}

#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum EngineEventDto {
    DocumentOpened { info: ImageInfo, generation: u64 },
}

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum StrataError {
    #[error("file not found: {path}")]
    NotFound { path: String },
    #[error("unsupported image: {message}")]
    UnsupportedFormat { message: String },
    #[error("decode failed: {message}")]
    Decode { message: String },
    #[error("color conversion failed: {message}")]
    Color { message: String },
    #[error("render failed: {message}")]
    Render { message: String },
    #[error("request cancelled: {message}")]
    Cancelled { message: String },
    #[error("engine unavailable: {message}")]
    Internal { message: String },
    #[error("no canvas attached")]
    NoCanvas,
}

#[uniffi::export(with_foreign)]
pub trait EngineListener: Send + Sync {
    fn on_event(&self, event: EngineEventDto);
}

#[derive(Default)]
struct Inner {
    engine: Engine,
    renderer: Option<Renderer>,
    open_generation: u64,
    events: VecDeque<EngineEventDto>,
    delivering: bool,
}

#[derive(uniffi::Object)]
pub struct StrataEngine {
    inner: Mutex<Inner>,
    listener: Arc<dyn EngineListener>,
}

struct OpenCandidate {
    document: ImageDocument,
    display: PixelBuffer,
}

fn prepare_document(path: &Path) -> Result<OpenCandidate, StrataError> {
    let decoded = strata_io::decode_image(path).map_err(|e| match e {
        strata_io::IoError::NotFound(p) => StrataError::NotFound {
            path: p.to_string_lossy().into_owned(),
        },
        strata_io::IoError::Unsupported { reason, .. } => {
            StrataError::UnsupportedFormat { message: reason }
        }
        e => StrataError::Decode {
            message: e.to_string(),
        },
    })?;
    let converted = strata_color::to_display_p3(
        &decoded.pixels,
        decoded.width,
        decoded.height,
        decoded.icc_profile.as_deref(),
    )
    .map_err(|e| StrataError::Color {
        message: e.to_string(),
    })?;
    let document = ImageDocument::new(
        decoded.width,
        decoded.height,
        decoded.pixels,
        decoded.icc_profile,
        converted.source_profile,
        converted.profile_fallback,
    )
    .map_err(|e| StrataError::Decode {
        message: e.to_string(),
    })?;
    Ok(OpenCandidate {
        document,
        display: converted.pixels,
    })
}

#[uniffi::export]
impl StrataEngine {
    #[uniffi::constructor]
    pub fn new(listener: Arc<dyn EngineListener>) -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(Inner::default()),
            listener,
        })
    }
    pub fn core_version(&self) -> String {
        strata_core::version().into()
    }
    pub fn document_info(&self) -> Option<ImageInfo> {
        let inner = self.inner.lock().ok()?;
        inner.engine.document().map(|d| ImageInfo::from(d.info()))
    }
    pub fn open_image(&self, path: String) -> Result<ImageInfo, StrataError> {
        let generation = self.begin_open()?;
        let candidate = prepare_document(Path::new(&path))?;
        self.commit_open(generation, candidate)
    }
    pub fn render_frame(&self) -> Result<FrameInfo, StrataError> {
        let mut inner = self.lock()?;
        let frame = inner
            .renderer
            .as_mut()
            .ok_or(StrataError::NoCanvas)?
            .render()
            .map_err(|e| StrataError::Render {
                message: e.to_string(),
            })?;
        Ok(FrameInfo {
            cpu_ms: frame.cpu_ms,
            skipped: frame.skipped,
        })
    }

    /// Trusted native-host bridge only; never exposed as an editing/AI command.
    pub fn attach_canvas(
        &self,
        metal_layer_ptr: u64,
        viewport: CanvasViewport,
    ) -> Result<(), StrataError> {
        #[cfg(target_os = "macos")]
        {
            // SAFETY: CanvasView holds CAMetalLayer strongly until synchronous detach.
            // Calls are serialized by Inner; Swift must not supply arbitrary pointers.
            let mut renderer = unsafe {
                Renderer::new_for_metal_layer(
                    metal_layer_ptr as *mut std::ffi::c_void,
                    viewport.into(),
                )
            }
            .map_err(|e| StrataError::Render {
                message: e.to_string(),
            })?;
            let mut inner = self.lock()?;
            if let Some(doc) = inner.engine.document() {
                let display = strata_color::to_display_p3(
                    doc.pixels(),
                    doc.info().width,
                    doc.info().height,
                    doc.icc_profile(),
                )
                .map_err(|e| StrataError::Color {
                    message: e.to_string(),
                })?;
                let prepared = renderer
                    .prepare_image(doc.info().width, doc.info().height, &display.pixels)
                    .map_err(|e| StrataError::Render {
                        message: e.to_string(),
                    })?;
                renderer
                    .commit_image(prepared)
                    .map_err(|e| StrataError::Render {
                        message: e.to_string(),
                    })?;
            }
            inner.renderer = Some(renderer);
            Ok(())
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (metal_layer_ptr, viewport);
            Err(StrataError::Render {
                message: "native surface is available on macOS only".into(),
            })
        }
    }

    pub fn detach_canvas(&self) -> Result<(), StrataError> {
        let mut inner = self.lock()?;
        inner.renderer = None;
        Ok(())
    }
    pub fn resize_canvas(&self, viewport: CanvasViewport) -> Result<(), StrataError> {
        if let Some(renderer) = &mut self.lock()?.renderer {
            renderer
                .resize(viewport.into())
                .map_err(|e| StrataError::Render {
                    message: e.to_string(),
                })?;
        }
        Ok(())
    }
    pub fn pan_by(&self, dx: f32, dy: f32) -> Result<(), StrataError> {
        if let Some(renderer) = &mut self.lock()?.renderer {
            let mut view = renderer.view();
            view.pan_by(dx, dy);
            renderer.set_view(view).map_err(|e| StrataError::Render {
                message: e.to_string(),
            })?;
        }
        Ok(())
    }
    pub fn zoom_at(&self, x: f32, y: f32, factor: f32) -> Result<(), StrataError> {
        if let Some(renderer) = &mut self.lock()?.renderer {
            let mut view = renderer.view();
            view.zoom_at(renderer.viewport(), x, y, factor);
            renderer.set_view(view).map_err(|e| StrataError::Render {
                message: e.to_string(),
            })?;
        }
        Ok(())
    }
    pub fn fit_to_screen(&self) -> Result<(), StrataError> {
        if let Some(renderer) = &mut self.lock()?.renderer {
            renderer.fit_to_screen();
        }
        Ok(())
    }
}

impl StrataEngine {
    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Inner>, StrataError> {
        self.inner.lock().map_err(|_| StrataError::Internal {
            message: "state lock poisoned".into(),
        })
    }
    fn begin_open(&self) -> Result<u64, StrataError> {
        let mut inner = self.lock()?;
        inner.open_generation =
            inner
                .open_generation
                .checked_add(1)
                .ok_or_else(|| StrataError::Internal {
                    message: "request generation exhausted".into(),
                })?;
        Ok(inner.open_generation)
    }
    fn commit_open(
        &self,
        generation: u64,
        candidate: OpenCandidate,
    ) -> Result<ImageInfo, StrataError> {
        let info = ImageInfo::from(candidate.document.info());
        {
            let mut inner = self.lock()?;
            if generation != inner.open_generation {
                return Err(StrataError::Cancelled {
                    message: "a newer open request superseded this one".into(),
                });
            }
            // GPU preparation is fallible; the old document/frame remain intact until this succeeds.
            if let Some(renderer) = &mut inner.renderer {
                let prepared = renderer
                    .prepare_image(info.width, info.height, &candidate.display)
                    .map_err(|e| StrataError::Render {
                        message: e.to_string(),
                    })?;
                renderer
                    .commit_image(prepared)
                    .map_err(|e| StrataError::Render {
                        message: e.to_string(),
                    })?;
            }
            inner.engine.set_document(candidate.document);
            inner.events.push_back(EngineEventDto::DocumentOpened {
                info: info.clone(),
                generation,
            });
        }
        self.deliver_events()?;
        Ok(info)
    }
    fn deliver_events(&self) -> Result<(), StrataError> {
        {
            let mut inner = self.lock()?;
            if inner.delivering {
                return Ok(());
            }
            inner.delivering = true;
        }
        loop {
            let event = {
                let mut inner = self.lock()?;
                match inner.events.pop_front() {
                    Some(e) => e,
                    None => {
                        inner.delivering = false;
                        return Ok(());
                    }
                }
            };
            // Serial notifications, outside the state lock. A listener may query or open again.
            // Listener failure cannot roll back an already committed document.
            if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                self.listener.on_event(event)
            }))
            .is_err()
            {
                eprintln!("Strata: event listener panicked after document commit");
            }
        }
    }
}

#[cfg(test)]
mod tests;
