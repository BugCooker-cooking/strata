use super::{registry, *};
use crate::{
    Document, DocumentCandidate, DocumentError, DocumentId, Layer, LayerId, StateId,
    document::tile_limit,
};
use serde_json::json;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::{
    collections::{HashMap, HashSet},
    time::{Duration, Instant},
};
use strata_pixels::{PixelBuffer, PixelFormat, TiledPixels};
use uuid::Uuid;
#[path = "files.rs"]
mod files;
use files::{RecentFile, Resource, WriteCandidate};
#[path = "clipboard.rs"]
mod clipboard;
pub use clipboard::ClipboardImage;
use clipboard::{ClipCandidate, ClipResource};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Source {
    User,
    Ai { turn_id: Uuid },
    Test,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Control {
    User,
    Running { turn_id: Uuid },
    Paused { turn_id: Uuid },
}

/// Created by a trusted host, never deserialized from model-provided JSON.
#[derive(Clone, Debug)]
pub struct Request {
    pub call_id: Uuid,
    source: Source,
    session: Uuid,
    document: Option<DocumentId>,
    revision: u64,
    issued: Instant,
    deadline: Instant,
    cancellation: Arc<AtomicBool>,
    local_cancellation: Option<Arc<AtomicBool>>,
    paint_colors_version: u64,
    presentation_version: u64,
    snap_context: Option<super::guides::SnapContext>,
}

impl Request {
    /// Cancel this request's generation without touching a newer editor turn.
    pub fn cancel(&self) {
        self.cancellation.store(true, Ordering::Release);
    }
    /// Trusted callers may shorten a request's commit deadline. Model parameters
    /// cannot construct or extend it. Checked again after fallible preparation.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.deadline = self
            .deadline
            .min(Instant::now() + timeout.min(Duration::from_secs(3600)));
        self
    }
    fn expired(&self) -> bool {
        Instant::now() >= self.deadline
            || self.cancellation.load(Ordering::Acquire)
            || self
                .local_cancellation
                .as_ref()
                .is_some_and(|flag| flag.load(Ordering::Acquire))
    }
}

