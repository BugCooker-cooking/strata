use crate::*;
#[uniffi::export]
impl StrataEngine {
    /// Shared owned draft; the sole committed action is view.presentation.set.
    pub fn begin_ruler_origin(
        &self,
        document_id: String,
        revision: u64,
        presentation_version: u64,
    ) -> Result<String, StrataError> {
        self.begin_geometry(
            document_id,
            revision,
            "view.presentation.set",
            None,
            Some((presentation_version, None)),
        )
    }
    pub fn end_ruler_origin(
        &self,
        token: String,
        x: f64,
        y: f64,
        cancel: bool,
    ) -> Result<(), StrataError> {
        if !cancel && (!x.is_finite() || !y.is_finite()) {
            self.end_geometry(token, "{}".into(), true, "view.presentation.set")?;
            return Err(render_error("ruler origin must be finite"));
        }
        self.end_geometry(
            token,
            json!({"ruler_origin":[x,y]}).to_string(),
            cancel,
            "view.presentation.set",
        )
    }
    /// Capture the original request/control epoch in the shared native slot.
    pub fn begin_guide(
        &self,
        document_id: String,
        revision: u64,
        presentation_version: u64,
        guide_id: Option<String>,
    ) -> Result<String, StrataError> {
        let target = guide_id
            .map(|id| serde_json::from_value(json!({"id":id})))
            .transpose()
            .map_err(render_error)?;
        let command = if target.is_some() {
            "view.guides.update"
        } else {
            "view.guides.add"
        };
        self.begin_geometry(
            document_id,
            revision,
            command,
            None,
            Some((presentation_version, target)),
        )
    }
    pub fn end_guide(
        &self,
        token: String,
        params_json: String,
        cancel: bool,
    ) -> Result<(), StrataError> {
        let command = self.native_guide_command(&token)?;
        self.end_geometry(token, params_json, cancel, command)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Listener {
        events: Mutex<Vec<String>>,
    }
    impl EngineListener for Listener {
        fn on_event(&self, event: EngineEventDto) {
            if let EngineEventDto::StateChanged { snapshot_json } = event {
                self.events.lock().unwrap().push(snapshot_json);
            }
        }
    }
    fn setup() -> (Arc<StrataEngine>, Arc<Listener>) {
        let listener = Arc::new(Listener {
            events: Mutex::new(Vec::new()),
        });
        let engine = StrataEngine::new(listener.clone());
        engine
            .dispatch(
                "file.new".into(),
                json!({"width":32,"height":24,"bit_depth":16}).to_string(),
            )
            .unwrap();
        (engine, listener)
    }
    #[test]
    fn ruler_origin_uses_owned_view_command_and_late_or_cancelled_tokens_preserve_new_truth() {
        for change in [
            "apply",
            "cancel",
            "view",
            "document",
            "invalid",
            "superseded",
        ] {
            let (engine, _) = setup();
            let state = engine.lock().unwrap().editor.snapshot();
            let token = engine
                .begin_ruler_origin(
                    state.document_id.unwrap().to_string(),
                    state.revision,
                    state.presentation.version,
                )
                .unwrap();
            let archive = engine
                .lock()
                .unwrap()
                .editor
                .document()
                .unwrap()
                .to_archive()
                .unwrap();
            match change {
                "view" => {
                    engine
                        .dispatch(
                            "view.presentation.set".into(),
                            json!({"ruler_origin":[7,9]}).to_string(),
                        )
                        .unwrap();
                }
                "document" => {
                    engine
                        .dispatch(
                            "file.new".into(),
                            json!({"width":4,"height":4,"bit_depth":8,"discard_changes":true})
                                .to_string(),
                        )
                        .unwrap();
                }
                "superseded" => {
                    let newer = engine
                        .begin_ruler_origin(
                            state.document_id.unwrap().to_string(),
                            state.revision,
                            state.presentation.version,
                        )
                        .unwrap();
                    assert!(engine.end_ruler_origin(token, 0., 0., true).is_err());
                    engine.end_ruler_origin(newer, 4.25, 6.5, false).unwrap();
                    assert_eq!(
                        engine
                            .lock()
                            .unwrap()
                            .editor
                            .snapshot()
                            .presentation
                            .ruler_origin,
                        [4.25, 6.5]
                    );
                    continue;
                }
                _ => {}
            }
            let before = engine.lock().unwrap().editor.snapshot();
            let result = engine.end_ruler_origin(
                token,
                if change == "invalid" { f64::NAN } else { 20.5 },
                10.25,
                change == "cancel",
            );
            if matches!(change, "view" | "document" | "invalid") {
                assert!(result.is_err());
            } else {
                result.unwrap();
            }
            let after = engine.lock().unwrap().editor.snapshot();
            assert_eq!(after.revision, before.revision);
            assert_eq!(after.history_cursor, before.history_cursor);
            assert_eq!(after.dirty, before.dirty);
            if change == "apply" {
                assert_eq!(after.presentation.ruler_origin, [20.5, 10.25]);
                assert_eq!(after.presentation.version, before.presentation.version + 1);
            } else {
                assert_eq!(after.presentation, before.presentation);
            }
            if change != "document" {
                assert!(
                    engine
                        .lock()
                        .unwrap()
                        .editor
                        .document()
                        .unwrap()
                        .to_archive()
                        .unwrap()
                        == archive
                );
            }
            assert!(engine.geometry_session.lock().unwrap().is_none());
        }
    }
    #[test]
    fn smart_overlay_returns_only_the_exact_displayed_candidate_and_clears_on_cancel_or_context_change()
     {
        let (engine, _) = setup();
        let moving: Value = serde_json::from_str(
            &engine
                .dispatch(
                    "layer.create".into(),
                    json!({"name":"Moving","fill":[10001,30003,50001,40001]}).to_string(),
                )
                .unwrap(),
        )
        .unwrap();
        let reference: Value = serde_json::from_str(
            &engine
                .dispatch(
                    "layer.create".into(),
                    json!({"name":"Reference","fill":[50001,20003,10005,65535]}).to_string(),
                )
                .unwrap(),
        )
        .unwrap();
        engine
            .dispatch(
                "layer.set_position".into(),
                json!({"layer":{"id":reference["layer_id"]},"x":50,"y":30}).to_string(),
            )
            .unwrap();
        engine
            .dispatch(
                "layer.select".into(),
                json!({"layer":{"id":moving["layer_id"]}}).to_string(),
            )
            .unwrap();
        let state = engine.lock().unwrap().editor.snapshot();
        let before = engine
            .lock()
            .unwrap()
            .editor
            .document()
            .unwrap()
            .to_archive()
            .unwrap();
        let params=json!({"x":19,"y":7,"guides":false,"artboard":false,"layers":true,"anchors":"edges","tolerance":2}).to_string();
        for change in ["cancel", "settings", "visibility"] {
            let seq = engine.issue_tonal_preview().unwrap();
            engine
                .begin_filter_preview(
                    moving["layer_id"].as_str().unwrap().into(),
                    "layer.snap_position".into(),
                    state.revision,
                    seq,
                )
                .unwrap();
            engine.preview_filter(params.clone(), seq).unwrap();
            let shown: Value =
                serde_json::from_str(&engine.snap_preview_result().unwrap()).unwrap();
            assert_eq!(shown["x"], 18);
            assert_eq!(shown["y"], 6);
            assert_eq!(shown["snapped_x"]["reference_layer"], reference["layer_id"]);
            assert!(
                engine
                    .lock()
                    .unwrap()
                    .editor
                    .document()
                    .unwrap()
                    .to_archive()
                    .unwrap()
                    == before
            );
            if change == "cancel" {
                engine
                    .end_filter_preview_checked(params.clone(), true, seq)
                    .unwrap();
            } else if change == "settings" {
                engine
                    .dispatch(
                        "view.presentation.set".into(),
                        json!({"snap_layers":true}).to_string(),
                    )
                    .unwrap();
            } else {
                engine
                    .dispatch(
                        "layer.set_visibility".into(),
                        json!({"layer":{"id":reference["layer_id"]},"visible":false}).to_string(),
                    )
                    .unwrap();
            }
            assert_eq!(engine.snap_preview_result().unwrap(), "null");
        }
    }
    #[test]
    fn owned_guide_uses_shared_commands_and_late_epoch_version_or_cancel_cannot_retarget() {
        let (engine, listener) = setup();
        let state = engine.lock().unwrap().editor.snapshot();
        let original = engine
            .lock()
            .unwrap()
            .editor
            .document()
            .unwrap()
            .to_archive()
            .unwrap();
        let token = engine
            .begin_guide(
                state.document_id.unwrap().to_string(),
                state.revision,
                state.presentation.version,
                None,
            )
            .unwrap();
        engine
            .end_guide(
                token,
                json!({"name":"Host","axis":"vertical","position":10.5}).to_string(),
                false,
            )
            .unwrap();
        let changed = engine
            .lock()
            .unwrap()
            .editor
            .document()
            .unwrap()
            .to_archive()
            .unwrap();
        assert_eq!(changed.tiles, original.tiles);
        assert_eq!(changed.metadata["guides"][0]["position"], 10.5);
        assert!(
            listener
                .events
                .lock()
                .unwrap()
                .last()
                .unwrap()
                .contains("10.5")
        );
        engine.dispatch("edit.undo".into(), "{}".into()).unwrap();
        assert!(
            engine
                .lock()
                .unwrap()
                .editor
                .document()
                .unwrap()
                .to_archive()
                .unwrap()
                == original
        );
        for change in ["version", "revision", "document", "control_epoch", "cancel"] {
            let (engine, _) = setup();
            let state = engine.lock().unwrap().editor.snapshot();
            let token = engine
                .begin_guide(
                    state.document_id.unwrap().to_string(),
                    state.revision,
                    state.presentation.version,
                    None,
                )
                .unwrap();
            match change {
                "version" => {
                    engine
                        .dispatch(
                            "view.presentation.set".into(),
                            json!({"guides_locked":true}).to_string(),
                        )
                        .unwrap();
                }
                "revision" => {
                    engine
                        .dispatch("layer.create".into(), json!({"name":"next"}).to_string())
                        .unwrap();
                }
                "document" => {
                    engine
                        .dispatch(
                            "file.new".into(),
                            json!({"width":8,"height":8,"bit_depth":16,"discard_changes":true})
                                .to_string(),
                        )
                        .unwrap();
                }
                "control_epoch" => {
                    let mut inner = engine.lock().unwrap();
                    inner.editor.begin_turn().unwrap();
                    inner.editor.stop_turn();
                }
                _ => {
                    engine.end_guide(token.clone(), "{}".into(), true).unwrap();
                }
            }
            let before = engine.snapshot_json().unwrap();
            assert!(
                engine
                    .end_guide(
                        token,
                        json!({"name":"late","axis":"horizontal","position":5}).to_string(),
                        false
                    )
                    .is_err()
            );
            assert_eq!(engine.snapshot_json().unwrap(), before);
        }
    }
    #[test]
    fn snapping_uses_shared_display_preview_without_document_history_until_apply() {
        let (engine, _) = setup();
        engine
            .dispatch(
                "paint.fill".into(),
                json!({"layer":{"name":"Background"},"color":[0.2,0.4,0.8,1]}).to_string(),
            )
            .unwrap();
        engine
            .dispatch(
                "view.guides.add".into(),
                json!({"name":"X","axis":"vertical","position":10.5}).to_string(),
            )
            .unwrap();
        let state = engine.lock().unwrap().editor.snapshot();
        let before = engine
            .lock()
            .unwrap()
            .editor
            .document()
            .unwrap()
            .to_archive()
            .unwrap();
        let sequence = engine.issue_tonal_preview().unwrap();
        engine
            .begin_filter_preview(
                state.active_layer.unwrap().to_string(),
                "layer.snap_position".into(),
                state.revision,
                sequence,
            )
            .unwrap();
        let params =
            json!({"x":9,"y":0,"tolerance":2,"artboard":false,"anchors":"edges"}).to_string();
        engine.preview_filter(params.clone(), sequence).unwrap();
        assert!(
            engine
                .lock()
                .unwrap()
                .editor
                .document()
                .unwrap()
                .to_archive()
                .unwrap()
                == before
        );
        engine
            .end_filter_preview_checked(params, false, sequence)
            .unwrap();
        assert_eq!(
            engine.lock().unwrap().editor.document().unwrap().layers()[0]
                .info()
                .x,
            11
        );
        engine.dispatch("edit.undo".into(), "{}".into()).unwrap();
        assert!(
            engine
                .lock()
                .unwrap()
                .editor
                .document()
                .unwrap()
                .to_archive()
                .unwrap()
                == before
        );
        let state = engine.lock().unwrap().editor.snapshot();
        let sequence = engine.issue_tonal_preview().unwrap();
        engine
            .begin_filter_preview(
                state.active_layer.unwrap().to_string(),
                "layer.snap_position".into(),
                state.revision,
                sequence,
            )
            .unwrap();
        engine
            .preview_filter(json!({"x":9,"y":0,"tolerance":2}).to_string(), sequence)
            .unwrap();
        engine
            .dispatch(
                "view.presentation.set".into(),
                json!({"snap_enabled":true}).to_string(),
            )
            .unwrap();
        assert!(engine.lock().unwrap().tonal_preview.is_none());
        assert!(
            engine
                .lock()
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

#[cfg(test)]
mod metal_tests {
    use super::*;
    struct Listener;
    impl EngineListener for Listener {
        fn on_event(&self, _: EngineEventDto) {}
    }
    #[test]
    fn required_metal_snap_preview_cancel_apply_and_guide_only_undo_keep_the_exact_scene() {
        let mut renderer = match Renderer::new_offscreen(Viewport {
            width: 32,
            height: 24,
            scale: 1.,
        }) {
            Ok(r) => r,
            Err(strata_render::RenderError::NoAdapter)
                if std::env::var_os("STRATA_REQUIRE_GPU").is_none() =>
            {
                return;
            }
            Err(e) => panic!("Metal: {e}"),
        };
        let engine = StrataEngine::new(Arc::new(Listener));
        engine.dispatch("file.new".into(),json!({"width":32,"height":24,"bit_depth":16,"working_space":"display_p3","background":[10001,30003,50001,32769]}).to_string()).unwrap();
        engine
            .dispatch(
                "view.guides.add".into(),
                json!({"name":"Reference","axis":"vertical","position":10.5}).to_string(),
            )
            .unwrap();
        {
            let mut inner = engine.lock().unwrap();
            let scene = renderer
                .prepare_document(inner.editor.document().unwrap(), false)
                .unwrap();
            renderer.commit_document(scene).unwrap();
            renderer
                .set_view(strata_render::ViewTransform {
                    zoom: 1.,
                    center_x: 16.,
                    center_y: 12.,
                })
                .unwrap();
            inner.renderer = Some(renderer);
        }
        let frame = |engine: &StrataEngine| {
            engine.render_frame().unwrap();
            engine
                .lock()
                .unwrap()
                .renderer
                .as_ref()
                .unwrap()
                .read_offscreen_rgba()
                .unwrap()
        };
        let before = frame(&engine);
        let archive = engine
            .lock()
            .unwrap()
            .editor
            .document()
            .unwrap()
            .to_archive()
            .unwrap();
        for unit in ["inches", "millimeters", "percent"] {
            engine
                .dispatch(
                    "view.presentation.set".into(),
                    json!({"ruler_unit":unit,"ruler_origin":[-12.5,30]}).to_string(),
                )
                .unwrap();
            let converted: Value = serde_json::from_str(
                &engine
                    .dispatch(
                        "view.rulers.convert".into(),
                        json!({"coordinates":[59.5,102]}).to_string(),
                    )
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(converted["metrics"]["unit"], unit);
            assert_eq!(frame(&engine), before);
            assert!(
                engine
                    .lock()
                    .unwrap()
                    .editor
                    .document()
                    .unwrap()
                    .to_archive()
                    .unwrap()
                    == archive
            );
        }
        for cancel in [true, false] {
            let state = engine.lock().unwrap().editor.snapshot();
            let token = engine
                .begin_ruler_origin(
                    state.document_id.unwrap().to_string(),
                    state.revision,
                    state.presentation.version,
                )
                .unwrap();
            assert_eq!(frame(&engine), before);
            engine.end_ruler_origin(token, 20.5, 10.25, cancel).unwrap();
            assert_eq!(frame(&engine), before);
            assert!(
                engine
                    .lock()
                    .unwrap()
                    .editor
                    .document()
                    .unwrap()
                    .to_archive()
                    .unwrap()
                    == archive
            );
        }
        let state = engine.lock().unwrap().editor.snapshot();
        let params =
            json!({"x":9,"y":0,"tolerance":2,"artboard":false,"anchors":"edges"}).to_string();
        let seq = engine.issue_tonal_preview().unwrap();
        engine
            .begin_filter_preview(
                state.active_layer.unwrap().to_string(),
                "layer.snap_position".into(),
                state.revision,
                seq,
            )
            .unwrap();
        engine.preview_filter(params.clone(), seq).unwrap();
        let shown = frame(&engine);
        assert!(shown != before);
        assert!(
            engine
                .lock()
                .unwrap()
                .editor
                .document()
                .unwrap()
                .to_archive()
                .unwrap()
                == archive
        );
        engine
            .end_filter_preview_checked(params.clone(), true, seq)
            .unwrap();
        assert_eq!(frame(&engine), before);
        let seq = engine.issue_tonal_preview().unwrap();
        engine
            .begin_filter_preview(
                state.active_layer.unwrap().to_string(),
                "layer.snap_position".into(),
                state.revision,
                seq,
            )
            .unwrap();
        engine.preview_filter(params.clone(), seq).unwrap();
        engine
            .end_filter_preview_checked(params, false, seq)
            .unwrap();
        assert_eq!(frame(&engine), shown);
        {
            let inner = engine.lock().unwrap();
            let reference = strata_core::composite::Compositor::new(1)
                .tile(
                    inner.editor.document().unwrap(),
                    strata_pixels::TileCoord::new(0, 0).unwrap(),
                )
                .unwrap();
            for y in 2..22usize {
                for x in 1..31usize {
                    let p = reference[y * 256 + x];
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
                            shown[(y * 32 + x) * 4 + c]
                                .abs_diff((v.clamp(0., 1.) * 255.).round() as u8)
                                <= 3,
                            "snap CPU reference {x},{y},{c}"
                        );
                    }
                }
            }
        }
        engine
            .dispatch(
                "view.guides.add".into(),
                json!({"name":"Overlay only","axis":"horizontal","position":5.25}).to_string(),
            )
            .unwrap();
        assert_eq!(frame(&engine), shown);
        engine.dispatch("edit.undo".into(), "{}".into()).unwrap();
        assert_eq!(frame(&engine), shown);
        engine.dispatch("edit.undo".into(), "{}".into()).unwrap();
        assert_eq!(frame(&engine), before);
        assert!(
            engine
                .lock()
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
