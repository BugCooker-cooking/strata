use crate::*;
#[derive(Default)]
struct Listener(Mutex<usize>);
impl EngineListener for Listener {
    fn on_event(&self, event: EngineEventDto) {
        if matches!(event, EngineEventDto::ExportPresetChanged) {
            *self.0.lock().unwrap() += 1;
        }
    }
}
#[test]
fn required_metal_quick_export_settings_and_actual_file_keep_exact_scene_and_document() {
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
    let listener = Arc::new(Listener::default());
    let e = StrataEngine::new(listener.clone());
    e.dispatch("file.new".into(),json!({"width":32,"height":24,"bit_depth":16,"working_space":"display_p3","background":[10001,30003,50001,32769]}).to_string()).unwrap();
    let initial: Value = serde_json::from_str(
        &e.dispatch("file.export_preset.get".into(), "{}".into())
            .unwrap(),
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
                center_x: 16.,
                center_y: 12.,
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
    let p:Value=serde_json::from_str(&e.dispatch("file.export_preset.set".into(),json!({"options":{"format":"png","width":8,"height":6,"target_space":"srgb"},"expected_version":initial["version"]}).to_string()).unwrap()).unwrap();
    assert_eq!(frame(&e), before);
    assert_eq!(*listener.0.lock().unwrap(), 1);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("actual.png");
    let resource = e.grant_write(path.display().to_string(), false).unwrap();
    e.dispatch(
        "file.quick_export".into(),
        json!({"resource":resource,"preset_version":p["version"]}).to_string(),
    )
    .unwrap();
    assert_eq!(frame(&e), before);
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
    assert_eq!(e.lock().unwrap().editor.snapshot().revision, state.revision);
    let decoded = strata_io::decode_image(&path).unwrap();
    assert_eq!((decoded.width, decoded.height), (8, 6));
    assert_eq!(decoded.pixels.format().bit_depth(), 16);
    e.dispatch(
        "file.export_preset.set".into(),
        json!({"options":initial["options"]}).to_string(),
    )
    .unwrap();
    assert_eq!(frame(&e), before);
}