/// A cancellable preview anchored to one document revision and control epoch.
/// Dropping this value cancels the gesture without touching content/history.
pub struct MoveInteraction {
    request: Request,
    epoch: u64,
    layer: LayerId,
}
pub struct MarqueeInteraction {
    request: Request,
    epoch: u64,
}
#[derive(Clone)]
pub struct GeometryInteraction {
    request: Request,
    epoch: u64,
    command: &'static str,
    transform_target: Option<(LayerId, TransformScope)>,
    transform_roots: Vec<LayerId>,
    guide_target: Option<crate::GuideId>,
}
pub type CropInteraction = GeometryInteraction;
pub type StraightenInteraction = GeometryInteraction;
impl GeometryInteraction {
    pub fn with_work_cancellation(&self, work: &super::WorkCancellation) -> Self {
        let mut value = self.clone();
        value.request.local_cancellation = Some(work.token());
        value
    }
}
pub struct ExposureInteraction {
    request: Request,
    epoch: u64,
    layer: LayerId,
}
#[derive(Clone)]
pub struct TonalInteraction {
    request: Request,
    epoch: u64,
    layer: LayerId,
    active: Option<LayerId>,
}
impl TonalInteraction {
    /// A preview calculation may be superseded without revoking its original
    /// document context or any unrelated editor request.
    pub fn with_work_cancellation(&self, work: &super::WorkCancellation) -> Self {
        let mut copy = self.clone();
        copy.request.local_cancellation = Some(work.token());
        copy
    }
}
#[derive(Clone)]
pub struct BrushInteraction {
    request: Request,
    epoch: u64,
    layer: LayerId,
    target: BrushTarget,
    command: String,
}
impl BrushInteraction {
    pub fn command(&self) -> &str {
        &self.command
    }
}
#[derive(Clone)]
pub struct PaintInteraction {
    request: Request,
    epoch: u64,
    layer: LayerId,
    command: String,
}
impl PaintInteraction {
    pub fn command(&self) -> &str {
        &self.command
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CommandError {
    #[error("invalid command: {0}")]
    Invalid(String),
    #[error("control locked")]
    ControlLocked,
    #[error("stale document, revision or control epoch")]
    Stale,
    #[error("call ID reused with different arguments")]
    CallConflict,
    #[error("request expired")]
    Expired,
    #[error("no document")]
    NoDocument,
    #[error("unsaved changes require an explicit decision")]
    UnsavedChanges,
    #[error("preparation failed: {0}")]
    Preparation(String),
    #[error(transparent)]
    Document(#[from] DocumentError),
    #[error(transparent)]
    Storage(#[from] strata_io::project::StorageError),
    #[error(transparent)]
    Image(#[from] strata_io::IoError),
    #[error("resource permission denied or expired")]
    Permission,
}

#[derive(Clone, Serialize, JsonSchema)]
pub struct HistoryInfo {
    pub command: String,
    pub source: SourceInfo,
    pub turn_id: Option<Uuid>,
}
#[derive(Clone, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SourceInfo {
    User,
    Ai,
    Test,
}

#[derive(Clone)]
struct HistoryEntry {
    info: HistoryInfo,
    before: Document,
    after: Document,
}

#[derive(Serialize, JsonSchema)]
pub struct Snapshot {
    pub ruler_metrics: Option<crate::RulerMetrics>,
    pub presentation: crate::Presentation,
    pub guides: Vec<crate::Guide>,
    pub paint_colors: super::PaintColors,
    pub selection: Option<strata_pixels::MaskLayout>,
    pub selection_extent: Option<strata_pixels::CoverageRect>,
    pub selection_extent_conservative: bool,
    pub selection_summary_available: bool,
    pub resolution_dpi: Option<u32>,
    pub preset: Option<String>,
    pub session_id: Uuid,
    pub document_id: Option<DocumentId>,
    pub revision: u64,
    pub dirty: bool,
    pub frame: Option<crate::Frame>,
    pub color_mode: Option<ColorMode>,
    pub bit_depth: Option<u8>,
    pub profile_name: Option<String>,
    pub profile_fallback: bool,
    pub layers: Vec<crate::LayerInfo>,
    pub active_layer: Option<LayerId>,
    pub history: Vec<HistoryInfo>,
    pub history_cursor: usize,
    pub executions: Vec<Value>,
    pub control_events: Vec<Value>,
    pub control: Control,
}

#[derive(Clone, Copy, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ColorMode {
    Rgb,
}

enum Action {
    Preferences(u64, super::AppPreferences),
    TrimMemory,
    Duplicate {
        name: String,
        index_budget: Arc<strata_pixels::memory::Reservation>,
    },
    ExportPreset(u64, super::ExportPreset),
    Presentation(crate::Presentation),
    PaintColors(super::PaintColors),
    Query,
    View,
    New,
    Edit,
    Place(String),
    History(usize),
    Open(Option<std::path::PathBuf>, std::path::PathBuf),
    Recover,
    External(Box<WriteCandidate>),
    ClipboardCopy(Box<ClipCandidate>),
    ClipboardEdit(String),
}

/// An opaque staged transaction. The host can prepare display resources before
/// commit; no public method can mutate its candidate.
pub struct PreparedCommand {
    annotations_only: bool,
    read_resource: Option<String>,
    request: Request,
    command: String,
    params: Value,
    epoch: u64,
    candidate: Option<Document>,
    action: Action,
    result: Value,
    selected: Option<LayerId>,
    replay: bool,
    required_active_layer: Option<LayerId>,
    required_export_preset: Option<u64>,
    required_revert_source: Option<std::path::PathBuf>,
}
impl PreparedCommand {
    /// Display overlays read the exact validated command result.
    pub fn result(&self) -> &Value {
        &self.result
    }
    pub fn work_expired(&self) -> bool {
        self.request.expired()
    }
    pub fn is_replay(&self) -> bool {
        self.replay
    }
    pub fn document(&self) -> Option<&Document> {
        self.candidate.as_ref()
    }
    pub fn changes_display(&self) -> bool {
        matches!(
            self.action,
            Action::New
                | Action::Open(..)
                | Action::Recover
                | Action::Edit
                | Action::Place(_)
                | Action::History(_)
                | Action::ClipboardEdit(_)
        ) && !self.replay
            && !self.annotations_only
            && !self.command.starts_with("select.")
    }
    pub fn replaces_document(&self) -> bool {
        matches!(
            self.action,
            Action::New | Action::Open(..) | Action::Recover
        )
    }
}

#[derive(Clone)]
struct Receipt {
    view: bool,
    source: Source,
    document: Option<DocumentId>,
    command: String,
    params: Value,
    result: Value,
    issued: Instant,
}

#[derive(Clone)]
pub struct Editor {
    preferences: super::AppPreferencesStore,
    copy_index_budget: Option<Arc<strata_pixels::memory::Reservation>>,
    export_presets: super::ExportPresetStore,
    presentation: crate::Presentation,
    paint_colors: super::PaintColors,
    pool: strata_pixels::memory::MemoryPool,
    pub(crate) document: Option<Document>,
    session: Uuid,
    revision: u64,
    saved: Option<StateId>,
    selected: Option<LayerId>,
    history: Vec<HistoryEntry>,
    cursor: usize,
    control: Control,
    epoch: u64,
    receipts: HashMap<Uuid, Receipt>,
    pub(crate) project_path: Option<std::path::PathBuf>,
    opened_path: Option<std::path::PathBuf>,
    resources: HashMap<String, Resource>,
    clipboards: HashMap<String, ClipResource>,
    recent: Vec<RecentFile>,
    executions: Vec<Value>,
    control_events: Vec<Value>,
    decoder: Option<std::sync::Arc<dyn strata_io::PlatformDecoder>>,
    selector: Option<Arc<dyn crate::SubjectSelector>>,
    cancellation: WorkCancellation,
}
impl Default for Editor {
    fn default() -> Self {
        Self::new()
    }
}
impl Editor {
    pub fn new() -> Self {
        let pool = strata_pixels::memory::MemoryPool::global();
        Self {
            preferences: super::AppPreferencesStore::new(pool.clone()),
            copy_index_budget: None,
            export_presets: Default::default(),
            presentation: Default::default(),
            paint_colors: Default::default(),
            pool,
            document: None,
            session: Uuid::new_v4(),
            revision: 0,
            saved: None,
            selected: None,
            history: vec![],
            cursor: 0,
            control: Control::User,
            epoch: 0,
            receipts: HashMap::new(),
            project_path: None,
            opened_path: None,
            resources: HashMap::new(),
            clipboards: HashMap::new(),
            recent: Vec::new(),
            executions: Vec::new(),
            control_events: Vec::new(),
            decoder: None,
            selector: None,
            cancellation: WorkCancellation::default(),
        }
    }
    pub fn with_decoder(decoder: std::sync::Arc<dyn strata_io::PlatformDecoder>) -> Self {
        Self {
            decoder: Some(decoder),
            ..Self::new()
        }
    }
    /// Trusted app bootstrap shares one preset across independent document engines.
    pub fn with_export_presets(mut self, store: super::ExportPresetStore) -> Self {
        self.export_presets = store;
        self
    }
    /// Attach a shared settings store before opening the first document.
    pub fn with_app_preferences(mut self, store: super::AppPreferencesStore) -> Self {
        assert!(
            self.document.is_none(),
            "attach preferences before opening a document"
        );
        self.pool = store.pool();
        self.preferences = store;
        self
    }
    pub fn app_preferences(&self) -> Result<super::AppPreferences, CommandError> {
        self.preferences.get()
    }
    pub fn set_subject_selector(&mut self, selector: Arc<dyn crate::SubjectSelector>) {
        self.selector = Some(selector);
    }
    pub fn clear_subject_selector(&mut self) {
        self.cancel_pending_work();
        self.selector = None;
    }
    pub fn with_memory_pool(pool: strata_pixels::memory::MemoryPool) -> Self {
        Self {
            preferences: super::AppPreferencesStore::new(pool.clone()),
            pool,
            ..Self::new()
        }
    }
    fn decode_image(
        &self,
        path: &std::path::Path,
    ) -> Result<(strata_io::DecodedImage, strata_pixels::memory::Reservation), CommandError> {
        let bytes = match &self.decoder {
            Some(decoder) => decoder.decoded_bytes(path, strata_io::DEFAULT_DECODE_LIMIT)?,
            None => strata_io::decoded_bytes(path, strata_io::DEFAULT_DECODE_LIMIT)?,
        };
        let allocation = self
            .pool
            .reserve(
                strata_pixels::memory::MemoryKind::Transient,
                bytes * 3 + 16 * 1024 * 1024,
            )
            .map_err(DocumentError::from)?;
        let image = match &self.decoder {
            Some(decoder) => decoder.decode(path, strata_io::DEFAULT_DECODE_LIMIT),
            None => strata_io::decode_image(path),
        }?;
        Ok((image, allocation))
    }
    pub fn document(&self) -> Option<&Document> {
        self.document.as_ref()
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn project_path(&self) -> Option<&std::path::Path> {
        self.project_path.as_deref()
    }
    /// Trusted host metadata; this is not a model file-access capability.
    pub fn revert_source(&self) -> Option<&std::path::Path> {
        self.project_path.as_deref().or(self.opened_path.as_deref())
    }
    pub fn control(&self) -> &Control {
        &self.control
    }
    pub fn dirty(&self) -> bool {
        self.document
            .as_ref()
            .is_some_and(|d| Some(d.state_id()) != self.saved)
    }
    pub fn request(&self, source: Source) -> Request {
        Request {
            presentation_version: self.presentation.version,
            snap_context: None,
            paint_colors_version: self.paint_colors.version,
            call_id: Uuid::new_v4(),
            source,
            session: self.session,
            document: self.document.as_ref().map(Document::id),
            revision: self.revision,
            issued: Instant::now(),
            deadline: Instant::now() + Duration::from_secs(3600),
            cancellation: self.cancellation.token(),
            local_cancellation: None,
        }
    }
    pub fn begin_move(&self, layer: LayerRef) -> Result<MoveInteraction, CommandError> {
        self.authorize(&Source::User, "layer.set_position", CommandKind::Mutate)?;
        let layer = self
            .document
            .as_ref()
            .ok_or(CommandError::NoDocument)?
            .resolve(&layer)?;
        Ok(MoveInteraction {
            request: self.request(Source::User),
            epoch: self.epoch,
            layer,
        })
    }
    pub fn preview_move(
        &self,
        interaction: &MoveInteraction,
        x: i64,
        y: i64,
    ) -> Result<PreparedCommand, CommandError> {
        if interaction.epoch != self.epoch {
            return Err(CommandError::Stale);
        }
        self.prepare(
            interaction.request.clone(),
            "layer.set_position",
            json!({"layer":{"id":interaction.layer},"x":x,"y":y}),
        )
    }
    pub fn begin_marquee(&self) -> Result<MarqueeInteraction, CommandError> {
        self.authorize(&Source::User, "select.marquee", CommandKind::Mutate)?;
        self.document.as_ref().ok_or(CommandError::NoDocument)?;
        Ok(MarqueeInteraction {
            request: self.request(Source::User),
            epoch: self.epoch,
        })
    }
    pub fn begin_crop(&self) -> Result<CropInteraction, CommandError> {
        self.begin_geometry("image.crop")
    }
    pub fn begin_straighten(&self) -> Result<StraightenInteraction, CommandError> {
        self.begin_geometry("image.straighten")
    }
    fn begin_geometry(&self, command: &'static str) -> Result<GeometryInteraction, CommandError> {
        self.authorize(&Source::User, command, registry::kind(command)?)?;
        self.document.as_ref().ok_or(CommandError::NoDocument)?;
        Ok(GeometryInteraction {
            request: self.request(Source::User),
            epoch: self.epoch,
            command,
            transform_target: None,
            transform_roots: Vec::new(),
            guide_target: None,
        })
    }
    /// Native origin drafts own the same request/epoch as other geometry,
    /// but commit only the existing editor-local view command.
    pub fn begin_ruler_origin(&self) -> Result<GeometryInteraction, CommandError> {
        self.begin_geometry("view.presentation.set")
    }
    pub fn prepare_ruler_origin(
        &self,
        interaction: &GeometryInteraction,
        origin: [f64; 2],
    ) -> Result<PreparedCommand, CommandError> {
        self.prepare_geometry(
            interaction,
            "view.presentation.set",
            json!({"ruler_origin":origin,"expected_version":interaction.request.presentation_version}),
        )
    }
    pub fn begin_canvas_transform(
        &self,
        layer: LayerRef,
        scope: TransformScope,
    ) -> Result<GeometryInteraction, CommandError> {
        let mut interaction = self.begin_geometry("transform.apply")?;
        let doc = self.document().ok_or(CommandError::NoDocument)?;
        let (target, _) = super::transform::target(doc, &layer)?;
        if doc.active_layer() != Some(target) || !doc.effective_visibility(target) {
            return Err(CommandError::Invalid(
                "canvas transform must start on the active visible layer".into(),
            ));
        }
        interaction.transform_target = Some((target, scope));
        Ok(interaction)
    }
    pub fn begin_canvas_transform_layers(
        &self,
        layers: &[LayerRef],
    ) -> Result<GeometryInteraction, CommandError> {
        let mut interaction = self.begin_geometry("transform.apply_layers")?;
        let doc = self.document().ok_or(CommandError::NoDocument)?;
        let active = doc.active_layer().ok_or(CommandError::Stale)?;
        interaction.transform_target = Some((active, TransformScope::WholeLayer));
        interaction.transform_roots = super::transform::roots(doc, layers)?;
        Ok(interaction)
    }
    pub fn begin_guide(
        &self,
        guide: Option<crate::GuideRef>,
    ) -> Result<GeometryInteraction, CommandError> {
        let command = if guide.is_some() {
            "view.guides.update"
        } else {
            "view.guides.add"
        };
        let mut interaction = self.begin_geometry(command)?;
        if self.presentation.guides_locked {
            return Err(CommandError::Invalid("guides are locked".into()));
        }
        if let Some(guide) = guide {
            let doc = self.document().ok_or(CommandError::NoDocument)?;
            interaction.guide_target = Some(doc.guides[super::guides::resolve(doc, &guide)?].id);
        }
        Ok(interaction)
    }
    pub fn prepare_guide(
        &self,
        interaction: &GeometryInteraction,
        params: Value,
    ) -> Result<PreparedCommand, CommandError> {
        if interaction.command == "view.guides.update" {
            let p: UpdateGuide = parse(params.clone())?;
            let doc = self.document().ok_or(CommandError::NoDocument)?;
            if Some(doc.guides[super::guides::resolve(doc, &p.guide)?].id)
                != interaction.guide_target
            {
                return Err(CommandError::Stale);
            }
        } else if interaction.command != "view.guides.add" {
            return Err(CommandError::Stale);
        }
        self.prepare_geometry(interaction, interaction.command, params)
    }
    pub fn canvas_transform_bounds(
        &self,
        interaction: &GeometryInteraction,
    ) -> Result<LayerBoundsResult, CommandError> {
        let (layer, scope) = self.check_canvas_transform(interaction)?;
        let doc = self.document().ok_or(CommandError::NoDocument)?;
        if !interaction.transform_roots.is_empty() {
            let bounds = super::transform_group::visible_bounds_of(
                doc,
                &interaction.transform_roots,
                &|| interaction.request.expired(),
            )?;
            return Ok(LayerBoundsResult {
                layer_id: layer,
                bounds,
            });
        }
        super::transform::canvas_bounds(doc, &LayerRef::Id(layer), scope, &|| {
            interaction.request.expired()
        })
    }
    pub fn canvas_transform_context_matches(&self, interaction: &GeometryInteraction) -> bool {
        self.check_canvas_transform(interaction).is_ok()
    }
    fn check_canvas_transform(
        &self,
        interaction: &GeometryInteraction,
    ) -> Result<(LayerId, TransformScope), CommandError> {
        self.check_request(&interaction.request)?;
        let (layer, scope) = interaction.transform_target.ok_or(CommandError::Stale)?;
        if !matches!(
            interaction.command,
            "transform.apply" | "transform.apply_layers"
        ) || interaction.epoch != self.epoch
            || self.document().and_then(Document::active_layer) != Some(layer)
        {
            return Err(CommandError::Stale);
        }
        Ok((layer, scope))
    }
    pub fn prepare_canvas_transform(
        &self,
        interaction: &GeometryInteraction,
        params: Value,
    ) -> Result<PreparedCommand, CommandError> {
        let (layer, scope) = self.check_canvas_transform(interaction)?;
        let doc = self.document().ok_or(CommandError::NoDocument)?;
        let matches_target = if interaction.command == "transform.apply_layers" {
            let parsed: TransformLayers = parse(params.clone())?;
            super::transform::roots(doc, &parsed.layers)? == interaction.transform_roots
        } else {
            let parsed: TransformApply = parse(params.clone())?;
            doc.resolve(&parsed.layer)? == layer && parsed.scope == scope
        };
        if !matches_target {
            return Err(CommandError::Invalid(
                "canvas transform layer and scope are anchored to their initial values".into(),
            ));
        }
        let mut prepared = self.prepare_geometry(interaction, interaction.command, params)?;
        prepared.required_active_layer = Some(layer);
        Ok(prepared)
    }
    pub fn prepare_crop(
        &self,
        interaction: &CropInteraction,
        params: Value,
    ) -> Result<PreparedCommand, CommandError> {
        self.prepare_geometry(interaction, "image.crop", params)
    }
    pub fn prepare_straighten(
        &self,
        interaction: &StraightenInteraction,
        params: Value,
    ) -> Result<PreparedCommand, CommandError> {
        self.prepare_geometry(interaction, "image.straighten", params)
    }
    fn prepare_geometry(
        &self,
        interaction: &GeometryInteraction,
        command: &str,
        params: Value,
    ) -> Result<PreparedCommand, CommandError> {
        if interaction.epoch != self.epoch || interaction.command != command {
            return Err(CommandError::Stale);
        }
        self.prepare(interaction.request.clone(), command, params)
    }
    pub fn prepare_marquee(
        &self,
        interaction: &MarqueeInteraction,
        params: Value,
    ) -> Result<PreparedCommand, CommandError> {
        self.prepare_selection(interaction, "select.marquee", params)
    }
    pub fn prepare_selection(
        &self,
        interaction: &MarqueeInteraction,
        command: &str,
        params: Value,
    ) -> Result<PreparedCommand, CommandError> {
        if !matches!(
            command,
            "select.marquee"
                | "select.ellipse"
                | "select.polygon"
                | "select.lasso"
                | "select.wand"
                | "select.quick"
        ) {
            return Err(CommandError::Invalid(
                "not a supported selection gesture".into(),
            ));
        }
        if interaction.epoch != self.epoch {
            return Err(CommandError::Stale);
        }
        self.prepare(interaction.request.clone(), command, params)
    }
    pub fn begin_exposure(&self, layer: LayerRef) -> Result<ExposureInteraction, CommandError> {
        self.authorize(&Source::User, "adjust.exposure.set", CommandKind::Mutate)?;
        let doc = self.document.as_ref().ok_or(CommandError::NoDocument)?;
        let layer = doc.resolve(&layer)?;
        if !doc
            .layers()
            .iter()
            .any(|l| l.id() == layer && matches!(l.info().kind, crate::LayerKind::Exposure { .. }))
        {
            return Err(CommandError::Invalid(
                "target must be an exposure layer".into(),
            ));
        }
        Ok(ExposureInteraction {
            request: self.request(Source::User),
            epoch: self.epoch,
            layer,
        })
    }
    pub fn preview_exposure(
        &self,
        interaction: &ExposureInteraction,
        stops: f64,
    ) -> Result<PreparedCommand, CommandError> {
        if interaction.epoch != self.epoch {
            return Err(CommandError::Stale);
        }
        self.prepare(
            interaction.request.clone(),
            "adjust.exposure.set",
            json!({"layer":{"id":interaction.layer},"stops":stops}),
        )
    }
    pub fn begin_tonal(&self, layer: LayerRef) -> Result<TonalInteraction, CommandError> {
        self.authorize(&Source::User, "adjust.tonal.set", CommandKind::Mutate)?;
        let doc = self.document().ok_or(CommandError::NoDocument)?;
        let layer = doc.resolve(&layer)?;
        if !doc.layers().iter().any(|l| {
            l.id() == layer && matches!(l.info().kind, crate::LayerKind::Adjustment { .. })
        }) {
            return Err(CommandError::Invalid(
                "target must be a tonal adjustment layer".into(),
            ));
        }
        if doc
            .layers()
            .iter()
            .any(|l| l.info().locks.all && (l.id() == layer || doc.is_descendant(layer, l.id())))
        {
            return Err(CommandError::Invalid(
                "tonal preview target or its parent is locked".into(),
            ));
        }
        Ok(TonalInteraction {
            active: doc.active_layer(),
            request: self.request(Source::User),
            epoch: self.epoch,
            layer,
        })
    }
    pub fn begin_filter(
        &self,
        layer: LayerRef,
        command: &str,
    ) -> Result<TonalInteraction, CommandError> {
        if !matches!(
            command,
            "filter.gaussian_blur"
                | "filter.unsharp_mask"
                | "filter.add_noise"
                | "filter.high_pass"
                | "transform.apply"
                | "image.auto"
                | "layer.snap_position"
        ) {
            return Err(CommandError::Invalid(
                "unsupported pixel filter preview".into(),
            ));
        }
        self.authorize(&Source::User, command, CommandKind::Mutate)?;
        let doc = self.document().ok_or(CommandError::NoDocument)?;
        let layer = if command == "transform.apply" {
            super::transform::target(doc, &layer)?.0
        } else if command == "layer.snap_position" {
            let target = doc.resolve(&layer)?;
            super::guides::check_snap_target(doc, target)?;
            target
        } else {
            super::pixel_transform::target(doc, &layer)?.0
        };
        Ok(TonalInteraction {
            active: doc.active_layer(),
            request: self.request(Source::User),
            epoch: self.epoch,
            layer,
        })
    }
    pub fn preview_filter(
        &self,
        interaction: &TonalInteraction,
        command: &str,
        mut params: Value,
    ) -> Result<PreparedCommand, CommandError> {
        if !self.tonal_context_matches(interaction)
            || self.document().and_then(Document::active_layer) != interaction.active
        {
            return Err(CommandError::Stale);
        }
        if !matches!(
            command,
            "filter.gaussian_blur"
                | "filter.unsharp_mask"
                | "filter.add_noise"
                | "filter.high_pass"
                | "transform.apply"
                | "image.auto"
                | "layer.snap_position"
        ) {
            return Err(CommandError::Invalid(
                "unsupported pixel filter preview".into(),
            ));
        }
        let fields = params
            .as_object_mut()
            .ok_or_else(|| CommandError::Invalid("filter parameters must be an object".into()))?;
        let layer = json!({"id":interaction.layer});
        if fields.get("layer").is_some_and(|value| *value != layer) {
            return Err(CommandError::Invalid(
                "filter preview target is anchored to its initial layer".into(),
            ));
        }
        fields.insert("layer".into(), layer);
        let mut prepared = self.prepare(interaction.request.clone(), command, params)?;
        prepared.required_active_layer = interaction.active;
        Ok(prepared)
    }
    /// Trusted cached source extent, measured outside the editor/render lock.
    pub fn measure_snap_preview(
        &self,
        interaction: &TonalInteraction,
    ) -> Result<TonalInteraction, CommandError> {
        self.measure_snap_preview_with_layers(interaction, false)
    }
    pub fn measure_snap_preview_with_layers(
        &self,
        interaction: &TonalInteraction,
        layers: bool,
    ) -> Result<TonalInteraction, CommandError> {
        self.check_request(&interaction.request)?;
        if !self.tonal_context_matches(interaction)
            || self.document().and_then(Document::active_layer) != interaction.active
            || interaction.request.presentation_version != self.presentation.version
        {
            return Err(CommandError::Stale);
        }
        let mut measured = interaction.clone();
        if measured
            .request
            .snap_context
            .as_ref()
            .is_none_or(|c| c.includes_layers() != layers)
        {
            measured.request.snap_context = Some(
                super::guides::snap_context(
                    self.document().ok_or(CommandError::NoDocument)?,
                    interaction.layer,
                    layers,
                    &|| interaction.request.expired(),
                )?
                .ok_or_else(|| CommandError::Invalid("no finite content to snap".into()))?,
            );
        }
        Ok(measured)
    }
    pub fn snap_preview_measurement_ready(
        &self,
        interaction: &TonalInteraction,
        layers: bool,
    ) -> bool {
        self.filter_preview_context_matches(interaction)
            && interaction
                .request
                .snap_context
                .as_ref()
                .is_some_and(|c| c.includes_layers() == layers)
    }
    pub fn filter_preview_context_matches(&self, interaction: &TonalInteraction) -> bool {
        self.tonal_context_matches(interaction)
            && self.document().and_then(Document::active_layer) == interaction.active
            && (interaction.request.snap_context.is_none()
                || interaction.request.presentation_version == self.presentation.version)
    }
    pub fn begin_paint(
        &self,
        layer: LayerRef,
        command: &str,
    ) -> Result<PaintInteraction, CommandError> {
        if !matches!(command, "paint.fill" | "paint.bucket" | "paint.gradient") {
            return Err(CommandError::Invalid("unsupported paint gesture".into()));
        }
        self.authorize(&Source::User, command, CommandKind::Mutate)?;
        let doc = self.document().ok_or(CommandError::NoDocument)?;
        let layer = doc.resolve(&layer)?;
        if !doc
            .layers()
            .iter()
            .any(|l| l.id() == layer && matches!(l.info().kind, crate::LayerKind::Pixel))
            || doc.layers().iter().any(|l| {
                (l.id() == layer || doc.is_descendant(layer, l.id()))
                    && (l.info().locks.all || l.info().locks.pixels)
            })
        {
            return Err(CommandError::Invalid(
                "paint target requires an unlocked pixel layer and ancestors".into(),
            ));
        }
        Ok(PaintInteraction {
            request: self.request(Source::User),
            epoch: self.epoch,
            layer,
            command: command.into(),
        })
    }
    pub fn prepare_paint(
        &self,
        interaction: &PaintInteraction,
        mut params: Value,
    ) -> Result<PreparedCommand, CommandError> {
        if self.epoch != interaction.epoch {
            return Err(CommandError::Stale);
        }
        let fields = params
            .as_object_mut()
            .ok_or_else(|| CommandError::Invalid("paint params must be an object".into()))?;
        let layer = json!({"id":interaction.layer});
        if fields.get("layer").is_some_and(|value| *value != layer) {
            return Err(CommandError::Invalid(
                "paint gesture target cannot change after first input".into(),
            ));
        }
        fields.insert("layer".into(), layer);
        self.prepare(interaction.request.clone(), &interaction.command, params)
    }
    pub fn begin_brush(
        &self,
        layer: LayerRef,
        target: BrushTarget,
        erase: bool,
    ) -> Result<BrushInteraction, CommandError> {
        let command = if erase { "paint.erase" } else { "paint.stroke" };
        self.authorize(&Source::User, command, CommandKind::Mutate)?;
        let doc = self.document().ok_or(CommandError::NoDocument)?;
        let layer = doc.resolve(&layer)?;
        let info = doc
            .layers()
            .iter()
            .find(|l| l.id() == layer)
            .expect("resolved layer")
            .info();
        if matches!(target, BrushTarget::Pixels) && !matches!(info.kind, crate::LayerKind::Pixel)
            || matches!(target, BrushTarget::Mask) && info.mask.is_none()
        {
            return Err(CommandError::Invalid(
                "choose a pixel layer or an existing layer mask for the brush".into(),
            ));
        }
        if doc.layers().iter().any(|l| {
            (l.id() == layer || doc.is_descendant(layer, l.id()))
                && (l.info().locks.all || l.info().locks.pixels)
        }) || erase && matches!(target, BrushTarget::Pixels) && info.locks.transparency
        {
            return Err(CommandError::Invalid(
                "brush target or its parent is locked".into(),
            ));
        }
        Ok(BrushInteraction {
            request: self.request(Source::User),
            epoch: self.epoch,
            layer,
            target,
            command: command.into(),
        })
    }
    pub fn begin_clone(&self, layer: LayerRef) -> Result<BrushInteraction, CommandError> {
        self.authorize(&Source::User, "retouch.clone", CommandKind::Mutate)?;
        let mut interaction = self.begin_brush(layer, BrushTarget::Pixels, false)?;
        interaction.command = "retouch.clone".into();
        Ok(interaction)
    }
    pub fn begin_heal(&self, layer: LayerRef) -> Result<BrushInteraction, CommandError> {
        self.authorize(&Source::User, "retouch.heal", CommandKind::Mutate)?;
        let mut interaction = self.begin_brush(layer, BrushTarget::Pixels, false)?;
        interaction.command = "retouch.heal".into();
        Ok(interaction)
    }
    pub fn begin_spot_heal(&self, layer: LayerRef) -> Result<BrushInteraction, CommandError> {
        self.authorize(&Source::User, "retouch.spot_heal", CommandKind::Mutate)?;
        let mut interaction = self.begin_brush(layer, BrushTarget::Pixels, false)?;
        interaction.command = "retouch.spot_heal".into();
        Ok(interaction)
    }
    pub fn begin_tonal_brush(
        &self,
        layer: LayerRef,
        command: &str,
    ) -> Result<BrushInteraction, CommandError> {
        if !matches!(command, "retouch.dodge" | "retouch.burn" | "retouch.sponge") {
            return Err(CommandError::Invalid(
                "unsupported tonal brush command".into(),
            ));
        }
        self.authorize(&Source::User, command, CommandKind::Mutate)?;
        let mut interaction = self.begin_brush(layer, BrushTarget::Pixels, false)?;
        interaction.command = command.into();
        Ok(interaction)
    }
    pub fn prepare_brush(
        &self,
        interaction: &BrushInteraction,
        mut params: Value,
    ) -> Result<PreparedCommand, CommandError> {
        if self.epoch != interaction.epoch {
            return Err(CommandError::Stale);
        }
        let payload = if matches!(
            interaction.command.as_str(),
            "retouch.heal"
                | "retouch.spot_heal"
                | "retouch.dodge"
                | "retouch.burn"
                | "retouch.sponge"
        ) {
            params.get_mut("stroke").ok_or_else(|| {
                CommandError::Invalid("healing needs nested stroke parameters".into())
            })?
        } else {
            &mut params
        };
        let fields = payload
            .as_object_mut()
            .ok_or_else(|| CommandError::Invalid("brush parameters must be an object".into()))?;
        let layer = json!({"id":interaction.layer});
        let target = json!(interaction.target);
        if fields.get("layer").is_some_and(|value| *value != layer)
            || fields.get("target").is_some_and(|value| *value != target)
        {
            return Err(CommandError::Invalid(
                "brush gesture target is anchored at its first input".into(),
            ));
        }
        fields.insert("layer".into(), layer);
        fields.insert("target".into(), target);
        self.prepare(interaction.request.clone(), &interaction.command, params)
    }
    /// Context identity, independent of whether the request was explicitly
    /// cancelled. It is safe to restore a saved display only in this context.
    pub fn tonal_context_matches(&self, interaction: &TonalInteraction) -> bool {
        interaction.epoch == self.epoch
            && interaction.request.session == self.session
            && interaction.request.document == self.document().map(Document::id)
            && interaction.request.revision == self.revision
            && self.control == Control::User
    }
    pub fn preview_tonal(
        &self,
        interaction: &TonalInteraction,
        settings: crate::TonalAdjustment,
    ) -> Result<PreparedCommand, CommandError> {
        if !self.tonal_context_matches(interaction) {
            return Err(CommandError::Stale);
        }
        self.prepare(
            interaction.request.clone(),
            "adjust.tonal.set",
            json!({"layer":{"id":interaction.layer},"adjustment":settings}),
        )
    }
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            ruler_metrics: self
                .document
                .as_ref()
                .map(|doc| self.presentation.ruler_metrics(doc)),
            presentation: self.presentation.clone(),
            guides: self
                .document
                .as_ref()
                .map(|d| d.guides.clone())
                .unwrap_or_default(),
            paint_colors: self.paint_colors.clone(),
            selection_extent: self
                .document
                .as_ref()
                .and_then(|doc| doc.selection())
                .and_then(|mask| mask.content_bounds().ok().flatten()),
            selection_summary_available: self
                .document
                .as_ref()
                .and_then(|doc| doc.selection())
                .is_none_or(|mask| mask.content_bounds().is_ok()),
            selection_extent_conservative: self
                .document
                .as_ref()
                .and_then(|doc| doc.selection())
                .is_some_and(|mask| {
                    let layout = mask.layout();
                    (layout.default_coverage ^ if layout.inverted { 65535 } else { 0 }) != 0
                }),
            selection: self
                .document
                .as_ref()
                .and_then(|doc| doc.selection().map(strata_pixels::TiledMask::layout)),
            resolution_dpi: self.document.as_ref().map(Document::resolution_dpi),
            preset: self
                .document
                .as_ref()
                .and_then(|d| d.preset().map(str::to_owned)),
            session_id: self.session,
            document_id: self.document.as_ref().map(Document::id),
            revision: self.revision,
            dirty: self.dirty(),
            frame: self.document.as_ref().map(|d| d.frame().clone()),
            color_mode: self.document.as_ref().map(|_| ColorMode::Rgb),
            bit_depth: self.document.as_ref().map(Document::bit_depth),
            profile_name: self.document.as_ref().map(|d| d.profile_name().into()),
            profile_fallback: self
                .document
                .as_ref()
                .is_some_and(Document::profile_fallback),
            layers: self
                .document
                .as_ref()
                .map(|d| d.layers().iter().map(|l| l.info().clone()).collect())
                .unwrap_or_default(),
            active_layer: self.selected,
            history: self.history.iter().map(|h| h.info.clone()).collect(),
            history_cursor: self.cursor,
            executions: self.executions.clone(),
            control_events: self.control_events.clone(),
            control: self.control.clone(),
        }
    }
    pub fn begin_turn(&mut self) -> Result<Uuid, CommandError> {
        if self.control != Control::User {
            return Err(CommandError::ControlLocked);
        }
        let turn_id = Uuid::new_v4();
        self.cancellation.cancel();
        self.control = Control::Running { turn_id };
        self.epoch += 1;
        Ok(turn_id)
    }
    pub fn pause_turn(&mut self) -> Result<(), CommandError> {
        let Control::Running { turn_id } = self.control else {
            return Err(CommandError::ControlLocked);
        };
        self.control = Control::Paused { turn_id };
        self.epoch += 1;
        Ok(())
    }
    pub fn resume_turn(&mut self) -> Result<(), CommandError> {
        let Control::Paused { turn_id } = self.control else {
            return Err(CommandError::ControlLocked);
        };
        self.control = Control::Running { turn_id };
        self.epoch += 1;
        Ok(())
    }
    pub fn stop_turn(&mut self) {
        self.cancellation.cancel();
        self.control = Control::User;
        self.epoch += 1;
    }
    pub fn cancel_pending_work(&mut self) {
        self.cancellation.cancel();
        self.epoch += 1;
    }
    pub fn work_cancellation(&self) -> WorkCancellation {
        self.cancellation.clone()
    }
    fn authorize(&self, source: &Source, id: &str, kind: CommandKind) -> Result<(), CommandError> {
        if id.starts_with("paint.colors.")
            && !matches!(id, "paint.colors.get" | "paint.colors.describe")
            && matches!(source, Source::User | Source::Test)
            && self.control != Control::User
        {
            return Err(CommandError::ControlLocked);
        }
        let allowed = match (source, &self.control) {
            (Source::User | Source::Test, Control::User) => true,
            (Source::User | Source::Test, Control::Running { .. }) => {
                matches!(kind, CommandKind::Query | CommandKind::View)
            }
            (Source::User | Source::Test, Control::Paused { .. }) => {
                matches!(kind, CommandKind::Query | CommandKind::View)
                    || matches!(
                        id,
                        "layer.set_visibility"
                            | "edit.undo"
                            | "edit.redo"
                            | "edit.undo_turn"
                            | "select.marquee"
                            | "select.all"
                            | "select.none"
                            | "select.inverse"
                    )
                    || id.starts_with("select.") && id != "select.subject"
            }
            (Source::Ai { turn_id: a }, Control::Running { turn_id: b }) => a == b,
            (Source::Ai { turn_id: a }, Control::Paused { turn_id: b }) => {
                a == b && kind == CommandKind::Query
            }
            _ => false,
        };
        if allowed {
            Ok(())
        } else {
            Err(CommandError::ControlLocked)
        }
    }
    fn check_request(&self, r: &Request) -> Result<(), CommandError> {
        if r.expired() {
            return Err(CommandError::Expired);
        }
        if r.session != self.session
            || r.document != self.document.as_ref().map(Document::id)
            || r.revision != self.revision
        {
            return Err(CommandError::Stale);
        }
        Ok(())
    }
    pub fn execute(
        &mut self,
        request: Request,
        id: &str,
        params: Value,
        prepare_display: impl FnOnce(&PreparedCommand) -> Result<(), String>,
    ) -> Result<Value, CommandError> {
        let prepared = self.prepare(request, id, params)?;
        if !prepared.replay {
            prepare_display(&prepared).map_err(CommandError::Preparation)?;
        }
        self.commit(prepared)
    }
    pub fn prepare(
        &self,
        request: Request,
        id: &str,
        params: Value,
    ) -> Result<PreparedCommand, CommandError> {
        let kind = registry::kind(id)?;
        if kind == CommandKind::View && request.issued.elapsed() > Duration::from_secs(30) {
            return Err(CommandError::Expired);
        }
        if request.session != self.session || request.expired() {
            return Err(CommandError::Expired);
        }
        // Retries return the original result without reapplying an effect, even
        // when a subsequent call changed revision. Expired requests never replay.
        if let Some(old) = self.receipts.get(&request.call_id) {
            if old.command != id
                || old.params != params
                || old.source != request.source
                || old.document != request.document
            {
                return Err(CommandError::CallConflict);
            }
            return Ok(PreparedCommand {
                annotations_only: false,
                read_resource: None,
                request,
                command: id.into(),
                params,
                epoch: self.epoch,
                candidate: None,
                action: Action::Query,
                result: old.result.clone(),
                selected: self.selected,
                replay: true,
                required_active_layer: None,
                required_export_preset: None,
                required_revert_source: None,
            });
        }
        self.authorize(&request.source, id, kind)?;
        self.check_request(&request)?;
        if (id.starts_with("view.guides.")
            || id.starts_with("view.presentation.")
            || id == "layer.snap_position"
            || id == "view.rulers.convert")
            && request.presentation_version != self.presentation.version
        {
            return Err(CommandError::Stale);
        }
        if id.starts_with("paint.colors.")
            && request.paint_colors_version != self.paint_colors.version
        {
            return Err(CommandError::Stale);
        }
        // View/query calls run for every display-link frame. They never need a
        // copy of every layer's tile index; edits alone stage a COW document.
        let mut candidate = if kind == CommandKind::Mutate {
            self.document.clone()
        } else {
            None
        };
        let mut selected = self.selected;
        let mut read_resource = None;
        let mut annotations_only = false;
        let mut required_export_preset = None;
        let mut required_revert_source = None;
        let (action, result) = match kind {
            CommandKind::Query => {
                if id == "edit.clipboard.write_status" {
                    let p: ClipboardResource = parse(params.clone())?;
                    self.check_clipboard(&request, &p.resource)?;
                    if !self.clipboard_write_confirmed(&request, &p.resource)? {
                        return Err(CommandError::Permission);
                    }
                } else if id != "paint.sample_color"
                    && id != "layer.pixel_bounds"
                    && id != "type.inspect"
                    && id != "view.guides.list"
                    && id != "view.rulers.convert"
                {
                    let _: Empty = parse(params.clone())?;
                }
                (
                    Action::Query,
                    if id == "system.preferences.get" {
                        json!(self.preferences.get()?)
                    } else if id == "system.memory.get" {
                        json!(super::MemoryStatus::read(&self.pool))
                    } else if id == "file.revert_status" {
                        self.revert_status()
                    } else if id == "file.export_preset.get" {
                        json!(self.export_presets.get()?)
                    } else if id == "view.rulers.convert" {
                        super::guides::ruler_coordinates(
                            self.document().ok_or(CommandError::NoDocument)?,
                            &self.presentation,
                            params.clone(),
                        )?
                    } else if id == "view.presentation.get" {
                        json!(self.presentation)
                    } else if id == "view.guides.list" {
                        super::guides::list(
                            self.document().ok_or(CommandError::NoDocument)?,
                            params.clone(),
                        )?
                    } else if id == "type.fonts" {
                        super::typography::fonts()
                    } else if id == "type.inspect" {
                        let p: Target = parse(params.clone())?;
                        super::typography::inspect(
                            self.document().ok_or(CommandError::NoDocument)?,
                            &p.layer,
                        )?
                    } else if id == "paint.colors.get" {
                        json!(self.paint_colors)
                    } else if id == "paint.colors.describe" {
                        json!(super::color_components::describe(&self.paint_colors))
                    } else if id == "layer.pixel_bounds" {
                        let p: Target = parse(params.clone())?;
                        let doc = self.document().ok_or(CommandError::NoDocument)?;
                        let target = doc.resolve(&p.layer)?;
                        let layer = doc.layers().iter().find(|l| l.id() == target).unwrap();
                        if !doc.effective_visibility(target) {
                            return Err(CommandError::Invalid(
                                "bounds query requires a visible layer".into(),
                            ));
                        }
                        let bounds = super::transform::bounds(doc, layer, &|| request.expired())?;
                        json!({"layer_id":target,"bounds":bounds})
                    } else if id == "paint.sample_color" {
                        super::paint::sample(
                            self.document().ok_or(CommandError::NoDocument)?,
                            params.clone(),
                        )?
                    } else if id == "edit.clipboard.write_status" {
                        json!({"status":"completed","undoable":false})
                    } else if id == "file.recent.list" {
                        self.recent_result(&request)
                    } else if id == "document.histogram" {
                        json!(
                            self.document
                                .as_ref()
                                .ok_or(CommandError::NoDocument)?
                                .histogram()?
                        )
                    } else if id == "layer.list" {
                        json!({"layers":self.snapshot().layers})
                    } else {
                        json!(self.snapshot())
                    },
                )
            }
            CommandKind::Lifecycle
                if matches!(id, "file.open" | "file.recover" | "file.revert") =>
            {
                let p: OpenFile = if id == "file.revert" {
                    let r: RevertFile = parse(params.clone())?;
                    if r.expected_revision.is_some_and(|v| v != self.revision) {
                        return Err(CommandError::Stale);
                    }
                    OpenFile {
                        resource: r.resource,
                        discard_changes: r.discard_changes,
                    }
                } else {
                    parse(params.clone())?
                };
                let source = self.read_resource_path(&request, &p.resource)?;
                if id == "file.revert" {
                    self.validate_revert(&request, &p)?;
                    required_revert_source = Some(source.clone());
                }
                let (doc, path) = self.prepare_open(
                    &request,
                    serde_json::to_value(&p).unwrap(),
                    id == "file.recover",
                )?;
                read_resource = Some(p.resource);
                selected = doc.active_layer();
                let result = json!({"document_id":doc.id(),"layer_id":selected});
                candidate = Some(doc);
                (
                    if id == "file.recover" {
                        Action::Recover
                    } else {
                        Action::Open(path, source)
                    },
                    result,
                )
            }
            CommandKind::Lifecycle => {
                let p: NewDocument = parse(params.clone())?;
                if self.dirty() && !p.discard_changes {
                    return Err(CommandError::UnsavedChanges);
                }
                let doc = super::new::document(p, self.pool.clone())?;
                selected = doc.active_layer();
                let result = json!({"document_id":doc.id(),"layer_id":selected});
                candidate = Some(doc);
                (Action::New, result)
            }
            CommandKind::Mutate => {
                let doc = candidate.as_mut().ok_or(CommandError::NoDocument)?;
                let (action, result) = if matches!(
                    id,
                    "edit.cut" | "edit.paste" | "edit.paste_in_place"
                ) {
                    let (token, result) = self.clipboard_edit(&request, doc, id, params.clone())?;
                    (Action::ClipboardEdit(token), result)
                } else if id == "select.subject" {
                    let p: crate::SubjectParams = parse(params.clone())?;
                    let service=self.selector.as_ref().ok_or_else(||CommandError::Invalid("Subject model is not configured. Open the subject model panel and approve the verified model download.".into()))?;
                    let result = service
                        .select(doc, &p, &|| request.expired())
                        .map_err(CommandError::Preparation)?;
                    if request.expired() {
                        return Err(CommandError::Expired);
                    }
                    if result.needs_confirmation && !p.accept_ambiguous {
                        return Err(CommandError::Invalid(format!(
                            "Subject selection is ambiguous; no selection committed. Ask the user to identify a rectangle or explicitly confirm all detected subjects before retrying with accept_ambiguous=true. Diagnostics: {}",
                            result.diagnostics
                        )));
                    }
                    if let Some(current) = &doc.selection {
                        doc.previous_selection = Some(current.clone());
                    }
                    doc.selection = Some(super::local::combine(
                        doc,
                        doc.selection.as_ref(),
                        result.mask,
                        p.mode,
                    )?);
                    (
                        Action::Edit,
                        json!({"selection":doc.selection().map(strata_pixels::TiledMask::layout),"diagnostics":result.diagnostics}),
                    )
                } else if id == "layer.snap_position" {
                    let result = super::guides::snap(
                        doc,
                        params.clone(),
                        request.snap_context.as_ref(),
                        &|| request.expired(),
                    )?;
                    (
                        if result["changed"] == false {
                            Action::Query
                        } else {
                            Action::Edit
                        },
                        result,
                    )
                } else if let Some(result) =
                    super::guides::mutate(doc, id, params.clone(), self.presentation.guides_locked)?
                {
                    annotations_only = true;
                    (
                        if result["changed"] == false {
                            Action::Query
                        } else {
                            Action::Edit
                        },
                        result,
                    )
                } else if let Some(result) =
                    super::typography::mutate(doc, id, params.clone(), &|| request.expired())?
                {
                    (
                        if result["changed"] == false {
                            Action::Query
                        } else {
                            Action::Edit
                        },
                        result,
                    )
                } else if let Some(result) =
                    super::selection::mutate_cancellable(doc, id, params.clone(), &|| {
                        request.expired()
                    })?
                {
                    (Action::Edit, result)
                } else if let Some(result) =
                    super::transform::mutate(doc, id, params.clone(), &|| request.expired())?
                {
                    (
                        if result["changed"] == false {
                            Action::Query
                        } else {
                            Action::Edit
                        },
                        result,
                    )
                } else if let Some(result) =
                    super::depth::mutate(doc, id, params.clone(), &|| request.expired())?
                {
                    (
                        if result["changed"] == false {
                            Action::Query
                        } else {
                            Action::Edit
                        },
                        result,
                    )
                } else if let Some(result) =
                    super::straighten::mutate(doc, id, params.clone(), &|| request.expired())?
                {
                    (
                        if result["changed"] == false {
                            Action::Query
                        } else {
                            Action::Edit
                        },
                        result,
                    )
                } else if let Some(result) =
                    super::resize_image::mutate(doc, id, params.clone(), &|| request.expired())?
                {
                    (
                        if result["changed"] == false {
                            Action::Query
                        } else {
                            Action::Edit
                        },
                        result,
                    )
                } else if let Some(result) =
                    super::rotate_image::mutate(doc, id, params.clone(), &|| request.expired())?
                {
                    (
                        if result["changed"] == false {
                            Action::Query
                        } else {
                            Action::Edit
                        },
                        result,
                    )
                } else if let Some(result) =
                    super::orthogonal::mutate(doc, id, params.clone(), &|| request.expired())?
                {
                    (Action::Edit, result)
                } else if let Some(result) =
                    super::trim::mutate(doc, id, params.clone(), &|| request.expired())?
                {
                    (
                        if result["changed"] == false {
                            Action::Query
                        } else {
                            Action::Edit
                        },
                        result,
                    )
                } else if let Some(result) =
                    super::canvas::mutate(doc, id, params.clone(), &|| request.expired())?
                {
                    (Action::Edit, result)
                } else if let Some(result) =
                    super::profiles::mutate(doc, id, params.clone(), &|| request.expired())?
                {
                    (Action::Edit, result)
                } else if let Some(result) =
                    super::auto_correct::mutate(doc, id, params.clone(), &|| request.expired())?
                {
                    (
                        if result["changed"] == false {
                            Action::Query
                        } else {
                            Action::Edit
                        },
                        result,
                    )
                } else if let Some(result) =
                    super::filters::mutate(doc, id, params.clone(), &|| request.expired())?
                {
                    (
                        if id == "filter.high_pass" && result["touched_tiles"] == 0 {
                            Action::Query
                        } else {
                            Action::Edit
                        },
                        result,
                    )
                } else if let Some(result) =
                    super::tonal_brush::mutate(doc, id, params.clone(), &|| request.expired())?
                {
                    (Action::Edit, result)
                } else if let Some(result) =
                    super::spot_heal::mutate(doc, id, params.clone(), &|| request.expired())?
                {
                    (Action::Edit, result)
                } else if let Some(result) =
                    super::heal::mutate(doc, id, params.clone(), &|| request.expired())?
                {
                    (Action::Edit, result)
                } else if let Some(result) =
                    super::brush::mutate_cancellable(doc, id, params.clone(), &|| {
                        request.expired()
                    })?
                {
                    (Action::Edit, result)
                } else if let Some(result) =
                    super::paint::mutate(doc, id, params.clone(), &|| request.expired())?
                {
                    (Action::Edit, result)
                } else if id == "file.place" {
                    let (resource, result) = self.prepare_place(&request, doc, params.clone())?;
                    (Action::Place(resource), result)
                } else if id == "document.batch" {
                    (Action::Edit, batch(doc, params.clone(), self.snapshot())?)
                } else {
                    (Action::Edit, mutate(doc, id, params.clone())?)
                };
                doc.validate()?;
                doc.state = StateId::fresh();
                selected = doc.active;
                (action, result)
            }
            CommandKind::History => {
                let next = match id {
                    "edit.undo" => {
                        let _: Empty = parse(params.clone())?;
                        self.cursor
                            .checked_sub(1)
                            .ok_or_else(|| CommandError::Invalid("nothing to undo".into()))?
                    }
                    "edit.redo" => {
                        let _: Empty = parse(params.clone())?;
                        if self.cursor >= self.history.len() {
                            return Err(CommandError::Invalid("nothing to redo".into()));
                        }
                        self.cursor + 1
                    }
                    "edit.undo_turn" => {
                        let p: UndoTurn = parse(params.clone())?;
                        if self.cursor == 0
                            || self.history[self.cursor - 1].info.turn_id != Some(p.turn_id)
                        {
                            return Err(CommandError::Invalid(
                                "turn is not at history tail".into(),
                            ));
                        }
                        let mut n = self.cursor;
                        while n > 0 && self.history[n - 1].info.turn_id == Some(p.turn_id) {
                            n -= 1;
                        }
                        n
                    }
                    _ => unreachable!(),
                };
                candidate = Some(if next < self.cursor {
                    self.history[next].before.clone()
                } else {
                    self.history[next - 1].after.clone()
                });
                annotations_only = self.history[next.min(self.cursor)..next.max(self.cursor)]
                    .iter()
                    .all(|e| e.info.command.starts_with("view.guides."));
                selected = candidate.as_ref().and_then(Document::active_layer);
                (Action::History(next), json!({"history_cursor":next}))
            }
            CommandKind::View => {
                if id == "view.presentation.set" {
                    let value = super::guides::presentation(&self.presentation, params.clone())?;
                    (Action::Presentation(value.clone()), json!(value))
                } else if id.starts_with("paint.colors.") {
                    let mut colors = self.paint_colors.clone();
                    colors.version = colors.version.checked_add(1).ok_or_else(|| {
                        CommandError::Invalid("paint color version exhausted".into())
                    })?;
                    match id {
                        "paint.colors.set_components" => {
                            let p: super::SetColorComponents = parse(params.clone())?;
                            if p.expected_version
                                .is_some_and(|v| v != self.paint_colors.version)
                            {
                                return Err(CommandError::Stale);
                            }
                            let color = super::color_components::parse_color(p.color)?;
                            match p.target {
                                super::PaintColorTarget::Foreground => colors.foreground = color,
                                super::PaintColorTarget::Background => colors.background = color,
                            }
                        }
                        "paint.colors.set" => {
                            let p: super::SetPaintColors = parse(params.clone())?;
                            if p.expected_version
                                .is_some_and(|v| v != self.paint_colors.version)
                            {
                                return Err(CommandError::Stale);
                            }
                            if p.foreground.is_none() && p.background.is_none() {
                                return Err(CommandError::Invalid(
                                    "provide foreground and/or background working RGB color".into(),
                                ));
                            }
                            for value in [p.foreground, p.background].into_iter().flatten() {
                                if value
                                    .iter()
                                    .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
                                {
                                    return Err(CommandError::Invalid(
                                        "paint colors require finite normalized RGBA 0..1".into(),
                                    ));
                                }
                            }
                            if let Some(c) = p.foreground {
                                colors.foreground = c;
                            }
                            if let Some(c) = p.background {
                                colors.background = c;
                            }
                        }
                        "paint.colors.swap" => {
                            let _: Empty = parse(params.clone())?;
                            std::mem::swap(&mut colors.foreground, &mut colors.background);
                        }
                        "paint.colors.reset" => {
                            let _: Empty = parse(params.clone())?;
                            colors.foreground = [0., 0., 0., 1.];
                            colors.background = [1., 1., 1., 1.];
                        }
                        _ => {
                            return Err(CommandError::Invalid(
                                "unknown paint color state operation".into(),
                            ));
                        }
                    }
                    (Action::PaintColors(colors.clone()), json!(colors))
                } else {
                    match id {
                        "layer.select" => {
                            let p: Target = parse(params.clone())?;
                            selected = Some(
                                self.document
                                    .as_ref()
                                    .ok_or(CommandError::NoDocument)?
                                    .resolve(&p.layer)?,
                            );
                        }
                        "view.pan" => {
                            let p: Pan = parse(params.clone())?;
                            if !p.dx.is_finite() || !p.dy.is_finite() {
                                return Err(CommandError::Invalid("nonfinite pan".into()));
                            }
                        }
                        "view.zoom" => {
                            let p: Zoom = parse(params.clone())?;
                            if !p.x.is_finite()
                                || !p.y.is_finite()
                                || !p.factor.is_finite()
                                || p.factor <= 0.0
                            {
                                return Err(CommandError::Invalid("invalid zoom".into()));
                            }
                        }
                        _ => {
                            let _: Empty = parse(params.clone())?;
                        }
                    }
                    (Action::View, json!({"view":id,"params":params}))
                }
            }
            CommandKind::Workspace => {
                let duplicate: super::DuplicateDocument = parse(params.clone())?;
                if duplicate
                    .expected_revision
                    .is_some_and(|revision| revision != self.revision)
                {
                    return Err(CommandError::Stale);
                }
                let original = self.document().ok_or(CommandError::NoDocument)?;
                let (copy, index_budget) =
                    super::duplicate_document::prepare(original, &duplicate, &|| {
                        request.expired()
                    })?;
                let result = json!(super::DuplicateResult {
                    copy_document_id: copy.id(),
                    name: duplicate.name.clone(),
                    mode: duplicate.mode,
                });
                candidate = Some(copy);
                (
                    Action::Duplicate {
                        name: duplicate.name,
                        index_budget,
                    },
                    result,
                )
            }
            CommandKind::Settings => match id {
                "system.preferences.set" => {
                    let params: super::SetPreferences = parse(params.clone())?;
                    let (expected, value) = self.preferences.prepare(params)?;
                    (Action::Preferences(expected, value.clone()), json!(value))
                }
                "system.memory.trim" => {
                    let _: Empty = parse(params.clone())?;
                    (
                        Action::TrimMemory,
                        json!(super::MemoryStatus::read(&self.pool)),
                    )
                }
                _ => {
                    let p: super::SetExportPreset = parse(params.clone())?;
                    let (expected, value) = self.export_presets.prepare(p)?;
                    (Action::ExportPreset(expected, value.clone()), json!(value))
                }
            },
            CommandKind::External if id == "file.quick_export" => {
                let p: super::QuickExport = parse(params.clone())?;
                let preset = self.export_presets.get()?;
                if p.preset_version != preset.version {
                    return Err(CommandError::Stale);
                }
                let options = preset.options.export(p.resource);
                let write = self.prepare_write(
                    &request,
                    "file.export",
                    serde_json::to_value(&options).unwrap(),
                )?;
                let doc = self.document().ok_or(CommandError::NoDocument)?;
                required_export_preset = Some(preset.version);
                (
                    Action::External(Box::new(write)),
                    json!(super::QuickExportResult {
                        status: "completed".into(),
                        command: id.into(),
                        preset_version: preset.version,
                        format: preset.options.format,
                        width: options.width.unwrap_or(doc.frame().width),
                        height: options.height.unwrap_or(doc.frame().height)
                    }),
                )
            }
            CommandKind::External if matches!(id, "edit.copy" | "edit.copy_merged") => {
                let copy = self.prepare_copy(&request, id, params.clone())?;
                let result =
                    json!({"clipboard_resource":copy.token,"status":"prepared","undoable":false});
                (Action::ClipboardCopy(Box::new(copy)), result)
            }
            CommandKind::External => (
                Action::External(Box::new(self.prepare_write(
                    &request,
                    id,
                    params.clone(),
                )?)),
                json!({"status":"completed","command":id}),
            ),
        };
        Ok(PreparedCommand {
            annotations_only,
            read_resource,
            request,
            command: id.into(),
            params,
            epoch: self.epoch,
            candidate,
            action,
            result,
            selected,
            replay: false,
            required_active_layer: None,
            required_export_preset,
            required_revert_source,
        })
    }
    pub fn commit(&mut self, prepared: PreparedCommand) -> Result<Value, CommandError> {
        self.commit_inner(prepared, false).map(|(result, _)| result)
    }
    /// The workspace consumes the one detached editor only when its command
    /// commits. A receipt retry returns the original result without a new tab.
    pub fn commit_with_document_copy(
        &mut self,
        prepared: PreparedCommand,
    ) -> Result<(Value, Option<super::DocumentCopy>), CommandError> {
        self.commit_inner(prepared, true)
    }
    fn commit_inner(
        &mut self,
        mut prepared: PreparedCommand,
        accept_copy: bool,
    ) -> Result<(Value, Option<super::DocumentCopy>), CommandError> {
        // Two workers may prepare the same call before either commits. Recheck
        // its receipt under the host's commit lock, before checking revision.
        if prepared.request.session != self.session
            || prepared.request.expired()
            || (registry::kind(&prepared.command)? == CommandKind::View
                && prepared.request.issued.elapsed() > Duration::from_secs(30))
        {
            return Err(CommandError::Expired);
        }
        if let Some(old) = self.receipts.get(&prepared.request.call_id) {
            if old.command != prepared.command
                || old.params != prepared.params
                || old.source != prepared.request.source
                || old.document != prepared.request.document
            {
                return Err(CommandError::CallConflict);
            }
            return Ok((old.result.clone(), None));
        }
        if prepared.replay {
            return Err(CommandError::Expired);
        }
        if matches!(prepared.action, Action::Duplicate { .. }) && !accept_copy {
            return Err(CommandError::Invalid(
                "the workspace host must accept the new document".into(),
            ));
        }
        if matches!(prepared.action, Action::External(_)) {
            if prepared.request.session != self.session
                || prepared.request.document != self.document.as_ref().map(Document::id)
                || prepared.request.expired()
            {
                return Err(CommandError::Stale);
            }
        } else {
            self.check_request(&prepared.request)?;
        }
        if prepared.epoch != self.epoch {
            return Err(CommandError::Stale);
        }
        if prepared
            .required_revert_source
            .as_deref()
            .is_some_and(|p| Some(p) != self.revert_source())
        {
            return Err(CommandError::Stale);
        }
        if prepared
            .required_active_layer
            .is_some_and(|layer| self.document().and_then(Document::active_layer) != Some(layer))
        {
            return Err(CommandError::Stale);
        }
        self.authorize(
            &prepared.request.source,
            &prepared.command,
            registry::kind(&prepared.command)?,
        )?;
        if prepared.required_export_preset.is_some() && prepared.request.revision != self.revision {
            return Err(CommandError::Stale);
        }
        let export_presets = self.export_presets.clone();
        let _preset_guard = prepared
            .required_export_preset
            .map(|v| export_presets.guard(v))
            .transpose()?;
        if prepared.command.starts_with("paint.colors.")
            && prepared.request.paint_colors_version != self.paint_colors.version
        {
            return Err(CommandError::Stale);
        }
        if (prepared.command.starts_with("view.guides.")
            || prepared.command.starts_with("view.presentation.")
            || prepared.command == "layer.snap_position"
            || prepared.command == "view.rulers.convert")
            && prepared.request.presentation_version != self.presentation.version
        {
            return Err(CommandError::Stale);
        }
        self.receipts.retain(|_, r| {
            r.issued.elapsed() <= Duration::from_secs(if r.view { 30 } else { 3600 })
        });
        if self.receipts.len() >= 4096 {
            return Err(CommandError::Invalid(
                "call receipt capacity reached; retry after expiry".into(),
            ));
        }
        if matches!(
            prepared.action,
            Action::New
                | Action::Open(..)
                | Action::Recover
                | Action::Edit
                | Action::Place(_)
                | Action::History(_)
                | Action::ClipboardEdit(_)
        ) && self.revision == u64::MAX
        {
            return Err(CommandError::Invalid("revision exhausted".into()));
        }
        if let Action::Place(resource) = &prepared.action {
            self.resource(resource, &prepared.request, false)?;
        }
        if let Action::ClipboardEdit(resource) = &prepared.action {
            self.check_clipboard(&prepared.request, resource)?;
        }
        if let Some(resource) = &prepared.read_resource {
            self.resource(resource, &prepared.request, false)?;
        }
        let keeps_layer_selection = prepared.annotations_only
            || matches!(prepared.action, Action::Query)
            || matches!(
                prepared.action,
                Action::PaintColors(_)
                    | Action::Presentation(_)
                    | Action::ExportPreset(_, _)
                    | Action::Preferences(_, _)
                    | Action::TrimMemory
                    | Action::Duplicate { .. }
            )
            || prepared.command.starts_with("view.guides.")
            || prepared.command == "view.presentation.get"
            || prepared.command == "layer.snap_position"
            || (prepared.command == "filter.high_pass" && prepared.result["touched_tiles"] == 0)
            || matches!(
                prepared.command.as_str(),
                "paint.colors.get" | "paint.colors.describe" | "type.fonts" | "type.inspect"
            )
            || (matches!(
                prepared.command.as_str(),
                "image.trim"
                    | "image.rotate"
                    | "image.resize"
                    | "image.straighten"
                    | "image.convert_bit_depth"
                    | "image.auto"
                    | "transform.apply"
                    | "transform.apply_layers"
                    | "type.update"
                    | "layer.snap_position"
            ) && prepared.result["changed"] == false);
        let mut copy = None;
        match prepared.action {
            Action::Preferences(expected, value) => self.preferences.commit(expected, value)?,
            Action::TrimMemory => {
                self.pool.trim().map_err(DocumentError::from)?;
                prepared.result = json!(super::MemoryStatus::read(&self.pool));
            }
            Action::Duplicate { name, index_budget } => {
                let document = prepared.candidate.ok_or(CommandError::NoDocument)?;
                let mut editor = Self::with_memory_pool(self.pool.clone())
                    .with_app_preferences(self.preferences.clone())
                    .with_export_presets(self.export_presets.clone());
                editor.selected = document.active_layer();
                editor.document = Some(document);
                editor.copy_index_budget = Some(index_budget);
                editor.decoder = self.decoder.clone();
                editor.selector = self.selector.clone();
                copy = Some(super::DocumentCopy {
                    name,
                    editor,
                    source: prepared.request.source.clone(),
                });
            }
            Action::ExportPreset(expected, value) => self.export_presets.commit(expected, value)?,
            Action::Presentation(value) => {
                self.presentation = value;
            }
            Action::PaintColors(colors) => {
                self.paint_colors = colors;
            }
            Action::External(write) => {
                self.commit_write(&prepared.request, &prepared.command, *write)?;
            }
            Action::ClipboardCopy(copy) => {
                self.commit_copy(*copy, &prepared.command)?;
            }
            Action::Open(path, source) => {
                self.copy_index_budget = None;
                self.saved = prepared.candidate.as_ref().map(Document::state_id);
                self.document = prepared.candidate;
                self.history.clear();
                self.cursor = 0;
                self.project_path = path;
                self.opened_path = Some(source);
                self.revision += 1;
            }
            Action::Recover => {
                self.copy_index_budget = None;
                self.document = prepared.candidate;
                self.saved = None;
                self.history.clear();
                self.cursor = 0;
                self.project_path = None;
                self.opened_path = None;
                self.revision += 1;
            }
            Action::Edit | Action::Place(_) | Action::ClipboardEdit(_) => {
                let mut after = prepared.candidate.clone().ok_or(CommandError::NoDocument)?;
                let before = self.document.clone().ok_or(CommandError::NoDocument)?;
                if prepared.annotations_only || prepared.command == "layer.snap_position" {
                    after.active = before.active;
                }
                let mut unique = HashSet::new();
                let mut bytes = 0u64;
                for d in self.history[..self.cursor]
                    .iter()
                    .flat_map(|h| [&h.before, &h.after])
                    .chain([&before, &after])
                {
                    for (version, block_bytes) in d.source_blocks() {
                        if unique.insert(version) {
                            bytes += block_bytes;
                        }
                    }
                }
                if bytes > 24 * 1024 * 1024 * 1024 {
                    return Err(DocumentError::ResourceLimit.into());
                }
                self.history.truncate(self.cursor);
                let turn_id = match (&prepared.request.source, &self.control) {
                    (Source::Ai { turn_id }, _) | (_, Control::Paused { turn_id }) => {
                        Some(*turn_id)
                    }
                    _ => None,
                };
                let source = match prepared.request.source {
                    Source::User => SourceInfo::User,
                    Source::Ai { .. } => SourceInfo::Ai,
                    Source::Test => SourceInfo::Test,
                };
                self.history.push(HistoryEntry {
                    info: HistoryInfo {
                        command: prepared.command.clone(),
                        source,
                        turn_id,
                    },
                    before,
                    after: after.clone(),
                });
                if self.history.len() > 64 {
                    self.history.remove(0);
                }
                self.cursor = self.history.len();
                self.document = Some(after);
                self.revision += 1;
                if let Control::Paused { turn_id } = self.control {
                    let state = self.snapshot();
                    self.control_events.push(json!({"turn_id":turn_id,"command":prepared.command,"revision":self.revision,
                        "selection":state.selection,"selection_extent":state.selection_extent,"selection_extent_conservative":state.selection_extent_conservative}));
                    if self.control_events.len() > 256 {
                        self.control_events.remove(0);
                    }
                }
            }
            Action::New => {
                self.copy_index_budget = None;
                self.document = prepared.candidate;
                self.saved = None;
                self.history.clear();
                self.cursor = 0;
                self.project_path = None;
                self.opened_path = None;
                self.revision += 1;
            }
            Action::History(cursor) => {
                if let Control::Paused { turn_id } = self.control {
                    self.control_events.push(json!({"turn_id":turn_id,"command":prepared.command,"from_cursor":self.cursor,"to_cursor":cursor,"revision":self.revision+1}));
                    if self.control_events.len() > 256 {
                        self.control_events.remove(0);
                    }
                }
                let mut candidate = prepared.candidate;
                if prepared.annotations_only
                    && let Some(doc) = candidate.as_mut()
                {
                    doc.active = self.document.as_ref().and_then(Document::active_layer)
                }
                self.document = candidate;
                self.cursor = cursor;
                self.revision += 1;
            }
            Action::View => {
                if prepared.command == "layer.select"
                    && let Some(doc) = self.document.as_mut()
                {
                    // Selection is view state, but its current value is included
                    // when the user next saves the project.
                    doc.active = prepared.selected;
                }
            }
            Action::Query => {}
        }
        if !keeps_layer_selection {
            self.selected = prepared.selected;
        }
        self.receipts.insert(
            prepared.request.call_id,
            Receipt {
                view: registry::kind(&prepared.command)? == CommandKind::View,
                source: prepared.request.source,
                document: prepared.request.document,
                command: prepared.command,
                params: prepared.params,
                result: prepared.result.clone(),
                issued: prepared.request.issued,
            },
        );
        Ok((prepared.result, copy))
    }
}

pub(super) fn mutate(doc: &mut Document, id: &str, params: Value) -> Result<Value, CommandError> {
    if let Some(result) = clipboard::local_edit(doc, id, params.clone())? {
        return Ok(result);
    }
    if matches!(
        id,
        "layer.create" | "layer.create_group" | "layer.add_adjustment"
    ) && doc.insertion_parent().is_some_and(|parent| {
        doc.layers
            .iter()
            .any(|l| l.info.locks.all && (l.id() == parent || doc.is_descendant(parent, l.id())))
    }) {
        return Err(CommandError::Invalid(
            "cannot insert into a locked parent group".into(),
        ));
    }
    if id == "color.convert"
        && doc
            .layers()
            .iter()
            .any(|layer| layer.info().locks.all || layer.info().locks.pixels)
    {
        return Err(CommandError::Invalid(
            "working-space conversion would change locked pixels; unlock affected layers first"
                .into(),
        ));
    }
    if let Some(target) = params.get("layer") {
        let reference: LayerRef = parse(target.clone())?;
        let target = doc.resolve(&reference)?;
        let layer = doc
            .layers()
            .iter()
            .find(|layer| layer.id() == target)
            .expect("resolved layer");
        if doc
            .layers()
            .iter()
            .any(|ancestor| ancestor.info().locks.all && doc.is_descendant(target, ancestor.id()))
        {
            return Err(CommandError::Invalid(
                "a parent group is fully locked".into(),
            ));
        }
        if layer.info().locks.all && !matches!(id, "layer.set_locks" | "layer.set_visibility") {
            return Err(CommandError::Invalid(
                "layer is fully locked; unlock it before editing".into(),
            ));
        }
        if layer.info().locks.position && id == "layer.set_position" {
            return Err(CommandError::Invalid("layer position is locked".into()));
        }
        if id == "layer.set_position"
            && doc.layers.iter().any(|ancestor| {
                ancestor.info.locks.position && doc.is_descendant(target, ancestor.id())
            })
        {
            return Err(CommandError::Invalid(
                "a parent group locks positions".into(),
            ));
        }
        match id {
            "layer.set_pass_through" => {
                let p: PassThrough = parse(params)?;
                let layer = doc.layers.iter_mut().find(|l| l.id() == target).unwrap();
                if !matches!(layer.info.kind, crate::LayerKind::Group) {
                    return Err(CommandError::Invalid(
                        "pass-through applies only to layer groups".into(),
                    ));
                }
                layer.info.pass_through = p.pass_through;
                return Ok(json!({"layer_id":target}));
            }
            "layer.set_styles" => {
                let p: SetStyles = parse(params)?;
                doc.layers
                    .iter_mut()
                    .find(|l| l.id() == target)
                    .unwrap()
                    .info
                    .styles = p.styles;
                return Ok(json!({"layer_id":target}));
            }
            "layer.set_clipping" => {
                let p: Clipping = parse(params)?;
                doc.layers
                    .iter_mut()
                    .find(|l| l.id() == target)
                    .unwrap()
                    .info
                    .clipped = p.clipped;
                return Ok(json!({"layer_id":target}));
            }
            "layer.set_fill_opacity" => {
                let p: FillOpacity = parse(params)?;
                doc.layers
                    .iter_mut()
                    .find(|l| l.id() == target)
                    .expect("resolved layer")
                    .info
                    .fill_opacity = p.fill_opacity;
                return Ok(json!({"layer_id":target}));
            }
            "layer.set_locks" => {
                let p: SetLocks = parse(params)?;
                doc.layers
                    .iter_mut()
                    .find(|l| l.id() == target)
                    .expect("resolved layer")
                    .info
                    .locks = p.locks;
                return Ok(json!({"layer_id":target}));
            }
            "layer.set_color_label" => {
                let p: SetColorLabel = parse(params)?;
                doc.layers
                    .iter_mut()
                    .find(|l| l.id() == target)
                    .expect("resolved layer")
                    .info
                    .color_label = p.color_label;
                return Ok(json!({"layer_id":target}));
            }
            _ => {}
        }
    }
    if let Some(result) = super::local::mutate(doc, id, params.clone())? {
        return Ok(result);
    }
    if let Some(result) = super::selection::mutate(doc, id, params.clone())? {
        return Ok(result);
    }
    if let Some(result) = super::tree::mutate(doc, id, params.clone())? {
        return Ok(result);
    }
    if let Some(result) = super::generated::mutate(doc, id, params.clone())? {
        return Ok(result);
    }
    if let Some(result) = super::tonal::mutate(doc, id, params.clone())? {
        return Ok(result);
    }
    if let Some(result) = super::raster::mutate(doc, id, params.clone())? {
        return Ok(result);
    }
    if let Some(result) = super::arrange::mutate(doc, id, params.clone())? {
        return Ok(result);
    }
    if matches!(id, "layer.create")
        && doc.insertion_parent().is_some_and(|parent| {
            doc.layers()
                .iter()
                .any(|l| l.id() == parent && l.info().locks.all)
        })
    {
        return Err(CommandError::Invalid(
            "cannot insert into a locked group".into(),
        ));
    }
    if id == "layer.create" {
        let p: CreateLayer = parse(params)?;
        if p.name.len() > 1024 || doc.layers.len() >= crate::MAX_LAYERS {
            return Err(DocumentError::ResourceLimit.into());
        }
        let mut pixels =
            TiledPixels::with_pool(doc.format, tile_limit(doc.format), doc.memory_pool());
        if let Some(fill) = p.fill.filter(|fill| *fill != [0; 4]) {
            if doc.format == PixelFormat::Rgba8 && fill.iter().any(|&v| v > 255) {
                return Err(CommandError::Invalid("8-bit fill exceeds 255".into()));
            }
            let tile_bytes = doc
                .format
                .byte_count(256, 256)
                .map_err(DocumentError::from)? as u64;
            let added = u64::from(doc.frame.width.div_ceil(256))
                * u64::from(doc.frame.height.div_ceil(256))
                * tile_bytes;
            let existing: u64 = doc
                .layers
                .iter()
                .map(|layer| layer.pixels.referenced_bytes())
                .sum();
            if fill != [0; 4] && existing + added > crate::document::MAX_DOCUMENT_BYTES {
                return Err(DocumentError::ResourceLimit.into());
            }
            for ty in 0..doc.frame.height.div_ceil(256) {
                for tx in 0..doc.frame.width.div_ceil(256) {
                    let _scratch = doc
                        .memory_pool()
                        .reserve(strata_pixels::memory::MemoryKind::Transient, tile_bytes)
                        .map_err(DocumentError::from)?;
                    let mut tile = match doc.format {
                        PixelFormat::Rgba8 => PixelBuffer::Rgba8(vec![0; 256 * 256 * 4]),
                        PixelFormat::Rgba16 => PixelBuffer::Rgba16(vec![0; 256 * 256 * 4]),
                    };
                    for y in 0..256.min(doc.frame.height - ty * 256) as usize {
                        for x in 0..256.min(doc.frame.width - tx * 256) as usize {
                            let i = (y * 256 + x) * 4;
                            match &mut tile {
                                PixelBuffer::Rgba8(v) => {
                                    v[i..i + 4].copy_from_slice(&fill.map(|v| v as u8))
                                }
                                PixelBuffer::Rgba16(v) => v[i..i + 4].copy_from_slice(&fill),
                            }
                        }
                    }
                    pixels
                        .insert_tile(
                            strata_pixels::TileCoord::new(i64::from(tx), i64::from(ty))
                                .map_err(DocumentError::from)?,
                            tile,
                        )
                        .map_err(DocumentError::from)?;
                }
            }
        }
        let mut layer = Layer::new(p.name, pixels);
        layer.info.parent = doc.insertion_parent();
        layer.info.x = doc.frame.x;
        layer.info.y = doc.frame.y;
        let id = layer.id();
        doc.layers.push(layer);
        doc.active = Some(id);
        return Ok(json!({"layer_id":id}));
    }
    // Parse typed arguments first, then resolve a stable ID on this candidate.
    let target: LayerRef = serde_json::from_value(
        params
            .get("layer")
            .cloned()
            .ok_or_else(|| CommandError::Invalid("missing layer".into()))?,
    )
    .map_err(|e| CommandError::Invalid(e.to_string()))?;
    let layer_id = doc.resolve(&target)?;
    let index = doc
        .layers
        .iter()
        .position(|l| l.id() == layer_id)
        .ok_or(DocumentError::LayerNotFound)?;
    match id {
        "layer.delete" => {
            let _: Target = parse(params)?;
            return super::tree::delete(doc, layer_id);
        }
        "layer.duplicate" => {
            let _: Target = parse(params)?;
            return super::tree::duplicate(doc, layer_id);
        }
        "layer.rename" => {
            let p: Rename = parse(params)?;
            doc.layers[index].info.name = p.name;
        }
        "layer.set_visibility" => {
            let p: Visibility = parse(params)?;
            doc.layers[index].info.visible = p.visible;
        }
        "layer.set_opacity" => {
            let p: Opacity = parse(params)?;
            doc.layers[index].info.opacity = p.opacity;
        }
        "layer.set_blend_mode" => {
            let p: Blend = parse(params)?;
            doc.layers[index].info.blend_mode = p.blend_mode;
        }
        "layer.set_position" => {
            let p: Position = parse(params)?;
            return super::tree::move_position(doc, layer_id, p.x, p.y);
        }
        "layer.move_order" => {
            let p: Order = parse(params)?;
            let parent = doc.layers[index].info.parent;
            let count = doc
                .layers
                .iter()
                .filter(|l| l.info.parent == parent)
                .count();
            if p.index >= count {
                return Err(CommandError::Invalid("sibling index out of range".into()));
            }
            let layer = doc.layers.remove(index);
            let siblings: Vec<_> = doc
                .layers
                .iter()
                .enumerate()
                .filter(|(_, l)| l.info.parent == parent)
                .map(|(i, _)| i)
                .collect();
            let at = if p.index == siblings.len() {
                siblings.last().map_or(doc.layers.len(), |i| i + 1)
            } else {
                siblings[p.index]
            };
            doc.layers.insert(at, layer);
        }
        _ => return Err(CommandError::Invalid(format!("not a batchable edit: {id}"))),
    }
    doc.validate()?;
    Ok(json!({"layer_id":layer_id}))
}

fn batch(doc: &mut Document, params: Value, mut context: Snapshot) -> Result<Value, CommandError> {
    let batch: Batch = parse(params)?;
    if batch.steps.is_empty() || batch.steps.len() > 128 {
        return Err(CommandError::Invalid("batch length must be 1..128".into()));
    }
    for step in &batch.steps {
        let spec = registry::catalog()
            .iter()
            .find(|c| c.id == step.command)
            .ok_or_else(|| CommandError::Invalid("unknown batch step".into()))?;
        if !spec.batchable {
            return Err(CommandError::Invalid(format!(
                "{} cannot be batched",
                step.command
            )));
        }
    }
    let mut results = Vec::new();
    for step in batch.steps {
        let params = resolve_refs(step.params, &results, 0)?;
        let result = if registry::kind(&step.command)? == CommandKind::Query {
            let _: Empty = parse(params)?;
            let layers: Vec<_> = doc.layers().iter().map(|l| l.info().clone()).collect();
            if step.command == "document.histogram" {
                json!(doc.histogram()?)
            } else if step.command == "layer.list" {
                json!({"layers": layers})
            } else {
                context.frame = Some(doc.frame().clone());
                context.selection = doc.selection().map(strata_pixels::TiledMask::layout);
                context.selection_extent = doc
                    .selection()
                    .and_then(|mask| mask.content_bounds().ok().flatten());
                context.selection_summary_available = doc
                    .selection()
                    .is_none_or(|mask| mask.content_bounds().is_ok());
                context.selection_extent_conservative = doc.selection().is_some_and(|mask| {
                    let layout = mask.layout();
                    (layout.default_coverage ^ if layout.inverted { 65535 } else { 0 }) != 0
                });
                context.layers = layers;
                context.bit_depth = Some(doc.bit_depth());
                context.profile_name = Some(doc.profile_name().into());
                context.profile_fallback = doc.profile_fallback();
                context.active_layer = doc.active_layer();
                context.dirty = true;
                json!(&context)
            }
        } else {
            mutate(doc, &step.command, params)?
        };
        results.push(result);
    }
    Ok(json!({"results":results}))
}

fn resolve_refs(value: Value, results: &[Value], depth: usize) -> Result<Value, CommandError> {
    if depth > 64 {
        return Err(CommandError::Invalid(
            "batch reference nesting limit".into(),
        ));
    }
    match value {
        Value::Object(mut map) if map.contains_key("$ref") => {
            if map.len() != 1 {
                return Err(CommandError::Invalid("invalid result reference".into()));
            }
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct Ref {
                step: usize,
                field: String,
            }
            let reference: Ref = parse(map.remove("$ref").unwrap())?;
            results
                .get(reference.step)
                .and_then(|r| r.get(reference.field))
                .cloned()
                .ok_or_else(|| CommandError::Invalid("forward or missing result reference".into()))
        }
        Value::Object(map) => Ok(Value::Object(
            map.into_iter()
                .map(|(k, v)| Ok((k, resolve_refs(v, results, depth + 1)?)))
                .collect::<Result<_, CommandError>>()?,
        )),
        Value::Array(a) => Ok(Value::Array(
            a.into_iter()
                .map(|v| resolve_refs(v, results, depth + 1))
                .collect::<Result<_, _>>()?,
        )),
        v => Ok(v),
    }
}
