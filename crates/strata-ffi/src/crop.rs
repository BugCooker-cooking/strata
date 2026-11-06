//! Crop/straighten drafts share one operation-bound cancellable ownership slot.
//! Overlay-only staging never touches editor truth or renderer pixels/history.
use crate::*;
pub(crate) struct NativeGeometry {
    token: String,
    command: &'static str,
    interaction: strata_core::commands::GeometryInteraction,
    work: strata_core::commands::WorkCancellation,
    finishing: bool,
}
impl NativeGeometry {
    pub(crate) fn cancel(&self) {
        self.work.cancel();
    }
}
fn cancelled() -> StrataError {
    StrataError::Cancelled {
        message: "geometry draft is stale, superseded or cancelled".into(),
    }
}
#[uniffi::export]
impl StrataEngine {
    pub fn begin_crop(&self, document_id: String, revision: u64) -> Result<String, StrataError> {
        self.begin_geometry(document_id, revision, "image.crop")
    }
    pub fn begin_straighten(
        &self,
        document_id: String,
        revision: u64,
    ) -> Result<String, StrataError> {
        self.begin_geometry(document_id, revision, "image.straighten")
    }
    pub fn end_crop(
        &self,
        token: String,
        params_json: String,
        cancel: bool,
    ) -> Result<(), StrataError> {
        self.end_geometry(token, params_json, cancel, "image.crop")
    }
    pub fn end_straighten(
        &self,
        token: String,
        params_json: String,
        cancel: bool,
    ) -> Result<(), StrataError> {
        self.end_geometry(token, params_json, cancel, "image.straighten")
    }
}
impl StrataEngine {
    fn begin_geometry(
        &self,
        document_id: String,
        revision: u64,
        command: &'static str,
    ) -> Result<String, StrataError> {
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
            return Err(render_error(
                "finish the active canvas gesture before geometry",
            ));
        }
        let interaction = match command {
            "image.crop" => inner.editor.begin_crop(),
            "image.straighten" => inner.editor.begin_straighten(),
            _ => unreachable!(),
        }
        .map_err(command_error)?;
        self.issue_tonal_preview()?;
        crate::tonal_preview::restore(&mut inner)?;
        let token = uuid::Uuid::new_v4().to_string();
        let mut slot = self.geometry_session.lock().map_err(render_error)?;
        if let Some(old) = slot.take() {
            old.work.cancel();
        }
        *slot = Some(NativeGeometry {
            token: token.clone(),
            command,
            interaction,
            work: Default::default(),
            finishing: false,
        });
        Ok(token)
    }
    /// Cancellation acquires no editor/render lock. A late cancellation token
    /// cannot revoke another geometry draft or a newer AI request.
    fn end_geometry(
        &self,
        token: String,
        params_json: String,
        cancel: bool,
        command: &'static str,
    ) -> Result<(), StrataError> {
        let interaction = {
            let mut slot = self.geometry_session.lock().map_err(render_error)?;
            let value = slot
                .as_mut()
                .filter(|c| c.token == token && c.command == command)
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
                return Err(render_error("geometry parameter limit"));
            }
            let params: Value = serde_json::from_str(&params_json).map_err(render_error)?;
            let editor = self.lock()?.editor.clone();
            let prepared = match command {
                "image.crop" => editor.prepare_crop(&interaction, params.clone()),
                "image.straighten" => editor.prepare_straighten(&interaction, params.clone()),
                _ => unreachable!(),
            }
            .map_err(command_error)?;
            let mut inner = self.lock()?;
            if self
                .geometry_session
                .lock()
                .map_err(render_error)?
                .as_ref()
                .is_none_or(|c| c.token != token)
            {
                return Err(cancelled());
            }
            apply(&mut inner, prepared, command, &params)?;
            Ok(())
        })();
        {
            let mut slot = self.geometry_session.lock().map_err(render_error)?;
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
    fn start_straighten(e: &StrataEngine) -> String {
        let state = e.lock().unwrap().editor.snapshot();
        e.begin_straighten(state.document_id.unwrap().to_string(), state.revision)
            .unwrap()
    }
    fn line_params() -> String {
        json!({"reference":{"type":"line","start":[2,4],"end":[13,5],"align":"horizontal"},"crop":true}).to_string()
    }
    #[test]
    fn straighten_and_crop_tokens_are_operation_bound_and_supersede_each_other() {
        let e = engine();
        let original = e
            .lock()
            .unwrap()
            .editor
            .document()
            .unwrap()
            .to_archive()
            .unwrap();
        let snapshot = e.snapshot_json().unwrap();
        let crop = start(&e);
        let line = start_straighten(&e);
        assert_eq!(e.snapshot_json().unwrap(), snapshot);
        assert!(e.end_crop(crop, params(), false).is_err());
        assert!(e.end_crop(line.clone(), "{}".into(), true).is_err());
        assert!(e.end_crop(line.clone(), params(), false).is_err());
        e.end_straighten(line, line_params(), false).unwrap();
        {
            let i = e.lock().unwrap();
            assert_eq!(i.editor.snapshot().history.len(), 1);
            assert_eq!(i.editor.snapshot().history[0].command, "image.straighten");
            assert!(i.editor.document().unwrap().frame().height < 16);
        }
        e.dispatch("edit.undo".into(), "{}".into()).unwrap();
        assert_eq!(
            e.lock()
                .unwrap()
                .editor
                .document()
                .unwrap()
                .to_archive()
                .unwrap(),
            original
        );
        let line = start_straighten(&e);
        let crop = start(&e);
        assert!(e.end_straighten(line, line_params(), false).is_err());
        assert!(e.end_straighten(crop.clone(), "{}".into(), true).is_err());
        e.end_crop(crop, params(), false).unwrap();
        assert_eq!(
            e.lock().unwrap().editor.snapshot().history[0].command,
            "image.crop"
        );
    }
    #[test]
    fn straighten_request_cannot_become_crop_and_stale_contexts_cannot_retarget() {
        let e = engine();
        {
            let i = e.lock().unwrap();
            let crop = i.editor.begin_crop().unwrap();
            let line = i.editor.begin_straighten().unwrap();
            assert!(
                i.editor
                    .prepare_straighten(&crop, serde_json::from_str(&line_params()).unwrap())
                    .is_err()
            );
            assert!(
                i.editor
                    .prepare_crop(&line, serde_json::from_str(&params()).unwrap())
                    .is_err()
            );
        }
        for change in ["edit", "replace", "ai", "detach"] {
            let e = engine();
            let token = start_straighten(&e);
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
                "detach" => {
                    e.detach_canvas().unwrap();
                }
                _ => {
                    let mut i = e.lock().unwrap();
                    i.editor.begin_turn().unwrap();
                    i.editor.stop_turn();
                }
            }
            let snapshot = e.snapshot_json().unwrap();
            assert!(e.end_straighten(token, line_params(), false).is_err());
            assert_eq!(e.snapshot_json().unwrap(), snapshot);
        }
    }
    #[test]
    fn queued_straighten_cancellation_is_nonblocking_and_does_not_cancel_new_ai_work() {
        let e = engine();
        let token = start_straighten(&e);
        let mut held = e.lock().unwrap();
        let worker = e.clone();
        let finish_token = token.clone();
        let join =
            std::thread::spawn(move || worker.end_straighten(finish_token, line_params(), false));
        let start = std::time::Instant::now();
        while !e
            .geometry_session
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .finishing
            && start.elapsed() < std::time::Duration::from_secs(2)
        {
            std::thread::yield_now();
        }
        assert!(
            e.geometry_session
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .finishing
        );
        let turn = held.editor.begin_turn().unwrap();
        let ai_request = held.editor.request(Source::Ai { turn_id: turn });
        let (tx, rx) = std::sync::mpsc::channel();
        let canceler = e.clone();
        let cancel_join = std::thread::spawn(move || {
            tx.send(canceler.end_straighten(token, "{}".into(), true))
                .unwrap()
        });
        assert!(
            rx.recv_timeout(std::time::Duration::from_secs(1))
                .unwrap()
                .is_ok()
        );
        held.editor
            .execute(
                ai_request,
                "layer.create",
                json!({"name":"AI remains active"}),
                |_| Ok(()),
            )
            .unwrap();
        drop(held);
        assert!(join.join().unwrap().is_err());
        cancel_join.join().unwrap();
        let state = e.lock().unwrap().editor.snapshot();
        assert_eq!(state.history.len(), 1);
        assert_eq!(state.history[0].command, "layer.create");
        assert_eq!(state.history[0].turn_id, Some(turn));
    }
    #[test]
    fn straighten_cancelled_invalid_or_duplicate_completion_does_not_publish_a_second_edit() {
        let e = engine();
        let snapshot = e.snapshot_json().unwrap();
        let token = start_straighten(&e);
        e.end_straighten(token.clone(), "{}".into(), true).unwrap();
        assert!(e.end_straighten(token, line_params(), false).is_err());
        assert_eq!(e.snapshot_json().unwrap(), snapshot);
        let token = start_straighten(&e);
        assert!(e.end_straighten(token.clone(), "{}".into(), false).is_err());
        assert!(e.end_straighten(token, line_params(), false).is_err());
        assert_eq!(e.snapshot_json().unwrap(), snapshot);
        let token = start_straighten(&e);
        e.end_straighten(token.clone(), line_params(), false)
            .unwrap();
        let after = e.snapshot_json().unwrap();
        assert!(e.end_straighten(token, line_params(), false).is_err());
        assert_eq!(e.snapshot_json().unwrap(), after);
    }
    #[test]
    fn straighten_owned_draft_metal_keeps_staging_readonly_and_commits_or_undoes_the_same_scene() {
        let e = engine();
        let mut renderer = match strata_render::Renderer::new_offscreen(strata_render::Viewport {
            width: 64,
            height: 64,
            scale: 1.,
        }) {
            Ok(value) => value,
            Err(strata_render::RenderError::NoAdapter)
                if std::env::var_os("STRATA_REQUIRE_GPU").is_none() =>
            {
                return;
            }
            Err(error) => panic!("required GPU: {error}"),
        };
        {
            let i = e.lock().unwrap();
            let scene = renderer
                .prepare_document(i.editor.document().unwrap(), false)
                .unwrap();
            renderer.commit_document(scene).unwrap();
        }
        renderer
            .set_view(strata_render::ViewTransform {
                zoom: 3.,
                center_x: 8.,
                center_y: 8.,
            })
            .unwrap();
        renderer.render().unwrap();
        let original_scene = renderer.read_offscreen_rgba().unwrap();
        e.lock().unwrap().renderer = Some(renderer);
        let original = e.snapshot_json().unwrap();
        let token = start_straighten(&e);
        {
            let mut i = e.lock().unwrap();
            let r = i.renderer.as_mut().unwrap();
            r.render().unwrap();
            assert_eq!(r.read_offscreen_rgba().unwrap(), original_scene);
        }
        assert_eq!(e.snapshot_json().unwrap(), original);
        e.end_straighten(token, line_params(), false).unwrap();
        {
            let mut i = e.lock().unwrap();
            let doc = i.editor.document().unwrap();
            let f = doc.frame();
            assert!(f.x > 0 && f.y > 0);
            let x = f.x + i64::from(f.width / 2);
            let y = f.y + i64::from(f.height / 2);
            let reference = strata_core::composite::Compositor::new(1)
                .tile(doc, strata_pixels::TileCoord::new(0, 0).unwrap())
                .unwrap()[y as usize * 256 + x as usize];
            let r = i.renderer.as_mut().unwrap();
            r.render().unwrap();
            let shown = r.read_offscreen_rgba().unwrap();
            let sx = (32. + (x as f32 + 0.5 - 8.) * 3.) as usize;
            let sy = (32. + (y as f32 + 0.5 - 8.) * 3.) as usize;
            for c in 0..3 {
                let bg = f64::from(strata_render::CANVAS_BG_SRGB[c]) / 255.;
                let bg = if bg <= 0.04045 {
                    bg / 12.92
                } else {
                    ((bg + 0.055) / 1.055).powf(2.4)
                };
                let v = f64::from(reference[c]) * f64::from(reference[3])
                    + bg * (1. - f64::from(reference[3]));
                let v = if v <= 0.0031308 {
                    12.92 * v
                } else {
                    1.055 * v.powf(1. / 2.4) - 0.055
                };
                assert!(
                    shown[(sy * 64 + sx) * 4 + c].abs_diff((v.clamp(0., 1.) * 255.).round() as u8)
                        <= 3
                );
            }
        }
        e.dispatch("edit.undo".into(), "{}".into()).unwrap();
        let mut i = e.lock().unwrap();
        assert_eq!(i.editor.document().unwrap().frame().width, 16);
        let r = i.renderer.as_mut().unwrap();
        r.render().unwrap();
        assert_eq!(r.read_offscreen_rgba().unwrap(), original_scene);
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
