//! Pixel filters share the single display-preview slot with tonal adjustments.
//! The editor always retains original truth until one validated candidate wins.
use crate::*;
use std::sync::atomic::Ordering;
pub(crate) struct CachedFilter {
    prepared: strata_core::commands::PreparedCommand,
    parameters: Value,
    sequence: u64,
}
impl CachedFilter {
    pub(crate) fn selection_document(&self) -> Option<&strata_core::Document> {
        // Cached candidate is the last displayed scene even while a newer
        // sequence computes or has invalid parameters. Cancel removes the slot.
        (self.parameters["scope"] == "selection")
            .then(|| self.prepared.document())
            .flatten()
    }
}
fn cancelled() -> StrataError {
    StrataError::Cancelled {
        message: "filter preview superseded or cancelled".into(),
    }
}
fn parameters(json: &str) -> Result<Value, StrataError> {
    if json.len() > 64 * 1024 {
        return Err(render_error("filter parameters exceed preview limit"));
    }
    serde_json::from_str(json).map_err(|e| StrataError::Command {
        message: e.to_string(),
    })
}
#[uniffi::export]
impl StrataEngine {
    pub fn begin_filter_preview(
        &self,
        layer_id: String,
        command: String,
        revision: u64,
        sequence: u64,
    ) -> Result<(), StrataError> {
        if self.tonal_preview_sequence.load(Ordering::Acquire) != sequence {
            return Err(cancelled());
        }
        let layer = serde_json::from_value(json!({"id":layer_id})).map_err(render_error)?;
        let mut inner = self.lock()?;
        if inner.editor.revision() != revision {
            return Err(cancelled());
        }
        if inner.interaction.is_some()
            || inner.marquee.is_some()
            || inner.exposure.is_some()
            || inner.brush.is_some()
            || inner.paint.is_some()
        {
            return Err(render_error(
                "finish the active canvas gesture before filter preview",
            ));
        }
        self.cancel_native_geometry()?;
        crate::tonal_preview::restore(&mut inner)?;
        let interaction = inner
            .editor
            .begin_filter(layer, &command)
            .map_err(command_error)?;
        let document = inner.editor.document().cloned();
        let backup = if let (Some(doc), Some(renderer)) = (document, &mut inner.renderer) {
            Some(
                renderer
                    .prepare_document_cancellable(&doc, false, &|| {
                        self.tonal_preview_sequence.load(Ordering::Acquire) != sequence
                    })
                    .map_err(|e| {
                        if matches!(e, strata_render::RenderError::Cancelled) {
                            cancelled()
                        } else {
                            render_error(e)
                        }
                    })?,
            )
        } else {
            None
        };
        if self.tonal_preview_sequence.load(Ordering::Acquire) != sequence {
            return Err(cancelled());
        }
        inner.tonal_preview = Some(crate::tonal_preview::TonalPreview {
            interaction,
            backup,
            filter: Some(command),
            candidate: None,
        });
        Ok(())
    }
    pub fn preview_filter(&self, params_json: String, sequence: u64) -> Result<(), StrataError> {
        if self.tonal_preview_sequence.load(Ordering::Acquire) != sequence {
            return Err(cancelled());
        }
        let parameters = parameters(&params_json)?;
        let (editor, interaction, command) = {
            let inner = self.lock()?;
            let preview = inner.tonal_preview.as_ref().ok_or_else(cancelled)?;
            (
                inner.editor.clone(),
                preview.interaction.clone(),
                preview.filter.clone().ok_or_else(cancelled)?,
            )
        };
        let scoped = crate::tonal_preview::issued_interaction(self, &interaction, sequence)?;
        let scoped = if command == "layer.snap_position" {
            editor
                .measure_snap_preview(&scoped)
                .map_err(command_error)?
        } else {
            scoped
        };
        let candidate = editor
            .preview_filter(&scoped, &command, parameters.clone())
            .map_err(command_error)?;
        let mut inner = self.lock()?;
        if self.tonal_preview_sequence.load(Ordering::Acquire) != sequence
            || !inner.editor.filter_preview_context_matches(&scoped)
            || candidate.work_expired()
            || inner
                .tonal_preview
                .as_ref()
                .is_none_or(|p| p.filter.as_deref() != Some(command.as_str()))
        {
            return Err(cancelled());
        }
        if let Some(renderer) = &mut inner.renderer {
            let scene = renderer
                .prepare_document_cancellable(
                    candidate.document().expect("filter candidate"),
                    false,
                    &|| {
                        candidate.work_expired()
                            || self.tonal_preview_sequence.load(Ordering::Acquire) != sequence
                    },
                )
                .map_err(|e| {
                    if matches!(e, strata_render::RenderError::Cancelled) {
                        cancelled()
                    } else {
                        render_error(e)
                    }
                })?;
            if candidate.work_expired()
                || self.tonal_preview_sequence.load(Ordering::Acquire) != sequence
            {
                return Err(cancelled());
            }
            renderer.commit_document(scene).map_err(render_error)?;
        }
        let preview = inner.tonal_preview.as_mut().ok_or_else(cancelled)?;
        preview.interaction = scoped;
        preview.candidate = Some(CachedFilter {
            prepared: candidate,
            parameters,
            sequence,
        });
        Ok(())
    }
    pub fn end_filter_preview(&self, params_json: String, cancel: bool) -> Result<(), StrataError> {
        self.finish_filter_preview(params_json, cancel, None)
    }
    pub fn end_filter_preview_checked(
        &self,
        params_json: String,
        cancel: bool,
        sequence: u64,
    ) -> Result<(), StrataError> {
        self.finish_filter_preview(params_json, cancel, Some(sequence))
    }
}

