use crate::*;
struct Listener;
impl EngineListener for Listener {
    fn on_event(&self, _: EngineEventDto) {}
}
#[test]
fn required_metal_owned_group_transform_preserves_editable_children_and_exact_undo() {
    let mut renderer = match Renderer::new_offscreen(Viewport {
        width: 64,
        height: 48,
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
    engine
        .dispatch(
            "file.new".into(),
            json!({"width":64,"height":48,"bit_depth":16,"working_space":"display_p3"}).to_string(),
        )
        .unwrap();
    let g: Value = serde_json::from_str(
        &engine
            .dispatch("layer.create_group".into(), json!({"name":"G"}).to_string())
            .unwrap(),
    )
    .unwrap();
    let group = g["layer_id"].as_str().unwrap();
    engine
        .dispatch(
            "layer.create".into(),
            json!({"name":"Pixels","fill":[10001,30003,50001,40001]}).to_string(),
        )
        .unwrap();
    engine
        .dispatch(
            "layer.mask.add".into(),
            json!({"layer":{"name":"G"},"source":"white"}).to_string(),
        )
        .unwrap();
    engine.dispatch("layer.fill.gradient".into(),json!({"name":"Editable gradient","start":[20000,30000,41500,10000],"end":[10000,45000,34500,52000],"angle":15}).to_string()).unwrap();
    engine.dispatch("type.create".into(),json!({"name":"Type","spec":{"text":"Group","font":"sans","font_size":16,"layout":{"type":"point"}},"color":[0.8,0.2,0.1,1],"x":4,"y":25}).to_string()).unwrap();
    engine
        .dispatch(
            "layer.select".into(),
            json!({"layer":{"id":group}}).to_string(),
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
                center_x: 32.,
                center_y: 24.,
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
    let before = frame(&engine);
    let original = engine
        .lock()
        .unwrap()
        .editor
        .document()
        .unwrap()
        .to_archive()
        .unwrap();
    let state = engine.lock().unwrap().editor.snapshot();
    let token = engine
        .begin_canvas_transform(
            state.document_id.unwrap().to_string(),
            state.revision,
            group.into(),
            "whole_layer".into(),
        )
        .unwrap();
    let bounds: Value =
        serde_json::from_str(&engine.canvas_transform_bounds(token.clone()).unwrap()).unwrap();
    assert!(bounds["bounds"]["width"].as_u64().unwrap() >= 64);
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
            == original
    );
    let params = json!({"layer":{"id":group},"scope":"whole_layer","geometry":{"type":"components","scale":[0.7,0.8],"rotation_degrees":13,"pivot":[32,24]},"interpolation":"bilinear"});
    engine
        .end_canvas_transform(token, params.to_string(), false)
        .unwrap();
    let shown = frame(&engine);
    assert!(shown != before);
    {
        let inner = engine.lock().unwrap();
        let doc = inner.editor.document().unwrap();
        let gradient = doc
            .layers()
            .iter()
            .find(|layer| layer.info().name == "Editable gradient")
            .unwrap();
        assert!(gradient.info().generated_geometry.is_some());
        assert_eq!(gradient.pixels().tile_count(), 0);
        assert!(matches!(
            doc.layers().last().unwrap().info().kind,
            strata_core::LayerKind::Text { .. }
        ));
        let reference = strata_core::composite::Compositor::new(1)
            .tile(doc, strata_pixels::TileCoord::new(0, 0).unwrap())
            .unwrap();
        for y in 3..45usize {
            for x in 3..61usize {
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
                        shown[(y * 64 + x) * 4 + c]
                            .abs_diff((v.clamp(0., 1.) * 255.).round() as u8)
                            <= 3,
                        "group scene {x},{y},{c}"
                    );
                }
            }
        }
    }
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
            == original
    );
}
