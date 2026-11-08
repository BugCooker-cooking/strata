use crate::*;
struct Listener;
impl EngineListener for Listener {
    fn on_event(&self, _: EngineEventDto) {}
}
#[test]
fn required_metal_smart_overlay_matches_same_candidate_and_cancel_apply_undo_restore_exact_scene() {
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
    let e = StrataEngine::new(Arc::new(Listener));
    e.dispatch(
        "file.new".into(),
        json!({"width":64,"height":48,"bit_depth":16,"working_space":"display_p3"}).to_string(),
    )
    .unwrap();
    for (name, w, h, color, x, y) in [
        ("Moving", 12, 10, [0.3, 0.5, 0.8, 1.], 0, 0),
        ("Reference", 20, 15, [0.8, 0.4, 0.1, 1.], 32, 24),
    ] {
        e.dispatch("layer.create".into(), json!({"name":name}).to_string())
            .unwrap();
        e.dispatch(
            "select.marquee".into(),
            json!({"x":0,"y":0,"width":w,"height":h}).to_string(),
        )
        .unwrap();
        e.dispatch(
            "paint.fill".into(),
            json!({"layer":{"name":name},"color":color}).to_string(),
        )
        .unwrap();
        e.dispatch("select.none".into(), "{}".into()).unwrap();
        e.dispatch(
            "layer.set_position".into(),
            json!({"layer":{"name":name},"x":x,"y":y}).to_string(),
        )
        .unwrap();
    }
    e.dispatch(
        "layer.select".into(),
        json!({"layer":{"name":"Moving"}}).to_string(),
    )
    .unwrap();
    {
        let mut inner = e.lock().unwrap();
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
    let before = frame(&e);
    let original = e
        .lock()
        .unwrap()
        .editor
        .document()
        .unwrap()
        .to_archive()
        .unwrap();
    let state = e.lock().unwrap().editor.snapshot();
    let params=json!({"x":21,"y":15,"guides":false,"artboard":false,"layers":true,"anchors":"edges","tolerance":2}).to_string();
    let mut shown = Vec::new();
    for apply in [false, true] {
        let seq = e.issue_tonal_preview().unwrap();
        e.begin_filter_preview(
            state.active_layer.unwrap().to_string(),
            "layer.snap_position".into(),
            state.revision,
            seq,
        )
        .unwrap();
        e.preview_filter(params.clone(), seq).unwrap();
        let result: Value = serde_json::from_str(&e.snap_preview_result().unwrap()).unwrap();
        assert_eq!(result["x"], 20);
        assert_eq!(result["y"], 14);
        assert_eq!(result["snapped_x"]["coordinate"], 32.);
        assert_eq!(result["snapped_y"]["coordinate"], 24.);
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
        let current = frame(&e);
        assert!(current != before);
        if !apply {
            shown = current;
            e.end_filter_preview_checked(params.clone(), true, seq)
                .unwrap();
            assert_eq!(frame(&e), before);
        } else {
            assert_eq!(current, shown);
            e.end_filter_preview_checked(params.clone(), false, seq)
                .unwrap();
            assert_eq!(frame(&e), shown);
        }
        assert_eq!(e.snap_preview_result().unwrap(), "null");
    }
    {
        let inner = e.lock().unwrap();
        let reference = strata_core::composite::Compositor::new(1)
            .tile(
                inner.editor.document().unwrap(),
                strata_pixels::TileCoord::new(0, 0).unwrap(),
            )
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
                            <= 3
                    );
                }
            }
        }
    }
    e.dispatch("edit.undo".into(), "{}".into()).unwrap();
    assert_eq!(frame(&e), before);
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
