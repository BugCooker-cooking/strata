use crate::*;
#[uniffi::export]
impl StrataEngine {
    /// Draft ownership is trusted host context, not a field the AI can choose.
    /// A stale editor panel must not silently rebind its old text to newer truth.
    pub fn dispatch_text(
        &self,
        command: String,
        params_json: String,
        document_id: String,
        revision: u64,
        active_layer: Option<String>,
    ) -> Result<String, StrataError> {
        if !matches!(command.as_str(), "type.create" | "type.update")
            || params_json.len() > 1024 * 1024
        {
            return Err(render_error(
                "invalid native text operation/parameter length",
            ));
        }
        let params: Value = serde_json::from_str(&params_json).map_err(render_error)?;
        let (editor, request) = {
            let mut inner = self.lock()?;
            let state = inner.editor.snapshot();
            if state
                .document_id
                .is_none_or(|id| id.to_string() != document_id)
                || state.revision != revision
                || state.active_layer.map(|id| id.to_string()) != active_layer
                || inner.editor.control() != &strata_core::commands::Control::User
            {
                return Err(StrataError::Cancelled {
                    message: "text draft context changed; reload the current layer".into(),
                });
            }
            self.issue_tonal_preview()?;
            self.cancel_native_geometry()?;
            crate::tonal_preview::restore(&mut inner)?;
            (inner.editor.clone(), inner.editor.request(Source::User))
        };
        let prepared = editor
            .prepare(request, &command, params.clone())
            .map_err(command_error)?;
        let result = {
            let mut inner = self.lock()?;
            if inner
                .editor
                .snapshot()
                .active_layer
                .map(|id| id.to_string())
                != active_layer
            {
                return Err(StrataError::Cancelled {
                    message: "text draft layer changed".into(),
                });
            };
            apply(&mut inner, prepared, &command, &params)?
        };
        self.deliver_events()?;
        Ok(result.to_string())
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
            json!({"width":120,"height":90,"bit_depth":16}).to_string(),
        )
        .unwrap();
        e
    }
    fn params() -> String {
        json!({"name":"Type","spec":{"text":"Native 中文","font":"sans","font_size":24,"layout":{"type":"point"}},"color":[0.8,0.2,0.1,1],"x":5,"y":30}).to_string()
    }
    #[test]
    fn native_text_draft_context_cannot_rebind_to_changed_revision_layer_or_document() {
        for change in ["revision", "layer", "document", "control"] {
            let e = engine();
            let state = e.lock().unwrap().editor.snapshot();
            match change {
                "revision" => {
                    e.dispatch(
                        "layer.rename".into(),
                        json!({"layer":{"name":"Background"},"name":"changed"}).to_string(),
                    )
                    .unwrap();
                }
                "layer" => {
                    e.dispatch("layer.create".into(), json!({"name":"other"}).to_string())
                        .unwrap();
                }
                "document" => {
                    e.dispatch(
                        "file.new".into(),
                        json!({"width":8,"height":8,"bit_depth":16,"discard_changes":true})
                            .to_string(),
                    )
                    .unwrap();
                }
                _ => {
                    let mut inner = e.lock().unwrap();
                    inner.editor.begin_turn().unwrap();
                }
            }
            let before = e.snapshot_json().unwrap();
            assert!(
                e.dispatch_text(
                    "type.create".into(),
                    params(),
                    state.document_id.unwrap().to_string(),
                    state.revision,
                    state.active_layer.map(|id| id.to_string())
                )
                .is_err()
            );
            assert_eq!(e.snapshot_json().unwrap(), before);
        }
    }
    #[test]
    fn native_text_creation_uses_same_registered_operation_and_exact_undo() {
        let e = engine();
        let state = e.lock().unwrap().editor.snapshot();
        let original = e
            .lock()
            .unwrap()
            .editor
            .document()
            .unwrap()
            .to_archive()
            .unwrap();
        let result: Value = serde_json::from_str(
            &e.dispatch_text(
                "type.create".into(),
                params(),
                state.document_id.unwrap().to_string(),
                state.revision,
                state.active_layer.map(|id| id.to_string()),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(result["changed"], true);
        assert_eq!(result["definition"]["spec"]["text"], "Native 中文");
        assert_eq!(
            e.lock()
                .unwrap()
                .editor
                .snapshot()
                .history
                .last()
                .unwrap()
                .command,
            "type.create"
        );
        e.dispatch("edit.undo".into(), "{}".into()).unwrap();
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
    }
    #[test]
    fn real_metal_text_with_style_matches_cpu_and_update_undo_restores_scene() {
        let mut renderer = match Renderer::new_offscreen(Viewport {
            width: 96,
            height: 96,
            scale: 1.,
        }) {
            Ok(r) => r,
            Err(strata_render::RenderError::NoAdapter)
                if std::env::var_os("STRATA_REQUIRE_GPU").is_none() =>
            {
                return;
            }
            Err(error) => panic!("GPU: {error}"),
        };
        let e = engine();
        let mut transparent: Value = serde_json::from_str(&params()).unwrap();
        transparent["color"] = json!([0, 0, 0, 0]);
        e.dispatch("type.create".into(), transparent.to_string())
            .unwrap();
        e.dispatch("transform.apply".into(),json!({"layer":{"name":"Type"},"scope":"whole_layer","geometry":{"type":"components","scale":[1.1,0.9],"rotation_degrees":13,"shear_degrees":[6,0],"pivot":[35,28]}}).to_string()).unwrap();
        e.dispatch("layer.set_styles".into(),json!({"layer":{"name":"Type"},"styles":{"stroke":{"color":[10001,50001,30003,65535],"size":1,"position":"outside"},"drop_shadow":{"color":[0,0,0,32769],"offset_x":2,"offset_y":2,"blur":1}}}).to_string()).unwrap();
        {
            let mut inner = e.lock().unwrap();
            let scene = renderer
                .prepare_document(inner.editor.document().unwrap(), false)
                .unwrap();
            renderer.commit_document(scene).unwrap();
            renderer
                .set_view(strata_render::ViewTransform {
                    zoom: 1.,
                    center_x: 48.,
                    center_y: 30.,
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
        let state = e.lock().unwrap().editor.snapshot();
        e.dispatch_text("type.update".into(),json!({"layer":{"name":"Type"},"spec":{"text":"改字 Serif","font":"serif","font_size":26,"layout":{"type":"point"}},"color":[0.2,0.6,0.9,0.7]}).to_string(),state.document_id.unwrap().to_string(),state.revision,state.active_layer.map(|id|id.to_string())).unwrap();
        let shown = frame(&e);
        assert!(shown != before);
        {
            let inner = e.lock().unwrap();
            let doc = inner.editor.document().unwrap();
            let reference = strata_core::composite::Compositor::new(1)
                .tile(doc, strata_pixels::TileCoord::new(0, 0).unwrap())
                .unwrap();
            for y in 8..48usize {
                for x in 4..90usize {
                    let p = reference[y * 256 + x];
                    let screen_y = y + 18;
                    let i = (screen_y * 96 + x) * 4;
                    for c in 0..3 {
                        let bg = f64::from(strata_render::CANVAS_BG_SRGB[c]) / 255.;
                        let bg = if bg <= 0.04045 {
                            bg / 12.92
                        } else {
                            ((bg + 0.055) / 1.055).powf(2.4)
                        };
                        let v = f64::from(p[c]) * f64::from(p[3]) + bg * (1. - f64::from(p[3]));
                        let v = if v <= 0.0031308 {
                            v * 12.92
                        } else {
                            1.055 * v.powf(1. / 2.4) - 0.055
                        };
                        assert!(
                            shown[i + c].abs_diff((v.clamp(0., 1.) * 255.).round() as u8) <= 3,
                            "text {x},{y},{c}"
                        );
                    }
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