impl StrataEngine {
    fn finish_filter_preview(
        &self,
        params_json: String,
        cancel: bool,
        expected: Option<u64>,
    ) -> Result<(), StrataError> {
        let mut owned =
            expected.unwrap_or_else(|| self.tonal_preview_sequence.load(Ordering::Acquire));
        let result = self.finish_filter_candidate(params_json, cancel, &mut owned);
        if result.is_err()
            && self.tonal_preview_sequence.load(Ordering::Acquire) == owned
            && let Ok(next) = self.advance_preview_sequence(Some(owned))
        {
            let mut inner = self.lock()?;
            if self.tonal_preview_sequence.load(Ordering::Acquire) == next {
                crate::tonal_preview::restore(&mut inner)?;
            }
        }
        result
    }
    fn finish_filter_candidate(
        &self,
        params_json: String,
        cancel: bool,
        owned: &mut u64,
    ) -> Result<(), StrataError> {
        let expected = Some(*owned);
        if expected.is_some_and(|s| s != self.tonal_preview_sequence.load(Ordering::Acquire)) {
            return Err(cancelled());
        }
        if cancel {
            self.advance_preview_sequence(expected)?;
            let mut inner = self.lock()?;
            crate::tonal_preview::restore(&mut inner)?;
            return Ok(());
        }
        let parameters = parameters(&params_json)?;
        let ready = {
            let inner = self.lock()?;
            let preview = inner.tonal_preview.as_ref().ok_or_else(cancelled)?;
            preview.candidate.as_ref().is_some_and(|p| {
                p.parameters == parameters
                    && p.sequence == self.tonal_preview_sequence.load(Ordering::Acquire)
                    && !p.prepared.work_expired()
            })
        };
        if !ready {
            let sequence = self.advance_preview_sequence(expected)?;
            *owned = sequence;
            self.preview_filter(params_json, sequence)?;
        }
        let mut inner = self.lock()?;
        let mut preview = inner.tonal_preview.take().ok_or_else(cancelled)?;
        let result = (|| {
            if preview.filter.is_none()
                || !inner
                    .editor
                    .filter_preview_context_matches(&preview.interaction)
            {
                return Err(cancelled());
            }
            let candidate = preview.candidate.take().ok_or_else(cancelled)?;
            if candidate.parameters != parameters
                || candidate.sequence != self.tonal_preview_sequence.load(Ordering::Acquire)
                || candidate.prepared.work_expired()
            {
                return Err(cancelled());
            }
            // Display already holds this exact candidate. Publishing document
            // truth must not re-evaluate a filter or accumulate the preview.
            inner
                .editor
                .commit(candidate.prepared)
                .map_err(command_error)?;
            let snapshot_json = json!(inner.editor.snapshot()).to_string();
            inner
                .events
                .push_back(EngineEventDto::StateChanged { snapshot_json });
            Ok(())
        })();
        if result.is_err()
            && inner.editor.tonal_context_matches(&preview.interaction)
            && let (Some(backup), Some(renderer)) = (preview.backup, &mut inner.renderer)
        {
            renderer
                .restore_preview_scene(backup)
                .map_err(render_error)?;
        }
        self.issue_tonal_preview()?;
        drop(inner);
        result?;
        self.deliver_events()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    struct Listener(AtomicUsize);
    impl EngineListener for Listener {
        fn on_event(&self, _: EngineEventDto) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }
    fn engine() -> (Arc<StrataEngine>, Arc<Listener>) {
        let listener = Arc::new(Listener(AtomicUsize::new(0)));
        let engine = StrataEngine::new(listener.clone());
        engine.dispatch("file.new".into(),json!({"width":16,"height":16,"bit_depth":16,"background":[20000,30000,40000,32768]}).to_string()).unwrap();
        engine.dispatch("paint.stroke".into(),json!({"layer":{"name":"Background"},"points":[{"x":8.5,"y":8.5}],"size":4,"hardness":1,"color":[1,0.2,0.1,1]}).to_string()).unwrap();
        (engine, listener)
    }
    fn start(e: &StrataEngine, command: &str) -> u64 {
        let state = e.lock().unwrap().editor.snapshot();
        let sequence = e.issue_tonal_preview().unwrap();
        e.begin_filter_preview(
            state.active_layer.unwrap().to_string(),
            command.into(),
            state.revision,
            sequence,
        )
        .unwrap();
        sequence
    }
    #[test]
    fn preview_preserves_truth_cancel_restores_and_apply_commits_exact_candidate_once() {
        for (command, params) in [
            ("filter.gaussian_blur", json!({"sigma":0.8})),
            ("filter.unsharp_mask", json!({"sigma":0.8,"amount":1.2})),
            ("filter.high_pass", json!({"sigma":0.8})),
            ("filter.high_pass", json!({"sigma":0.8,"space":"linear_p3"})),
            ("filter.add_noise", json!({"amount":0.08,"seed":42})),
            (
                "image.auto",
                json!({"settings":{"method":"contrast","clip_percent":0}}),
            ),
            (
                "image.auto",
                json!({"settings":{"method":"tone","clip_percent":0}}),
            ),
            (
                "image.auto",
                json!({"settings":{"method":"color","clip_percent":0,"neutralize_strength":0.7}}),
            ),
            (
                "transform.apply",
                json!({"scope":"whole_layer","geometry":{"type":"components","translation":[0.5,0.25]}}),
            ),
            (
                "transform.apply",
                json!({"scope":"selection","geometry":{"type":"components","translation":[4.5,0.25]}}),
            ),
        ] {
            let (e, listener) = engine();
            if params["scope"] == "selection" {
                e.dispatch(
                    "select.marquee".into(),
                    json!({"x":3,"y":3,"width":6,"height":6}).to_string(),
                )
                .unwrap();
            }
            let before = e
                .lock()
                .unwrap()
                .editor
                .document()
                .unwrap()
                .to_archive()
                .unwrap();
            let history = e.lock().unwrap().editor.snapshot().history.len();
            let events = listener.0.load(Ordering::Relaxed);
            let sequence = start(&e, command);
            e.preview_filter(params.to_string(), sequence).unwrap();
            assert!(
                e.lock()
                    .unwrap()
                    .editor
                    .document()
                    .unwrap()
                    .to_archive()
                    .unwrap()
                    == before
            );
            assert_eq!(listener.0.load(Ordering::Relaxed), events);
            let staged = e
                .lock()
                .unwrap()
                .tonal_preview
                .as_ref()
                .unwrap()
                .candidate
                .as_ref()
                .unwrap()
                .prepared
                .document()
                .unwrap()
                .to_archive()
                .unwrap();
            assert!(staged.tiles != before.tiles);
            e.end_filter_preview("{}".into(), true).unwrap();
            assert!(
                e.lock()
                    .unwrap()
                    .editor
                    .document()
                    .unwrap()
                    .to_archive()
                    .unwrap()
                    == before
            );
            let sequence = start(&e, command);
            e.preview_filter(params.to_string(), sequence).unwrap();
            let staged = e
                .lock()
                .unwrap()
                .tonal_preview
                .as_ref()
                .unwrap()
                .candidate
                .as_ref()
                .unwrap()
                .prepared
                .document()
                .unwrap()
                .to_archive()
                .unwrap();
            e.end_filter_preview_checked(params.to_string(), false, sequence)
                .unwrap();
            let inner = e.lock().unwrap();
            assert!(inner.editor.document().unwrap().to_archive().unwrap() == staged);
            assert_eq!(inner.editor.snapshot().history.len(), history + 1);
            drop(inner);
            assert_eq!(listener.0.load(Ordering::Relaxed), events + 1);
            e.dispatch("edit.undo".into(), "{}".into()).unwrap();
            assert!(
                e.lock()
                    .unwrap()
                    .editor
                    .document()
                    .unwrap()
                    .to_archive()
                    .unwrap()
                    == before
            );
        }
    }
    #[test]
    fn selected_preview_outline_moves_without_mutating_truth_and_cancel_restores_it() {
        let (e, _) = engine();
        e.dispatch(
            "select.marquee".into(),
            json!({"x":2,"y":2,"width":4,"height":4}).to_string(),
        )
        .unwrap();
        let revision = e.lock().unwrap().editor.revision();
        let truth = e.selection_outline(revision).unwrap();
        let before = e.snapshot_json().unwrap();
        let sequence = start(&e, "transform.apply");
        e.preview_filter(
            json!({"scope":"selection","geometry":{"type":"components","translation":[6,1]}})
                .to_string(),
            sequence,
        )
        .unwrap();
        let shown = e.display_selection_outline(revision).unwrap();
        assert_ne!(shown, truth);
        assert_eq!(e.selection_outline(revision).unwrap(), truth);
        assert_eq!(e.snapshot_json().unwrap(), before);
        assert!(shown.iter().all(|edge| edge.x1 >= 8. && edge.x2 >= 8.));
        let newer = e.issue_tonal_preview().unwrap();
        assert_eq!(
            e.display_selection_outline(revision).unwrap(),
            shown,
            "last displayed candidate remains while newer input is pending"
        );
        e.end_filter_preview_checked("{}".into(), true, newer)
            .unwrap();
        assert_eq!(e.display_selection_outline(revision).unwrap(), truth);
        assert_eq!(e.snapshot_json().unwrap(), before);
    }
    #[test]
    fn invalid_and_superseded_apply_cannot_revive_work_or_overwrite_a_new_command() {
        let (e, _) = engine();
        let original = e
            .lock()
            .unwrap()
            .editor
            .document()
            .unwrap()
            .to_archive()
            .unwrap();
        let sequence = start(&e, "filter.gaussian_blur");
        e.preview_filter(json!({"sigma":0.8}).to_string(), sequence)
            .unwrap();
        let newer = e.issue_tonal_preview().unwrap();
        assert!(matches!(
            e.end_filter_preview_checked(json!({"sigma":0.8}).to_string(), false, sequence),
            Err(StrataError::Cancelled { .. })
        ));
        assert!(
            e.preview_filter(json!({"sigma":65}).to_string(), newer)
                .is_err()
        );
        assert!(
            e.lock()
                .unwrap()
                .editor
                .document()
                .unwrap()
                .to_archive()
                .unwrap()
                == original
        );
        e.preview_filter(json!({"sigma":1.2}).to_string(), newer)
            .unwrap();
        e.dispatch("select.all".into(), "{}".into()).unwrap();
        let history = e.lock().unwrap().editor.snapshot().history.len();
        assert!(
            e.end_filter_preview(json!({"sigma":1.2}).to_string(), false)
                .is_err()
        );
        assert_eq!(e.lock().unwrap().editor.snapshot().history.len(), history);
    }
    #[test]
    fn metal_preview_failure_restores_original_scene_and_cached_apply_keeps_display() {
        for (command, params) in [
            ("filter.gaussian_blur", json!({"sigma":0.8})),
            ("filter.unsharp_mask", json!({"sigma":0.8,"amount":1.2})),
            ("filter.high_pass", json!({"sigma":0.8})),
            ("filter.high_pass", json!({"sigma":0.8,"space":"linear_p3"})),
            ("filter.add_noise", json!({"amount":0.08,"seed":42})),
        ] {
            let mut renderer =
                match strata_render::Renderer::new_offscreen(strata_render::Viewport {
                    width: 64,
                    height: 64,
                    scale: 1.0,
                }) {
                    Ok(r) => r,
                    Err(strata_render::RenderError::NoAdapter)
                        if std::env::var_os("STRATA_REQUIRE_GPU").is_none() =>
                    {
                        return;
                    }
                    Err(e) => panic!("GPU: {e}"),
                };
            let (e, _) = engine();
            {
                let mut inner = e.lock().unwrap();
                let scene = renderer
                    .prepare_document(inner.editor.document().unwrap(), false)
                    .unwrap();
                renderer.commit_document(scene).unwrap();
                renderer
                    .set_view(strata_render::ViewTransform {
                        zoom: 3.0,
                        center_x: 8.0,
                        center_y: 8.0,
                    })
                    .unwrap();
                inner.renderer = Some(renderer);
            }
            let frame = |e: &StrataEngine| {
                e.render_frame().unwrap();
                e.lock()
                    .unwrap()
                    .renderer
                    .as_ref()
                    .unwrap()
                    .read_offscreen_rgba()
                    .unwrap()
            };
            let original = frame(&e);
            let before = e.snapshot_json().unwrap();
            let sequence = start(&e, command);
            e.preview_filter(params.to_string(), sequence).unwrap();
            let staged = frame(&e);
            assert!(staged != original);
            let invalid = if command == "filter.add_noise" {
                json!({"amount":2})
            } else {
                json!({"sigma":65})
            };
            assert!(e.end_filter_preview(invalid.to_string(), false).is_err());
            assert!(e.lock().unwrap().tonal_preview.is_none());
            assert!(frame(&e) == original);
            assert_eq!(e.snapshot_json().unwrap(), before);
            let sequence = start(&e, command);
            e.preview_filter(params.to_string(), sequence).unwrap();
            let staged = frame(&e);
            e.end_filter_preview_checked(params.to_string(), false, sequence)
                .unwrap();
            assert!(frame(&e) == staged);
            e.dispatch("edit.undo".into(), "{}".into()).unwrap();
            assert!(frame(&e) == original);
        }
    }
    #[test]
    fn newer_sequence_cancels_filter_math_without_taking_the_editor_lock() {
        let (e, _) = engine();
        e.dispatch("file.new".into(),json!({"width":512,"height":512,"bit_depth":16,"background":[20000,30000,40000,65535],"discard_changes":true}).to_string()).unwrap();
        let sequence = start(&e, "filter.gaussian_blur");
        let work = e.clone();
        let worker = std::thread::spawn(move || {
            work.preview_filter(json!({"sigma":8}).to_string(), sequence)
        });
        let pool = e.lock().unwrap().editor.document().unwrap().memory_pool();
        let start = std::time::Instant::now();
        while pool.stats().transient_bytes < 8 * 1024 * 1024
            && start.elapsed() < std::time::Duration::from_secs(2)
        {
            std::thread::yield_now();
        }
        let elapsed = std::time::Instant::now();
        e.issue_tonal_preview().unwrap();
        assert!(elapsed.elapsed() < std::time::Duration::from_millis(100));
        assert!(matches!(
            worker.join().unwrap(),
            Err(StrataError::Cancelled { .. })
        ));
        assert_eq!(e.lock().unwrap().editor.snapshot().history.len(), 0);
        e.end_filter_preview("{}".into(), true).unwrap();
    }
}
