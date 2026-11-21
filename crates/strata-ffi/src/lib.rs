//! Trusted native host facade. All content operations dispatch registered commands.
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    path::Path,
    sync::{Arc, Mutex},
};
use strata_core::{
    Document, Editor, Source,
    commands::{CommandError, MoveInteraction, PreparedCommand},
};
use strata_render::{Renderer, Viewport};
uniffi::setup_scaffolding!();
mod ai;
mod clipboard;
mod crop;
mod filter_preview;
#[cfg(test)]
mod group_transform_tests;
mod guides;
mod local;
#[cfg(test)]
mod preferences_tests;
#[cfg(test)]
mod quick_export_tests;
#[cfg(test)]
mod revert_tests;
#[cfg(test)]
mod smart_guides_tests;
mod subject;
mod tonal_preview;
mod typography;
pub use clipboard::ClipboardTransfer;
pub use local::SelectionEdge;
mod thumbnails;
pub use thumbnails::LayerThumbnail;

/// Pure, document-independent geometry for a drag outline. The registered
/// shape commands use the identical source generator.
#[uniffi::export]
pub fn shape_outline_json(geometry_json: String) -> Result<String, StrataError> {
    if geometry_json.len() > 4096 {
        return Err(render_error("shape parameters too large"));
    }
    let g: strata_core::ShapeGeometry =
        serde_json::from_str(&geometry_json).map_err(render_error)?;
    serde_json::to_string(&g.outline().map_err(render_error)?).map_err(render_error)
}
/// Process resource maintenance must not keep any document engine alive.
#[uniffi::export]
pub fn trim_global_source_memory() -> Result<(), StrataError> {
    strata_pixels::memory::MemoryPool::global()
        .trim_sources()
        .map_err(render_error)
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct ImageInfo {
    pub width: u32,
    pub height: u32,
    pub bit_depth: u8,
    pub source_profile: String,
    pub profile_fallback: bool,
}
impl From<&Document> for ImageInfo {
    fn from(d: &Document) -> Self {
        Self {
            width: d.frame().width,
            height: d.frame().height,
            bit_depth: d.bit_depth(),
            source_profile: d.profile_name().into(),
            profile_fallback: d.profile_fallback(),
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
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct ArtboardOverlay {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub name: String,
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
    DocumentOpened {
        info: ImageInfo,
        generation: u64,
    },
    StateChanged {
        snapshot_json: String,
    },
    ExportPresetChanged,
    PreferencesChanged,
    DocumentDuplicated {
        name: String,
        activate: bool,
        copy: Arc<DuplicatedDocument>,
    },
}

#[derive(uniffi::Object)]
pub struct DuplicatedDocument(Mutex<Option<Editor>>);

impl std::fmt::Debug for DuplicatedDocument {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DuplicatedDocument(..)")
    }
}
impl PartialEq for DuplicatedDocument {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
    }
}

#[uniffi::export]
impl DuplicatedDocument {
    /// The trusted workspace attaches its own listener and takes ownership once.
    pub fn take_engine(
        &self,
        listener: Arc<dyn EngineListener>,
    ) -> Result<Arc<StrataEngine>, StrataError> {
        let editor = self
            .0
            .lock()
            .map_err(render_error)?
            .take()
            .ok_or_else(|| render_error("the copy already belongs to a document tab"))?;
        Ok(StrataEngine::from_inner(
            Inner {
                editor,
                ..Inner::default()
            },
            listener,
        ))
    }
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
    #[error("command failed: {message}")]
    Command { message: String },
    #[error("unsaved document changes")]
    UnsavedChanges,
    #[error("no canvas attached")]
    NoCanvas,
}
fn command_error(e: CommandError) -> StrataError {
    match e {
        CommandError::UnsavedChanges => StrataError::UnsavedChanges,
        CommandError::Stale | CommandError::Expired => StrataError::Cancelled {
            message: e.to_string(),
        },
        CommandError::Document(strata_core::DocumentError::Cancelled) => StrataError::Cancelled {
            message: "composition cancelled".into(),
        },
        CommandError::Image(strata_io::IoError::NotFound(path)) => StrataError::NotFound {
            path: path.display().to_string(),
        },
        CommandError::Image(strata_io::IoError::Unsupported { reason, .. }) => {
            StrataError::UnsupportedFormat { message: reason }
        }
        CommandError::Image(e) => StrataError::Decode {
            message: e.to_string(),
        },
        CommandError::Document(strata_core::DocumentError::Color(e)) => StrataError::Color {
            message: e.to_string(),
        },
        _ => StrataError::Command {
            message: e.to_string(),
        },
    }
}
fn render_error(e: impl std::fmt::Display) -> StrataError {
    StrataError::Render {
        message: e.to_string(),
    }
}
#[uniffi::export(with_foreign)]
pub trait EngineListener: Send + Sync {
    fn on_event(&self, event: EngineEventDto);
}
struct Inner {
    editor: Editor,
    renderer: Option<Renderer>,
    events: VecDeque<EngineEventDto>,
    delivering: bool,
    open_generation: u64,
    interaction: Option<MoveInteraction>,
    marquee: Option<strata_core::commands::MarqueeInteraction>,
    exposure: Option<strata_core::commands::ExposureInteraction>,
    tonal_preview: Option<tonal_preview::TonalPreview>,
    brush: Option<strata_core::commands::BrushInteraction>,
    paint: Option<strata_core::commands::PaintInteraction>,
}
impl Default for Inner {
    fn default() -> Self {
        Self {
            editor: Editor::with_decoder(Arc::new(strata_platform::NativeDecoder))
                .with_app_preferences(native_app_preferences())
                .with_export_presets(native_export_presets()),
            renderer: None,
            events: VecDeque::new(),
            delivering: false,
            open_generation: 0,
            interaction: None,
            marquee: None,
            exposure: None,
            tonal_preview: None,
            brush: None,
            paint: None,
        }
    }
}
fn native_export_presets() -> strata_core::commands::ExportPresetStore {
    static PRESETS: std::sync::LazyLock<strata_core::commands::ExportPresetStore> =
        std::sync::LazyLock::new(Default::default);
    PRESETS.clone()
}
fn native_app_preferences() -> strata_core::commands::AppPreferencesStore {
    static PREFERENCES: std::sync::LazyLock<strata_core::commands::AppPreferencesStore> =
        std::sync::LazyLock::new(|| {
            strata_core::commands::AppPreferencesStore::new(
                strata_pixels::memory::MemoryPool::global(),
            )
        });
    PREFERENCES.clone()
}
#[derive(uniffi::Object)]
pub struct StrataEngine {
    inner: Mutex<Inner>,
    work_cancellation: strata_core::commands::WorkCancellation,
    active_ai_request: Mutex<Option<(uuid::Uuid, strata_core::commands::Request)>>,
    tonal_preview_sequence: std::sync::atomic::AtomicU64,
    preview_work: Mutex<strata_core::commands::WorkCancellation>,
    geometry_session: Mutex<Option<crop::NativeGeometry>>,
    listener: Arc<dyn EngineListener>,
    recovery: Mutex<Option<strata_io::recovery::RecoveryStore>>,
    ai: Mutex<Option<strata_ai::Session>>,
}

#[uniffi::export]
impl StrataEngine {
    pub fn working_profile_icc(&self) -> Vec<u8> {
        self.inner
            .lock()
            .ok()
            .and_then(|inner| {
                inner
                    .editor
                    .document()
                    .map(|doc| doc.working_icc().to_vec())
            })
            .unwrap_or_default()
    }
    /// Small host display copies. Never replace document samples or enter history.
    pub fn layer_thumbnails(&self, revision: u64) -> Result<Vec<LayerThumbnail>, StrataError> {
        let doc = {
            let inner = self.lock()?;
            if inner.editor.snapshot().revision != revision {
                return Ok(Vec::new());
            }
            inner.editor.document().cloned()
        };
        doc.as_ref()
            .map(thumbnails::make)
            .transpose()
            .map(|v| v.unwrap_or_default())
    }
    pub fn artboard_overlay(&self) -> Option<ArtboardOverlay> {
        let inner = self.inner.try_lock().ok()?;
        let renderer = inner.renderer.as_ref()?;
        let frame = inner.editor.document()?.frame();
        let view = renderer.view();
        let (x, y) = view.image_to_screen(renderer.viewport(), frame.x as f32, frame.y as f32);
        Some(ArtboardOverlay {
            x,
            y,
            width: frame.width as f32 * view.zoom,
            height: frame.height as f32 * view.zoom,
            name: frame.name.clone(),
        })
    }
    #[uniffi::constructor]
    pub fn new(listener: Arc<dyn EngineListener>) -> Arc<Self> {
        Self::from_inner(Inner::default(), listener)
    }
    pub fn configure_recovery(&self, directory: String) -> Result<(), StrataError> {
        let store =
            strata_io::recovery::RecoveryStore::new(Path::new(&directory)).map_err(render_error)?;
        *self.recovery.lock().map_err(render_error)? = Some(store);
        Ok(())
    }
    pub fn recovery_entries_json(&self) -> Result<String, StrataError> {
        let store = self.recovery.lock().map_err(render_error)?;
        let store = store
            .as_ref()
            .ok_or_else(|| render_error("recovery directory not configured"))?;
        serde_json::to_string(&store.list().map_err(render_error)?).map_err(render_error)
    }
    /// IO is serialized with cleanup; the live editor lock is held only while
    /// cloning immutable document/history indices, never for disk writes.
    pub fn checkpoint_recovery(&self, label: String) -> Result<String, StrataError> {
        let store = self.recovery.lock().map_err(render_error)?;
        let store = store
            .as_ref()
            .ok_or_else(|| render_error("recovery directory not configured"))?;
        let editor = self.lock()?.editor.clone();
        let info = editor
            .checkpoint_recovery(store, &label)
            .map_err(command_error)?;
        serde_json::to_string(&info).map_err(render_error)
    }
    pub fn clear_recovery_if_clean(&self) -> Result<(), StrataError> {
        let store = self.recovery.lock().map_err(render_error)?;
        let inner = self.lock()?;
        if !inner.editor.dirty()
            && let Some(store) = store.as_ref()
        {
            store
                .remove(&inner.editor.snapshot().session_id.to_string())
                .map_err(render_error)?;
        }
        Ok(())
    }
    pub fn discard_recovery(&self, id: Option<String>) -> Result<(), StrataError> {
        let store = self.recovery.lock().map_err(render_error)?;
        if let Some(store) = store.as_ref() {
            let id = match id {
                Some(id) => id,
                None => self.lock()?.editor.snapshot().session_id.to_string(),
            };
            store.remove(&id).map_err(render_error)?;
        }
        Ok(())
    }
    pub fn memory_stats_json(&self) -> String {
        json!({"limits":strata_pixels::memory::MemoryPool::global().limits(),"usage":strata_pixels::memory::MemoryPool::global().stats()}).to_string()
    }
    pub fn document_memory_json(&self) -> Result<String, StrataError> {
        let inner = self.lock()?;
        let Some(doc) = inner.editor.document() else {
            return Ok("null".into());
        };
        let mut versions = std::collections::HashSet::new();
        let mut referenced = 0u64;
        let mut tiles = 0usize;
        for layer in doc.layers() {
            referenced += layer.pixels().referenced_bytes();
            tiles += layer.pixels().tile_count();
            for coord in layer.pixels().tile_coords() {
                if let Some(version) = layer.pixels().tile_version(*coord) {
                    versions.insert(version);
                }
            }
        }
        Ok(json!({"layer_count":doc.layers().len(),"referenced_source_bytes":referenced,"source_tile_count":tiles,"unique_source_blocks":versions.len(),"history_entries":inner.editor.snapshot().history.len()}).to_string())
    }
    pub fn trim_memory(&self) -> Result<(), StrataError> {
        strata_pixels::memory::MemoryPool::global()
            .trim()
            .map_err(render_error)
    }
    pub fn trim_source_memory(&self) -> Result<(), StrataError> {
        trim_global_source_memory()
    }
    pub fn core_version(&self) -> String {
        strata_core::version().into()
    }
    pub fn document_info(&self) -> Option<ImageInfo> {
        self.inner
            .lock()
            .ok()?
            .editor
            .document()
            .map(ImageInfo::from)
    }
    pub fn snapshot_json(&self) -> Result<String, StrataError> {
        Ok(json!(self.lock()?.editor.snapshot()).to_string())
    }
    pub fn catalog_json(&self) -> String {
        serde_json::to_string(strata_core::commands::catalog()).expect("valid static catalog")
    }
    pub fn project_path(&self) -> Option<String> {
        self.inner
            .lock()
            .ok()?
            .editor
            .project_path()
            .map(|p| p.to_string_lossy().into_owned())
    }
    pub fn set_recent_files(&self, paths: Vec<String>) -> Result<(), StrataError> {
        self.lock()?
            .editor
            .set_recent_files(paths.into_iter().map(std::path::PathBuf::from).collect())
            .map_err(command_error)
    }
    /// Trusted host metadata only; models need a fresh read capability.
    pub fn revert_source_path(&self) -> Option<String> {
        self.inner.lock().ok().and_then(|i| {
            i.editor
                .revert_source()
                .map(|p| p.to_string_lossy().into_owned())
        })
    }
    pub fn grant_read(&self, path: String) -> Result<String, StrataError> {
        self.lock()?
            .editor
            .grant_read(Path::new(&path), Source::User)
            .map_err(|e| match &e {
                CommandError::Storage(strata_io::project::StorageError::Io(io))
                    if io.kind() == std::io::ErrorKind::NotFound =>
                {
                    StrataError::NotFound { path }
                }
                _ => command_error(e),
            })
    }
    pub fn grant_write(&self, path: String, overwrite: bool) -> Result<String, StrataError> {
        self.lock()?
            .editor
            .grant_write(Path::new(&path), overwrite, Source::User)
            .map_err(command_error)
    }
    /// JSON contains only command parameters; source and session are bound here.
    pub fn dispatch(&self, command: String, params_json: String) -> Result<String, StrataError> {
        if params_json.len() > 1024 * 1024 {
            return Err(StrataError::Command {
                message: "parameters exceed limit".into(),
            });
        }
        let params: Value =
            serde_json::from_str(&params_json).map_err(|e| StrataError::Command {
                message: e.to_string(),
            })?;
        if command.starts_with("paint.colors.") {
            // Palette is editor tool state. It must not invalidate a pixel
            // preview/crop or prepare renderer resources just to change a swatch.
            let result = {
                let mut inner = self.lock()?;
                let request = inner.editor.request(Source::User);
                let result = inner
                    .editor
                    .execute(request, &command, params, |_| Ok(()))
                    .map_err(command_error)?;
                if !matches!(
                    command.as_str(),
                    "paint.colors.get" | "paint.colors.describe"
                ) {
                    let snapshot_json = json!(inner.editor.snapshot()).to_string();
                    inner
                        .events
                        .push_back(EngineEventDto::StateChanged { snapshot_json });
                }
                result
            };
            self.deliver_events()?;
            return Ok(result.to_string());
        }
        if command.starts_with("view.")
            && !command.starts_with("view.guides.")
            && !command.starts_with("view.presentation.")
        {
            let mut inner = match self.inner.try_lock() {
                Ok(inner) => inner,
                Err(std::sync::TryLockError::WouldBlock) => {
                    return Err(StrataError::Cancelled {
                        message: "display preparation is busy; navigation input skipped".into(),
                    });
                }
                Err(error) => {
                    return Err(StrataError::Internal {
                        message: error.to_string(),
                    });
                }
            };
            let request = inner.editor.request(Source::User);
            let prepared = inner
                .editor
                .prepare(request, &command, params.clone())
                .map_err(command_error)?;
            return apply(&mut inner, prepared, &command, &params).map(|r| r.to_string());
        }
        if strata_core::commands::catalog()
            .iter()
            .find(|s| s.id == command)
            .is_some_and(|s| s.kind != strata_core::commands::CommandKind::Query)
        {
            self.issue_tonal_preview()?;
            let mut inner = self.lock()?;
            tonal_preview::restore(&mut inner)?;
        }
        let (snapshot, request, generation) = {
            let mut inner = self.lock()?;
            if matches!(command.as_str(), "file.open" | "file.revert") {
                inner.open_generation += 1;
            }
            (
                inner.editor.clone(),
                inner.editor.request(Source::User),
                inner.open_generation,
            )
        };
        // Decoding, composition and serialization do not hold the UI state lock.
        let prepared = snapshot
            .prepare(request, &command, params.clone())
            .map_err(command_error)?;
        let result = {
            let mut inner = self.lock()?;
            if matches!(command.as_str(), "file.open" | "file.revert")
                && generation != inner.open_generation
            {
                return Err(StrataError::Cancelled {
                    message: "newer open superseded request".into(),
                });
            }
            apply(&mut inner, prepared, &command, &params)?
        };
        self.deliver_events()?;
        if matches!(
            command.as_str(),
            "file.new" | "file.open" | "file.recover" | "file.revert"
        ) && let Err(error) = self.discard_recovery(None)
        {
            eprintln!("recovery cleanup after document replacement failed: {error}");
        }
        Ok(result.to_string())
    }
    /// Convenience host API, implemented by the registered file.open command.
    pub fn open_image(&self, path: String) -> Result<ImageInfo, StrataError> {
        let resource = self.grant_read(path)?;
        self.dispatch("file.open".into(), json!({"resource":resource}).to_string())?;
        self.document_info().ok_or(StrataError::Internal {
            message: "missing committed document".into(),
        })
    }
    pub fn render_frame(&self) -> Result<FrameInfo, StrataError> {
        let mut inner = match self.inner.try_lock() {
            Ok(inner) => inner,
            Err(std::sync::TryLockError::WouldBlock) => {
                return Ok(FrameInfo {
                    cpu_ms: 0.0,
                    skipped: true,
                });
            }
            Err(error) => {
                return Err(StrataError::Internal {
                    message: error.to_string(),
                });
            }
        };
        let frame = inner
            .renderer
            .as_mut()
            .ok_or(StrataError::NoCanvas)?
            .render()
            .map_err(render_error)?;
        Ok(FrameInfo {
            cpu_ms: frame.cpu_ms,
            skipped: frame.skipped,
        })
    }
    pub fn attach_canvas(
        &self,
        metal_layer_ptr: u64,
        viewport: CanvasViewport,
    ) -> Result<(), StrataError> {
        self.issue_tonal_preview()?;
        #[cfg(target_os = "macos")]
        {
            // SAFETY: native CanvasView owns and retains CAMetalLayer until detach.
            let mut renderer = unsafe {
                Renderer::new_for_metal_layer(
                    metal_layer_ptr as *mut std::ffi::c_void,
                    viewport.into(),
                )
            }
            .map_err(render_error)?;
            let mut inner = self.lock()?;
            tonal_preview::restore(&mut inner)?;
            if let Some(doc) = inner.editor.document() {
                let prepared = renderer.prepare_document(doc, true).map_err(render_error)?;
                renderer.commit_document(prepared).map_err(render_error)?;
            }
            inner.renderer = Some(renderer);
            Ok(())
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (metal_layer_ptr, viewport);
            Err(render_error("native canvas only available on macOS"))
        }
    }
    pub fn detach_canvas(&self) -> Result<(), StrataError> {
        if let Some(crop) = self.geometry_session.lock().map_err(render_error)?.take() {
            crop.cancel();
        }
        self.issue_tonal_preview()?;
        let mut inner = self.lock()?;
        inner.interaction = None;
        inner.marquee = None;
        inner.exposure = None;
        inner.tonal_preview = None;
        inner.brush = None;
        inner.paint = None;
        inner.renderer = None;
        Ok(())
    }
    pub fn resize_canvas(&self, viewport: CanvasViewport) -> Result<(), StrataError> {
        if let Some(renderer) = &mut self.lock()?.renderer {
            renderer.resize(viewport.into()).map_err(render_error)?;
        }
        Ok(())
    }
    pub fn set_canvas_background(&self, red: u8, green: u8, blue: u8) -> Result<(), StrataError> {
        if let Some(renderer) = &mut self.lock()?.renderer {
            renderer.set_background([red, green, blue]);
        }
        Ok(())
    }
    pub fn pan_by(&self, dx: f32, dy: f32) -> Result<(), StrataError> {
        self.dispatch("view.pan".into(), json!({"dx":dx,"dy":dy}).to_string())
            .map(|_| ())
    }
    pub fn zoom_at(&self, x: f32, y: f32, factor: f32) -> Result<(), StrataError> {
        self.dispatch(
            "view.zoom".into(),
            json!({"x":x,"y":y,"factor":factor}).to_string(),
        )
        .map(|_| ())
    }
    pub fn fit_to_screen(&self) -> Result<(), StrataError> {
        self.dispatch("view.fit".into(), "{}".into()).map(|_| ())
    }
    pub fn begin_layer_move(&self, layer_id: String) -> Result<(), StrataError> {
        self.issue_tonal_preview()?;
        let reference =
            serde_json::from_value(json!({"id":layer_id})).map_err(|e| StrataError::Command {
                message: e.to_string(),
            })?;
        let mut inner = self.lock()?;
        tonal_preview::restore(&mut inner)?;
        inner.interaction = Some(inner.editor.begin_move(reference).map_err(command_error)?);
        Ok(())
    }
    pub fn preview_layer_move(&self, x: i64, y: i64) -> Result<(), StrataError> {
        let mut inner = self.lock()?;
        let gesture = inner.interaction.as_ref().ok_or(StrataError::Cancelled {
            message: "no gesture".into(),
        })?;
        let candidate = inner
            .editor
            .preview_move(gesture, x, y)
            .map_err(command_error)?;
        if let Some(renderer) = &mut inner.renderer {
            let scene = renderer
                .prepare_document(candidate.document().unwrap(), false)
                .map_err(render_error)?;
            renderer.commit_document(scene).map_err(render_error)?;
        }
        Ok(())
    }
    pub fn end_layer_move(&self, x: i64, y: i64, cancel: bool) -> Result<(), StrataError> {
        {
            let mut inner = self.lock()?;
            let gesture = inner.interaction.take().ok_or(StrataError::Cancelled {
                message: "no gesture".into(),
            })?;
            if cancel {
                let doc = inner.editor.document().cloned();
                if let (Some(doc), Some(renderer)) = (doc, &mut inner.renderer) {
                    let scene = renderer
                        .prepare_document(&doc, false)
                        .map_err(render_error)?;
                    renderer.commit_document(scene).map_err(render_error)?;
                }
            } else {
                let candidate = inner
                    .editor
                    .preview_move(&gesture, x, y)
                    .map_err(command_error)?;
                apply(&mut inner, candidate, "layer.set_position", &json!({}))?;
            }
        }
        self.deliver_events()
    }
    pub fn canvas_zoom(&self) -> f32 {
        self.inner
            .try_lock()
            .ok()
            .and_then(|i| i.renderer.as_ref().map(|r| r.view().zoom))
            .unwrap_or(1.0)
    }
}
fn apply(
    inner: &mut Inner,
    prepared: PreparedCommand,
    command: &str,
    params: &Value,
) -> Result<Value, StrataError> {
    let opened = prepared.replaces_document();
    let replay = prepared.is_replay();
    let scene = if prepared.changes_display() {
        if let (Some(doc), Some(renderer)) = (prepared.document(), &mut inner.renderer) {
            Some(
                renderer
                    .prepare_document_cancellable(doc, opened, &|| prepared.work_expired())
                    .map_err(|error| {
                        if matches!(error, strata_render::RenderError::Cancelled) {
                            StrataError::Cancelled {
                                message: error.to_string(),
                            }
                        } else {
                            render_error(error)
                        }
                    })?,
            )
        } else {
            None
        }
    } else {
        None
    };
    // Same mutex covers core commit and renderer swap. The prepared scene belongs
    // to this renderer; swapping it performs no fallible GPU work.
    let old_revision = inner.editor.revision();
    let (result, copy) = inner
        .editor
        .commit_with_document_copy(prepared)
        .map_err(command_error)?;
    if let Some(copy) = copy {
        inner.events.push_back(EngineEventDto::DocumentDuplicated {
            name: copy.name,
            activate: !matches!(copy.source, Source::Ai { .. }),
            copy: Arc::new(DuplicatedDocument(Mutex::new(Some(copy.editor)))),
        });
        return Ok(result);
    }
    if inner.editor.revision() != old_revision
        && let (Some(scene), Some(renderer)) = (scene, &mut inner.renderer)
    {
        renderer.commit_document(scene).map_err(render_error)?;
    }
    if !replay && let Some(renderer) = &mut inner.renderer {
        match command {
            "view.pan" => {
                let mut view = renderer.view();
                view.pan_by(
                    params["dx"].as_f64().unwrap_or(0.0) as f32,
                    params["dy"].as_f64().unwrap_or(0.0) as f32,
                );
                renderer.set_view(view).map_err(render_error)?;
            }
            "view.zoom" => {
                let mut view = renderer.view();
                view.zoom_at(
                    renderer.viewport(),
                    params["x"].as_f64().unwrap_or(0.0) as f32,
                    params["y"].as_f64().unwrap_or(0.0) as f32,
                    params["factor"].as_f64().unwrap_or(1.0) as f32,
                );
                renderer.set_view(view).map_err(render_error)?;
            }
            "view.fit" => renderer.fit_to_screen(),
            "view.actual_size" => {
                let mut view = renderer.view();
                view.zoom = 1.0;
                renderer.set_view(view).map_err(render_error)?;
            }
            _ => {}
        }
    }
    if opened && let Some(doc) = inner.editor.document() {
        inner.events.push_back(EngineEventDto::DocumentOpened {
            info: ImageInfo::from(doc),
            generation: inner.open_generation,
        });
    }
    if command == "file.export_preset.set" {
        inner.events.push_back(EngineEventDto::ExportPresetChanged);
    }
    if command == "system.preferences.set" {
        inner.events.push_back(EngineEventDto::PreferencesChanged);
    }
    if !command.starts_with("view.")
        || command.starts_with("view.guides.")
        || command == "view.presentation.set"
    {
        inner.events.push_back(EngineEventDto::StateChanged {
            snapshot_json: json!(inner.editor.snapshot()).to_string(),
        });
    }
    Ok(result)
}
impl StrataEngine {
    fn from_inner(inner: Inner, listener: Arc<dyn EngineListener>) -> Arc<Self> {
        let work_cancellation = inner.editor.work_cancellation();
        Arc::new(Self {
            inner: Mutex::new(inner),
            work_cancellation,
            active_ai_request: Mutex::new(None),
            tonal_preview_sequence: std::sync::atomic::AtomicU64::new(0),
            preview_work: Mutex::new(strata_core::commands::WorkCancellation::default()),
            geometry_session: Mutex::new(None),
            listener,
            recovery: Mutex::new(None),
            ai: Mutex::new(None),
        })
    }
    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Inner>, StrataError> {
        self.inner.lock().map_err(|_| StrataError::Internal {
            message: "state lock poisoned".into(),
        })
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
            if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                self.listener.on_event(event)
            }))
            .is_err()
            {
                eprintln!("Strata listener failed after commit");
            }
        }
    }
}
#[cfg(test)]
mod tests;
