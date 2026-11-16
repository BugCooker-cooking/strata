//! Native geometry drafts share one operation-bound cancellable ownership slot.
//! Overlay-only staging never touches editor truth or renderer pixels/history.
use crate::*;
pub(crate) struct NativeGeometry {
    token: String,
    command: &'static str,
    interaction: strata_core::commands::GeometryInteraction,
    work: strata_core::commands::WorkCancellation,
    finishing: bool,
}
pub(crate) enum TransformSelection {
    Layer(strata_core::LayerRef, strata_core::commands::TransformScope),
    Layers(Vec<strata_core::LayerRef>),
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
        self.begin_geometry(document_id, revision, "image.crop", None, None)
    }
    pub fn begin_straighten(
        &self,
        document_id: String,
        revision: u64,
    ) -> Result<String, StrataError> {
        self.begin_geometry(document_id, revision, "image.straighten", None, None)
    }
    pub fn begin_canvas_transform(
        &self,
        document_id: String,
        revision: u64,
        layer_id: String,
        scope: String,
    ) -> Result<String, StrataError> {
        let layer = serde_json::from_value(json!({"id":layer_id})).map_err(render_error)?;
        let scope = serde_json::from_value(json!(scope)).map_err(render_error)?;
        self.begin_geometry(
            document_id,
            revision,
            "transform.apply",
            Some(TransformSelection::Layer(layer, scope)),
            None,
        )
    }
    pub fn begin_canvas_transform_layers(
        &self,
        document_id: String,
        revision: u64,
        layer_ids: Vec<String>,
    ) -> Result<String, StrataError> {
        if !(2..=64).contains(&layer_ids.len()) {
            return Err(render_error("select between 2 and 64 layers"));
        }
        let layers = layer_ids
            .into_iter()
            .map(|id| serde_json::from_value(json!({"id":id})).map_err(render_error))
            .collect::<Result<Vec<_>, _>>()?;
        self.begin_geometry(
            document_id,
            revision,
            "transform.apply_layers",
            Some(TransformSelection::Layers(layers)),
            None,
        )
    }
    /// Bounds scan runs on an immutable editor clone outside the renderer lock.
    /// The same token can be cancelled while its source rows are being scanned.
    pub fn canvas_transform_bounds(&self, token: String) -> Result<String, StrataError> {
        let interaction = {
            let slot = self.geometry_session.lock().map_err(render_error)?;
            let value = slot
                .as_ref()
                .filter(|c| {
                    c.token == token
                        && matches!(c.command, "transform.apply" | "transform.apply_layers")
                        && !c.finishing
                })
                .ok_or_else(cancelled)?;
            value.interaction.with_work_cancellation(&value.work)
        };
        let editor = self.lock()?.editor.clone();
        let bounds = editor
            .canvas_transform_bounds(&interaction)
            .map_err(command_error)?;
        if !self
            .lock()?
            .editor
            .canvas_transform_context_matches(&interaction)
        {
            return Err(cancelled());
        }
        if self
            .geometry_session
            .lock()
            .map_err(render_error)?
            .as_ref()
            .is_none_or(|c| c.token != token || c.finishing)
        {
            return Err(cancelled());
        }
        serde_json::to_string(&bounds).map_err(render_error)
    }
    pub fn end_canvas_transform(
        &self,
        token: String,
        params_json: String,
        cancel: bool,
    ) -> Result<(), StrataError> {
        let command = self
            .geometry_session
            .lock()
            .map_err(render_error)?
            .as_ref()
            .filter(|c| {
                c.token == token
                    && matches!(c.command, "transform.apply" | "transform.apply_layers")
            })
            .map(|c| c.command)
            .ok_or_else(cancelled)?;
        self.end_geometry(token, params_json, cancel, command)
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
    pub(crate) fn native_guide_command(&self, token: &str) -> Result<&'static str, StrataError> {
        self.geometry_session
            .lock()
            .map_err(render_error)?
            .as_ref()
            .filter(|c| {
                c.token == token && matches!(c.command, "view.guides.add" | "view.guides.update")
            })
            .map(|c| c.command)
            .ok_or_else(cancelled)
    }
    pub(crate) fn cancel_native_geometry(&self) -> Result<(), StrataError> {
        if let Some(owned) = self.geometry_session.lock().map_err(render_error)?.take() {
            owned.cancel();
        }
        Ok(())
    }
    pub(crate) fn begin_geometry(
        &self,
        document_id: String,
        revision: u64,
        command: &'static str,
        transform: Option<TransformSelection>,
        guide: Option<(u64, Option<strata_core::GuideRef>)>,
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
        if guide
            .as_ref()
            .is_some_and(|(version, _)| *version != inner.editor.snapshot().presentation.version)
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
            "transform.apply" | "transform.apply_layers" => {
                match transform.expect("native transform targets") {
                    TransformSelection::Layer(layer, scope) => {
                        inner.editor.begin_canvas_transform(layer, scope)
                    }
                    TransformSelection::Layers(layers) => {
                        inner.editor.begin_canvas_transform_layers(&layers)
                    }
                }
            }
            "view.guides.add" | "view.guides.update" => inner
                .editor
                .begin_guide(guide.expect("native guide ownership").1),
            "view.presentation.set" => inner.editor.begin_ruler_origin(),
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
    pub(crate) fn end_geometry(
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
                "transform.apply" | "transform.apply_layers" => {
                    editor.prepare_canvas_transform(&interaction, params.clone())
                }
                "view.guides.add" | "view.guides.update" => {
                    editor.prepare_guide(&interaction, params.clone())
                }
                "view.presentation.set" => editor.prepare_ruler_origin(
                    &interaction,
                    serde_json::from_value(params["ruler_origin"].clone()).map_err(render_error)?,
                ),
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
    fn transform_start(e: &StrataEngine, scope: &str) -> String {
        let state = e.lock().unwrap().editor.snapshot();
        e.begin_canvas_transform(
            state.document_id.unwrap().to_string(),
            state.revision,
            state.active_layer.unwrap().to_string(),
            scope.into(),
        )
        .unwrap()
    }
    fn transform_parameters(e: &StrataEngine, scope: &str) -> String {
        json!({"layer":{"id":e.lock().unwrap().editor.snapshot().active_layer},"scope":scope,"geometry":{"type":"corners","source":[[0,0],[16,0],[16,16],[0,16]],"destination":[[2,1],[18,1],[18,17],[2,17]]},"interpolation":"nearest"}).to_string()
    }
    #[test]
    fn canvas_transform_bounds_and_pointer_staging_preserve_truth_and_apply_once() {
        let e = engine();
        let before = e.snapshot_json().unwrap();
        let archive = e
            .lock()
            .unwrap()
            .editor
            .document()
            .unwrap()
            .to_archive()
            .unwrap();
        let token = transform_start(&e, "whole_layer");
        let bounds: Value =
            serde_json::from_str(&e.canvas_transform_bounds(token.clone()).unwrap()).unwrap();
        assert_eq!(
            bounds["bounds"],
            json!({"x":0,"y":0,"width":16,"height":16})
        );
        assert_eq!(e.snapshot_json().unwrap(), before);
        let params = transform_parameters(&e, "whole_layer");
        e.end_canvas_transform(token.clone(), params.clone(), false)
            .unwrap();
        assert_eq!(
            e.lock()
                .unwrap()
                .editor
                .snapshot()
                .history
                .last()
                .unwrap()
                .command,
            "transform.apply"
        );
        assert!(e.end_canvas_transform(token, params, false).is_err());
        e.dispatch("edit.undo".into(), "{}".into()).unwrap();
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
    }
    #[test]
    fn canvas_transform_tokens_cannot_be_used_for_other_operations_or_scopes() {
        let e = engine();
        let crop = start(&e);
        let transform = transform_start(&e, "whole_layer");
        let before = e.snapshot_json().unwrap();
        assert!(e.end_crop(transform.clone(), params(), false).is_err());
        assert!(
            e.end_straighten(transform.clone(), line_params(), true)
                .is_err()
        );
        assert!(
            e.end_canvas_transform(crop, transform_parameters(&e, "whole_layer"), false)
                .is_err()
        );
        assert!(
            e.end_canvas_transform(
                transform.clone(),
                transform_parameters(&e, "selection"),
                false
            )
            .is_err()
        );
        assert!(
            e.end_canvas_transform(transform, transform_parameters(&e, "whole_layer"), false)
                .is_err()
        );
        assert_eq!(e.snapshot_json().unwrap(), before);
        let transform = transform_start(&e, "whole_layer");
        let crop = start(&e);
        assert!(e.canvas_transform_bounds(transform.clone()).is_err());
        assert!(
            e.end_canvas_transform(transform, "{}".into(), true)
                .is_err()
        );
        e.end_crop(crop, params(), false).unwrap();
    }
    #[test]
    fn newer_filter_preview_or_active_layer_change_revokes_a_transform_draft() {
        let e = engine();
        let token = transform_start(&e, "whole_layer");
        let state = e.lock().unwrap().editor.snapshot();
        let sequence = e.issue_tonal_preview().unwrap();
        e.begin_filter_preview(
            state.active_layer.unwrap().to_string(),
            "filter.add_noise".into(),
            state.revision,
            sequence,
        )
        .unwrap();
        e.preview_filter(json!({"amount":0.05,"seed":42}).to_string(), sequence)
            .unwrap();
        assert!(e.end_canvas_transform(token, "{}".into(), true).is_err());
        e.end_filter_preview_checked(
            json!({"amount":0.05,"seed":42}).to_string(),
            false,
            sequence,
        )
        .unwrap();
        assert_eq!(
            e.lock()
                .unwrap()
                .editor
                .snapshot()
                .history
                .last()
                .unwrap()
                .command,
            "filter.add_noise"
        );
        e.dispatch("layer.create".into(), json!({"name":"other"}).to_string())
            .unwrap();
        let other = e.lock().unwrap().editor.snapshot().active_layer;
        e.dispatch(
            "layer.select".into(),
            json!({"layer":{"name":"Background"}}).to_string(),
        )
        .unwrap();
        let token = transform_start(&e, "whole_layer");
        let params = transform_parameters(&e, "whole_layer");
        e.dispatch(
            "layer.select".into(),
            json!({"layer":{"id":other}}).to_string(),
        )
        .unwrap();
        let before = e.snapshot_json().unwrap();
        assert!(e.canvas_transform_bounds(token.clone()).is_err());
        assert!(e.end_canvas_transform(token, params, false).is_err());
        assert_eq!(e.snapshot_json().unwrap(), before);
    }
    #[test]
    fn canvas_transform_cancel_is_nonblocking_and_does_not_revoke_new_ai_work() {
        let e = engine();
        let token = transform_start(&e, "whole_layer");
        let held = e.lock().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let worker = e.clone();
        let cancel = token.clone();
        let join = std::thread::spawn(move || {
            tx.send(worker.end_canvas_transform(cancel, "{}".into(), true))
                .unwrap()
        });
        assert!(
            rx.recv_timeout(std::time::Duration::from_secs(1))
                .unwrap()
                .is_ok()
        );
        drop(held);
        join.join().unwrap();
        assert!(e.canvas_transform_bounds(token).is_err());
        let token = transform_start(&e, "whole_layer");
        let (turn, request) = {
            let mut inner = e.lock().unwrap();
            let turn = inner.editor.begin_turn().unwrap();
            let request = inner.editor.request(Source::Ai { turn_id: turn });
            (turn, request)
        };
        e.end_canvas_transform(token, "{}".into(), true).unwrap();
        let mut inner = e.lock().unwrap();
        inner
            .editor
            .execute(
                request,
                "layer.create",
                json!({"name":"AI unaffected"}),
                |_| Ok(()),
            )
            .unwrap();
        assert_eq!(
            inner.editor.snapshot().history.last().unwrap().turn_id,
            Some(turn)
        );
    }
    #[test]
    fn canvas_transform_real_metal_commits_shared_scene_and_exact_undo_for_both_scopes() {
        for scope in ["whole_layer", "selection"] {
            let mut renderer =
                match strata_render::Renderer::new_offscreen(strata_render::Viewport {
                    width: 64,
                    height: 64,
                    scale: 1.0,
                }) {
                    Ok(value) => value,
                    Err(strata_render::RenderError::NoAdapter)
                        if std::env::var_os("STRATA_REQUIRE_GPU").is_none() =>
                    {
                        return;
                    }
                    Err(error) => panic!("GPU: {error}"),
                };
            let e = engine();
            if scope == "selection" {
                e.dispatch(
                    "select.marquee".into(),
                    json!({"x":3,"y":4,"width":7,"height":6}).to_string(),
                )
                .unwrap();
                e.dispatch("select.feather".into(), json!({"radius":1}).to_string())
                    .unwrap();
            }
            {
                let mut inner = e.lock().unwrap();
                let scene = renderer
                    .prepare_document(inner.editor.document().unwrap(), false)
                    .unwrap();
                renderer.commit_document(scene).unwrap();
                renderer
                    .set_view(strata_render::ViewTransform {
                        zoom: 3.,
                        center_x: 8.,
                        center_y: 8.,
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
            let before = frame(&e);
            let archive = e
                .lock()
                .unwrap()
                .editor
                .document()
                .unwrap()
                .to_archive()
                .unwrap();
            let cursor = e.lock().unwrap().editor.snapshot().history_cursor;
            let token = transform_start(&e, scope);
            e.canvas_transform_bounds(token.clone()).unwrap();
            assert!(frame(&e) == before);
            e.end_canvas_transform(token, transform_parameters(&e, scope), false)
                .unwrap();
            assert!(frame(&e) != before);
            assert_eq!(
                e.lock().unwrap().editor.snapshot().history_cursor,
                cursor + 1
            );
            {
                let inner = e.lock().unwrap();
                let doc = inner.editor.document().unwrap();
                let cpu = strata_core::composite::Compositor::new(1)
                    .tile(doc, strata_pixels::TileCoord::new(0, 0).unwrap())
                    .unwrap();
                let shown = inner
                    .renderer
                    .as_ref()
                    .unwrap()
                    .read_offscreen_rgba()
                    .unwrap();
                for (x, y) in [(1usize, 1usize), (6, 7), (12, 12)] {
                    let p = cpu[y * 256 + x];
                    let offset = ((y * 3 + 9) * 64 + x * 3 + 9) * 4;
                    for c in 0..3 {
                        let bg = f64::from(strata_render::CANVAS_BG_SRGB[c]) / 255.;
                        let bg = if bg <= 0.04045 {
                            bg / 12.92
                        } else {
                            ((bg + 0.055) / 1.055).powf(2.4)
                        };
                        let v = f64::from(p[c]) * f64::from(p[3]) + bg * (1. - f64::from(p[3]));
                        let v = if v <= 0.0031308 {
                            12.92 * v
                        } else {
                            1.055 * v.powf(1. / 2.4) - 0.055
                        };
                        assert!(
                            shown[offset + c].abs_diff((v.clamp(0., 1.) * 255.).round() as u8) <= 3,
                            "{scope} {x},{y},{c}"
                        );
                    }
                }
            }
            e.dispatch("edit.undo".into(), "{}".into()).unwrap();
            assert!(frame(&e) == before);
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
        }
    }
}
