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
}

impl Request {
    /// Trusted callers may shorten a request's commit deadline. Model parameters
    /// cannot construct or extend it. Checked again after fallible preparation.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.deadline = self
            .deadline
            .min(Instant::now() + timeout.min(Duration::from_secs(3600)));
        self
    }
    fn expired(&self) -> bool {
        Instant::now() >= self.deadline || self.cancellation.load(Ordering::Acquire)
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
pub struct ExposureInteraction {
    request: Request,
    epoch: u64,
    layer: LayerId,
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

enum Action {
    Query,
    View,
    New,
    Edit,
    Place(String),
    History(usize),
    Open(Option<std::path::PathBuf>),
    Recover,
    External(Box<WriteCandidate>),
}

/// An opaque staged transaction. The host can prepare display resources before
/// commit; no public method can mutate its candidate.
pub struct PreparedCommand {
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
}
impl PreparedCommand {
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
                | Action::Open(_)
                | Action::Recover
                | Action::Edit
                | Action::Place(_)
                | Action::History(_)
        ) && !self.replay
            && !self.command.starts_with("select.")
    }
    pub fn replaces_document(&self) -> bool {
        matches!(self.action, Action::New | Action::Open(_) | Action::Recover)
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
    resources: HashMap<String, Resource>,
    recent: Vec<RecentFile>,
    executions: Vec<Value>,
    control_events: Vec<Value>,
    decoder: Option<std::sync::Arc<dyn strata_io::PlatformDecoder>>,
    selector: Option<Arc<dyn crate::SubjectSelector>>,
    cancellation: Arc<AtomicBool>,
}
impl Default for Editor {
    fn default() -> Self {
        Self::new()
    }
}
impl Editor {
    pub fn new() -> Self {
        Self {
            pool: strata_pixels::memory::MemoryPool::global(),
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
            resources: HashMap::new(),
            recent: Vec::new(),
            executions: Vec::new(),
            control_events: Vec::new(),
            decoder: None,
            selector: None,
            cancellation: Arc::new(AtomicBool::new(false)),
        }
    }
    pub fn with_decoder(decoder: std::sync::Arc<dyn strata_io::PlatformDecoder>) -> Self {
        Self {
            decoder: Some(decoder),
            ..Self::new()
        }
    }
    pub fn set_subject_selector(&mut self, selector: Arc<dyn crate::SubjectSelector>) {
        self.selector = Some(selector);
    }
    pub fn with_memory_pool(pool: strata_pixels::memory::MemoryPool) -> Self {
        Self {
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
            call_id: Uuid::new_v4(),
            source,
            session: self.session,
            document: self.document.as_ref().map(Document::id),
            revision: self.revision,
            issued: Instant::now(),
            deadline: Instant::now() + Duration::from_secs(3600),
            cancellation: self.cancellation.clone(),
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
    pub fn prepare_marquee(
        &self,
        interaction: &MarqueeInteraction,
        params: Value,
    ) -> Result<PreparedCommand, CommandError> {
        if interaction.epoch != self.epoch {
            return Err(CommandError::Stale);
        }
        self.prepare(interaction.request.clone(), "select.marquee", params)
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
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
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
        self.cancellation = Arc::new(AtomicBool::new(false));
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
        self.cancellation.store(true, Ordering::Release);
        self.cancellation = Arc::new(AtomicBool::new(false));
        self.control = Control::User;
        self.epoch += 1;
    }
    pub fn cancel_pending_work(&mut self) {
        self.cancellation.store(true, Ordering::Release);
        self.cancellation = Arc::new(AtomicBool::new(false));
        self.epoch += 1;
    }
    fn authorize(&self, source: &Source, id: &str, kind: CommandKind) -> Result<(), CommandError> {
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
            });
        }
        self.authorize(&request.source, id, kind)?;
        self.check_request(&request)?;
        // View/query calls run for every display-link frame. They never need a
        // copy of every layer's tile index; edits alone stage a COW document.
        let mut candidate = if kind == CommandKind::Mutate {
            self.document.clone()
        } else {
            None
        };
        let mut selected = self.selected;
        let mut read_resource = None;
        let (action, result) = match kind {
            CommandKind::Query => {
                let _: Empty = parse(params.clone())?;
                (
                    Action::Query,
                    if id == "file.recent.list" {
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
            CommandKind::Lifecycle if id == "file.open" || id == "file.recover" => {
                let (doc, path) =
                    self.prepare_open(&request, params.clone(), id == "file.recover")?;
                read_resource = Some(parse::<OpenFile>(params.clone())?.resource);
                selected = doc.active_layer();
                let result = json!({"document_id":doc.id(),"layer_id":selected});
                candidate = Some(doc);
                (
                    if id == "file.recover" {
                        Action::Recover
                    } else {
                        Action::Open(path)
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
                let (action, result) = if id == "select.subject" {
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
                selected = candidate.as_ref().and_then(Document::active_layer);
                (Action::History(next), json!({"history_cursor":next}))
            }
            CommandKind::View => {
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
        })
    }
    pub fn commit(&mut self, prepared: PreparedCommand) -> Result<Value, CommandError> {
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
            return Ok(old.result.clone());
        }
        if prepared.replay {
            return Err(CommandError::Expired);
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
        self.authorize(
            &prepared.request.source,
            &prepared.command,
            registry::kind(&prepared.command)?,
        )?;
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
                | Action::Open(_)
                | Action::Recover
                | Action::Edit
                | Action::Place(_)
                | Action::History(_)
        ) && self.revision == u64::MAX
        {
            return Err(CommandError::Invalid("revision exhausted".into()));
        }
        if let Action::Place(resource) = &prepared.action {
            self.resource(resource, &prepared.request, false)?;
        }
        if let Some(resource) = &prepared.read_resource {
            self.resource(resource, &prepared.request, false)?;
        }
        match prepared.action {
            Action::External(write) => {
                self.commit_write(&prepared.request, &prepared.command, *write)?;
            }
            Action::Open(path) => {
                self.saved = prepared.candidate.as_ref().map(Document::state_id);
                self.document = prepared.candidate;
                self.history.clear();
                self.cursor = 0;
                self.project_path = path;
                self.revision += 1;
            }
            Action::Recover => {
                self.document = prepared.candidate;
                self.saved = None;
                self.history.clear();
                self.cursor = 0;
                self.project_path = None;
                self.revision += 1;
            }
            Action::Edit | Action::Place(_) => {
                let after = prepared.candidate.clone().ok_or(CommandError::NoDocument)?;
                let before = self.document.clone().ok_or(CommandError::NoDocument)?;
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
                    after,
                });
                if self.history.len() > 64 {
                    self.history.remove(0);
                }
                self.cursor = self.history.len();
                self.document = prepared.candidate;
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
                self.document = prepared.candidate;
                self.saved = None;
                self.history.clear();
                self.cursor = 0;
                self.project_path = None;
                self.revision += 1;
            }
            Action::History(cursor) => {
                if let Control::Paused { turn_id } = self.control {
                    self.control_events.push(json!({"turn_id":turn_id,"command":prepared.command,"from_cursor":self.cursor,"to_cursor":cursor,"revision":self.revision+1}));
                    if self.control_events.len() > 256 {
                        self.control_events.remove(0);
                    }
                }
                self.document = prepared.candidate;
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
        self.selected = prepared.selected;
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
        Ok(prepared.result)
    }
}

pub(super) fn mutate(doc: &mut Document, id: &str, params: Value) -> Result<Value, CommandError> {
    if let Some(result) = super::local::mutate(doc, id, params.clone())? {
        return Ok(result);
    }
    if id == "color.convert" {
        let p: ConvertColor = parse(params)?;
        let icc = strata_color::working_profile(&p.space).map_err(DocumentError::from)?;
        let pool = doc.memory_pool();
        for layer in &mut doc.layers {
            let mut converted =
                TiledPixels::with_pool(doc.format, tile_limit(doc.format), pool.clone());
            for (coord, tile) in layer.pixels.tiles() {
                let tile = tile.map_err(DocumentError::from)?;
                let pixels =
                    strata_color::convert_to_profile(&tile, 256, 256, &doc.working_icc, &icc)
                        .map_err(DocumentError::from)?;
                converted
                    .insert_tile(*coord, pixels)
                    .map_err(DocumentError::from)?;
            }
            layer.pixels = converted;
        }
        let profile = strata_color::resolve_profile(Some(&icc)).map_err(DocumentError::from)?;
        doc.working_icc = icc;
        doc.profile_name = profile.name;
        doc.profile_fallback = false;
        return Ok(json!({"working_space":p.space}));
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
            doc.layers.remove(index);
            if doc.active == Some(layer_id) {
                doc.active = doc.layers.last().map(Layer::id);
            }
        }
        "layer.duplicate" => {
            let _: Target = parse(params)?;
            if doc.layers.len() >= crate::MAX_LAYERS {
                return Err(DocumentError::ResourceLimit.into());
            }
            let mut copy = doc.layers[index].clone();
            copy.info.id = LayerId::fresh();
            copy.info.name = format!("{} copy", copy.info.name);
            let id = copy.id();
            doc.layers.insert(index + 1, copy);
            doc.active = Some(id);
            return Ok(json!({"layer_id":id}));
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
            doc.layers[index].info.x = p.x;
            doc.layers[index].info.y = p.y;
        }
        "layer.move_order" => {
            let p: Order = parse(params)?;
            if p.index >= doc.layers.len() {
                return Err(CommandError::Invalid("layer index out of range".into()));
            }
            let layer = doc.layers.remove(index);
            doc.layers.insert(p.index, layer);
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
