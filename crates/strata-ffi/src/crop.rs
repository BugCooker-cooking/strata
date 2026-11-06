//! Native crop drafts have their own cancellable ownership token. Border-only
//! staging never touches editor truth, selection, renderer pixels or history.
use crate::*;
pub(crate) struct NativeCrop {
    token: String,
    interaction: strata_core::commands::CropInteraction,
    work: strata_core::commands::WorkCancellation,
    finishing: bool,
}
impl NativeCrop {
    pub(crate) fn cancel(&self) {
        self.work.cancel();
    }
}
fn cancelled() -> StrataError {
    StrataError::Cancelled {
        message: "crop draft is stale, superseded or cancelled".into(),
    }
}
#[uniffi::export]
impl StrataEngine {
    pub fn begin_crop(&self, document_id: String, revision: u64) -> Result<String, StrataError> {
        let mut inner = self.lock()?;
        if inner.editor.revision() != revision
            || inner
                .editor
                .document()
                .is_none_or(|d| d.id().to_string() != document_id)
        {
            return Err(cancelled());
        }
        if inner.interaction.is_some()
            || inner.marquee.is_some()
            || inner.exposure.is_some()
            || inner.brush.is_some()
            || inner.paint.is_some()
        {
            return Err(render_error("finish the active canvas gesture before crop"));
        }
        let interaction = inner.editor.begin_crop().map_err(command_error)?;
        self.issue_tonal_preview()?;
        crate::tonal_preview::restore(&mut inner)?;
        let token = uuid::Uuid::new_v4().to_string();
        let mut slot = self.crop_session.lock().map_err(render_error)?;
        if let Some(old) = slot.take() {
            old.work.cancel();
        }
        *slot = Some(NativeCrop {
            token: token.clone(),
            interaction,
            work: Default::default(),
            finishing: false,
        });
        Ok(token)
    }
    /// Cancellation acquires no editor/render lock. A late cancellation token
    /// cannot revoke another crop or a newer AI request.
    pub fn end_crop(
        &self,
        token: String,
        params_json: String,
        cancel: bool,
    ) -> Result<(), StrataError> {
        let interaction = {
            let mut slot = self.crop_session.lock().map_err(render_error)?;
            let value = slot
                .as_mut()
                .filter(|c| c.token == token)
                .ok_or_else(cancelled)?;
            if cancel {
                value.work.cancel();
                *slot = None;
                return Ok(());
            }
            if value.finishing {
                return Err(cancelled());
            }
            value.finishing = true;
            value.interaction.with_work_cancellation(&value.work)
        };
        let result = (|| {
            if params_json.len() > 64 * 1024 {
                return Err(render_error("crop parameter limit"));
            }
            let params: Value = serde_json::from_str(&params_json).map_err(render_error)?;
            let editor = self.lock()?.editor.clone();
            let prepared = editor
                .prepare_crop(&interaction, params.clone())
                .map_err(command_error)?;
            let mut inner = self.lock()?;
            if self
                .crop_session
                .lock()
                .map_err(render_error)?
                .as_ref()
                .is_none_or(|c| c.token != token)
            {
                return Err(cancelled());
            }
            apply(&mut inner, prepared, "image.crop", &params)?;
            Ok(())
        })();
        {
            let mut slot = self.crop_session.lock().map_err(render_error)?;
            if slot.as_ref().is_some_and(|c| c.token == token) {
                *slot = None;
            }
        }
        result?;
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
            json!({"width":16,"height":16,"bit_depth":16,"background":[12001,30003,50001,32769]})
                .to_string(),
        )
        .unwrap();
        e
    }
    fn start(e: &StrataEngine) -> String {
        let state = e.lock().unwrap().editor.snapshot();
        e.begin_crop(state.document_id.unwrap().to_string(), state.revision)
            .unwrap()
    }
    fn params() -> String {
        json!({"rect":{"x":3,"y":2,"width":9,"height":8},"pixels":"preserve","aspect_ratio":[4,3]})
            .to_string()
    }
    #[test]
    fn border_draft_cancel_and_supersession_keep_truth_and_only_one_owned_commit() {
        let e = engine();
        let before = e
            .lock()
            .unwrap()
            .editor
            .document()
            .unwrap()
            .to_archive()
            .unwrap();
        let snapshot = e.snapshot_json().unwrap();
        let old = start(&e);
        assert_eq!(e.snapshot_json().unwrap(), snapshot);
        e.end_crop(old.clone(), "{}".into(), true).unwrap();
        assert!(e.end_crop(old, params(), false).is_err());
        assert_eq!(e.snapshot_json().unwrap(), snapshot);
        let old = start(&e);
        let new = start(&e);
        assert!(e.end_crop(old, "{}".into(), true).is_err());
        e.end_crop(new, params(), false).unwrap();
        let inner = e.lock().unwrap();
        assert_eq!(inner.editor.snapshot().history.len(), 1);
        assert_eq!(inner.editor.document().unwrap().frame().width, 8);
        assert_eq!(
            inner.editor.document().unwrap().to_archive().unwrap().tiles,
            before.tiles
        );
        drop(inner);
        e.dispatch("edit.undo".into(), "{}".into()).unwrap();
        assert_eq!(
            e.lock()
                .unwrap()
                .editor
                .document()
                .unwrap()
                .to_archive()
                .unwrap(),
            before
        );
    }
    #[test]
    fn edited_replaced_and_ai_epoch_changed_contexts_reject_a_late_crop() {
        for change in ["edit", "replace", "ai"] {
            let e = engine();
            let token = start(&e);
            match change {
                "edit" => {
                    e.dispatch(
                        "layer.rename".into(),
                        json!({"layer":{"name":"Background"},"name":"changed"}).to_string(),
                    )
                    .unwrap();
                }
                "replace" => {
                    e.dispatch(
                        "file.new".into(),
                        json!({"width":4,"height":4,"bit_depth":16,"discard_changes":true})
                            .to_string(),
                    )
                    .unwrap();
                }
                _ => {
                    let mut i = e.lock().unwrap();
                    i.editor.begin_turn().unwrap();
                    i.editor.stop_turn();
                }
            }
            let snapshot = e.snapshot_json().unwrap();
            assert!(e.end_crop(token, params(), false).is_err());
            assert_eq!(e.snapshot_json().unwrap(), snapshot);
        }
    }
    #[test]
    fn cancellation_never_waits_for_the_editor_or_renderer_mutex() {
        let e = engine();
        let token = start(&e);
        let held = e.lock().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let worker = e.clone();
        let join =
            std::thread::spawn(move || tx.send(worker.end_crop(token, "{}".into(), true)).unwrap());
        assert!(
            rx.recv_timeout(std::time::Duration::from_secs(1))
                .unwrap()
                .is_ok()
        );
        drop(held);
        join.join().unwrap();
    }
    #[test]
    fn cancelling_a_crop_does_not_revoke_a_new_ai_request() {
        let e = engine();
        let token = start(&e);
        let (turn, request) = {
            let mut inner = e.lock().unwrap();
            let turn = inner.editor.begin_turn().unwrap();
            let request = inner.editor.request(Source::Ai { turn_id: turn });
            (turn, request)
        };
        e.end_crop(token, "{}".into(), true).unwrap();
        let mut inner = e.lock().unwrap();
        inner
            .editor
            .execute(
                request,
                "layer.create",
                json!({"name":"AI remains active"}),
                |_| Ok(()),
            )
            .unwrap();
        assert_eq!(
            inner.editor.snapshot().history.last().unwrap().turn_id,
            Some(turn)
        );
    }
    #[test]
    fn detaching_the_native_canvas_revokes_its_crop_without_editing_the_document() {
        let e = engine();
        let token = start(&e);
        let snapshot = e.snapshot_json().unwrap();
        e.detach_canvas().unwrap();
        assert!(e.end_crop(token, params(), false).is_err());
        assert_eq!(e.snapshot_json().unwrap(), snapshot);
    }
}
