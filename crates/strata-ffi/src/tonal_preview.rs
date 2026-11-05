//! Display-only user preview. It never enters command history until applied.
use crate::*;
use std::sync::atomic::Ordering;
use strata_core::commands::TonalInteraction;
pub(crate) struct TonalPreview {
    interaction: TonalInteraction,
    backup: Option<strata_render::PreparedScene>,
}
fn cancelled() -> StrataError {
    StrataError::Cancelled {
        message: "tonal preview superseded or cancelled".into(),
    }
}
fn settings(value: &str) -> Result<strata_core::TonalAdjustment, StrataError> {
    if value.len() > 64 * 1024 {
        return Err(render_error("tonal settings exceed preview limit"));
    }
    serde_json::from_str(value).map_err(|e| StrataError::Command {
        message: e.to_string(),
    })
}
pub(crate) fn restore(inner: &mut Inner) -> Result<(), StrataError> {
    if let Some(preview) = inner.tonal_preview.take()
        && inner.editor.tonal_context_matches(&preview.interaction)
        && let (Some(backup), Some(renderer)) = (preview.backup, &mut inner.renderer)
    {
        renderer
            .restore_preview_scene(backup)
            .map_err(render_error)?;
    }
    Ok(())
}
#[uniffi::export]
impl StrataEngine {
    /// Main-thread safe: only publishes a monotonic sequence, never waits on
    /// composition. A worker must present this exact sequence to swap display.
    pub fn issue_tonal_preview(&self) -> Result<u64, StrataError> {
        self.tonal_preview_sequence
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |v| v.checked_add(1))
            .map(|v| v + 1)
            .map_err(|_| render_error("tonal preview sequence exhausted"))
    }
    pub fn begin_tonal_preview(
        &self,
        layer_id: String,
        revision: u64,
        sequence: u64,
    ) -> Result<(), StrataError> {
        if self.tonal_preview_sequence.load(Ordering::Acquire) != sequence {
            return Err(cancelled());
        }
        let target = serde_json::from_value(json!({"id":layer_id})).map_err(render_error)?;
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
            return Err(StrataError::Command {
                message: "finish the active canvas gesture before tonal preview".into(),
            });
        }
        restore(&mut inner)?;
        let interaction = inner.editor.begin_tonal(target).map_err(command_error)?;
        let doc = inner.editor.document().cloned();
        let backup = if let (Some(doc), Some(renderer)) = (doc, &mut inner.renderer) {
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
        inner.tonal_preview = Some(TonalPreview {
            interaction,
            backup,
        });
        Ok(())
    }
    pub fn preview_tonal(&self, settings_json: String, sequence: u64) -> Result<(), StrataError> {
        if self.tonal_preview_sequence.load(Ordering::Acquire) != sequence {
            return Err(cancelled());
        }
        let settings = settings(&settings_json)?;
        let (editor, interaction) = {
            let inner = self.lock()?;
            let preview = inner.tonal_preview.as_ref().ok_or_else(cancelled)?;
            (inner.editor.clone(), preview.interaction.clone())
        };
        let candidate = editor
            .preview_tonal(&interaction, settings)
            .map_err(command_error)?;
        let mut inner = self.lock()?;
        if self.tonal_preview_sequence.load(Ordering::Acquire) != sequence
            || !inner.editor.tonal_context_matches(&interaction)
            || candidate.work_expired()
        {
            return Err(cancelled());
        }
        if let Some(renderer) = &mut inner.renderer {
            let scene = renderer
                .prepare_document_cancellable(
                    candidate.document().expect("tonal candidate"),
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
            if self.tonal_preview_sequence.load(Ordering::Acquire) != sequence
                || candidate.work_expired()
            {
                return Err(cancelled());
            }
            renderer.commit_document(scene).map_err(render_error)?;
        }
        Ok(())
    }
    pub fn end_tonal_preview(
        &self,
        settings_json: String,
        cancel: bool,
    ) -> Result<(), StrataError> {
        self.issue_tonal_preview()?;
        {
            let mut inner = self.lock()?;
            let Some(preview) = inner.tonal_preview.take() else {
                return if cancel { Ok(()) } else { Err(cancelled()) };
            };
            if cancel {
                if inner.editor.tonal_context_matches(&preview.interaction)
                    && let (Some(backup), Some(renderer)) = (preview.backup, &mut inner.renderer)
                {
                    renderer
                        .restore_preview_scene(backup)
                        .map_err(render_error)?;
                }
            } else {
                let result = (|| {
                    let settings = settings(&settings_json)?;
                    let candidate = inner
                        .editor
                        .preview_tonal(&preview.interaction, settings)
                        .map_err(command_error)?;
                    apply(&mut inner, candidate, "adjust.tonal.set", &json!({}))?;
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
                result?;
            }
        }
        self.deliver_events()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Listener;
    impl EngineListener for Listener {
        fn on_event(&self, _: EngineEventDto) {}
    }
    fn engine() -> Arc<StrataEngine> {
        let e = StrataEngine::new(Arc::new(Listener));
        e.dispatch(
            "file.new".into(),
            json!({"width":16,"height":16,"bit_depth":16,"background":[20000,20001,20002,65535]})
                .to_string(),
        )
        .unwrap();
        e.dispatch(
            "layer.add_tonal_adjustment".into(),
            json!({"name":"tone","adjustment":{"type":"brightness_contrast"}}).to_string(),
        )
        .unwrap();
        e
    }
    fn start(e: &StrataEngine) -> u64 {
        let state = e.lock().unwrap().editor.snapshot();
        let sequence = e.issue_tonal_preview().unwrap();
        e.begin_tonal_preview(
            state.active_layer.unwrap().to_string(),
            state.revision,
            sequence,
        )
        .unwrap();
        sequence
    }
    fn parameters(v: i32) -> String {
        json!({"type":"brightness_contrast","brightness":v}).to_string()
    }
    #[test]
    fn preview_never_changes_saved_truth_and_apply_adds_one_history_entry() {
        let e = engine();
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("before.strata");
        let write = e
            .grant_write(path.to_string_lossy().into_owned(), false)
            .unwrap();
        e.dispatch("file.save_as".into(), json!({"resource":write}).to_string())
            .unwrap();
        let before = e.snapshot_json().unwrap();
        let archive = e
            .lock()
            .unwrap()
            .editor
            .document()
            .unwrap()
            .to_archive()
            .unwrap();
        let mut sequence = start(&e);
        for value in [5, 10, 15] {
            e.preview_tonal(parameters(value), sequence).unwrap();
            sequence = e.issue_tonal_preview().unwrap();
        }
        assert_eq!(e.snapshot_json().unwrap(), before);
        assert!(
            e.lock()
                .unwrap()
                .editor
                .document()
                .unwrap()
                .to_archive()
                .unwrap()
                == archive
        );
        let history = e.lock().unwrap().editor.snapshot().history.len();
        e.end_tonal_preview(parameters(20), false).unwrap();
        let mut inner = e.lock().unwrap();
        assert!(inner.editor.dirty());
        assert_eq!(inner.editor.snapshot().history.len(), history + 1);
        let state = &inner
            .editor
            .document()
            .unwrap()
            .layers()
            .last()
            .unwrap()
            .info()
            .kind;
        assert!(
            matches!(state,strata_core::LayerKind::Adjustment{adjustment:strata_core::TonalAdjustment::BrightnessContrast{brightness,..}}if *brightness==20.0)
        );
        let request = inner.editor.request(Source::User);
        inner
            .editor
            .execute(request, "edit.undo", json!({}), |_| Ok(()))
            .unwrap();
        assert!(!inner.editor.dirty());
    }
    #[test]
    fn stale_sequences_cancel_restore_and_other_commands_revoke_preview() {
        let e = engine();
        let before = e.snapshot_json().unwrap();
        let old = start(&e);
        let current = e.issue_tonal_preview().unwrap();
        assert!(matches!(
            e.preview_tonal(parameters(10), old),
            Err(StrataError::Cancelled { .. })
        ));
        e.preview_tonal(parameters(20), current).unwrap();
        e.cancel_pending_work().unwrap();
        e.end_tonal_preview("{}".into(), true).unwrap();
        assert_eq!(e.snapshot_json().unwrap(), before);
        let sequence = start(&e);
        e.preview_tonal(parameters(15), sequence).unwrap();
        e.dispatch(
            "layer.create".into(),
            json!({"name":"normal edit"}).to_string(),
        )
        .unwrap();
        assert!(e.lock().unwrap().tonal_preview.is_none());
        assert!(e.end_tonal_preview(parameters(20), false).is_err());
        assert!(matches!(
            e.preview_tonal(parameters(20), sequence),
            Err(StrataError::Cancelled { .. })
        ));
    }
    #[test]
    fn real_display_preview_cancel_restores_pinned_scene_without_touching_document() {
        let mut renderer = match Renderer::new_offscreen(Viewport {
            width: 64,
            height: 64,
            scale: 1.0,
        }) {
            Ok(r) => r,
            Err(strata_render::RenderError::NoAdapter)
                if std::env::var_os("STRATA_REQUIRE_GPU").is_none() =>
            {
                eprintln!("SKIPPED GPU TEST: no adapter");
                return;
            }
            Err(e) => panic!("GPU: {e}"),
        };
        let e = engine();
        let before = e.snapshot_json().unwrap();
        {
            let mut inner = e.lock().unwrap();
            let scene = renderer
                .prepare_document(inner.editor.document().unwrap(), true)
                .unwrap();
            renderer.commit_document(scene).unwrap();
            inner.renderer = Some(renderer);
        }
        e.render_frame().unwrap();
        let initial = e
            .lock()
            .unwrap()
            .renderer
            .as_ref()
            .unwrap()
            .read_offscreen_rgba()
            .unwrap();
        let seq = start(&e);
        e.preview_tonal(parameters(20), seq).unwrap();
        e.render_frame().unwrap();
        let brighter = e
            .lock()
            .unwrap()
            .renderer
            .as_ref()
            .unwrap()
            .read_offscreen_rgba()
            .unwrap();
        assert_ne!(brighter, initial);
        assert_eq!(e.snapshot_json().unwrap(), before);
        let seq = e.issue_tonal_preview().unwrap();
        assert!(
            e.preview_tonal(
                "{\"type\":\"brightness_contrast\",\"brightness\":500}".into(),
                seq
            )
            .is_err()
        );
        e.end_tonal_preview("{}".into(), true).unwrap();
        e.render_frame().unwrap();
        assert_eq!(
            e.lock()
                .unwrap()
                .renderer
                .as_ref()
                .unwrap()
                .read_offscreen_rgba()
                .unwrap(),
            initial
        );
        assert_eq!(e.snapshot_json().unwrap(), before);
        let seq = start(&e);
        e.preview_tonal(parameters(15), seq).unwrap();
        e.begin_marquee().unwrap();
        e.end_marquee(json!({"x":1,"y":1,"width":4,"height":4}).to_string(), false)
            .unwrap();
        e.render_frame().unwrap();
        assert_eq!(
            e.lock()
                .unwrap()
                .renderer
                .as_ref()
                .unwrap()
                .read_offscreen_rgba()
                .unwrap(),
            initial,
            "selection-only edit left an uncommitted tonal display active"
        );
        let seq = start(&e);
        e.preview_tonal(parameters(10), seq).unwrap();
        e.pan_by(1.0, 2.0).unwrap();
        let camera = e.lock().unwrap().renderer.as_ref().unwrap().view();
        e.end_tonal_preview("{}".into(), true).unwrap();
        assert_eq!(e.lock().unwrap().renderer.as_ref().unwrap().view(), camera);
    }
}
