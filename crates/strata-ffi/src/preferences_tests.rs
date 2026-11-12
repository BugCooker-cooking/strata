use crate::*;

struct Listener;
impl EngineListener for Listener {
    fn on_event(&self, _: EngineEventDto) {}
}

#[test]
fn native_ai_defaults_feed_real_session_limits_without_network_requests() {
    let engine = StrataEngine::new(Arc::new(Listener));
    let original: Value = serde_json::from_str(
        &engine
            .dispatch("system.preferences.get".into(), "{}".into())
            .unwrap(),
    )
    .unwrap();
    engine.dispatch("system.preferences.set".into(), json!({"ai":{"mode":"direct","max_steps":1,"request_timeout_seconds":7,"turn_timeout_seconds":45}}).to_string()).unwrap();
    let turn = engine.ai_begin(
        json!({"kind":"open_ai","endpoint":"https://example.test/v1","model":"test","vision":false}).to_string(),
        String::new(), "Test saved limits".into(), false, "[]".into(),
    ).unwrap();
    let request: Value = serde_json::from_str(&engine.ai_poll(turn.clone()).unwrap()).unwrap();
    assert_eq!(request["timeout_seconds"], 7);
    let response = json!({"choices":[{"finish_reason":"tool_calls","message":{"tool_calls":[
        {"id":"first","type":"function","function":{"name":"inspect_document","arguments":"{}"}},
        {"id":"extra","type":"function","function":{"name":"inspect_document","arguments":"{}"}}
    ]}}]})
    .to_string();
    let request_id = request["request_id"].as_str().unwrap().to_owned();
    engine
        .ai_feed(request_id.clone(), response.into_bytes())
        .unwrap();
    engine.ai_finish_response(request_id).unwrap();
    let first: Value = serde_json::from_str(&engine.ai_poll(turn.clone()).unwrap()).unwrap();
    assert_eq!(first["kind"], "tool");
    assert!(engine.ai_poll(turn).is_err());
    assert_eq!(
        engine.lock().unwrap().editor.control(),
        &strata_core::commands::Control::User
    );
    engine
        .dispatch(
            "system.preferences.set".into(),
            json!({"ai":original["ai"]}).to_string(),
        )
        .unwrap();
}

#[test]
fn required_metal_preferences_and_trim_preserve_pixels_history_and_document() {
    let mut renderer = match Renderer::new_offscreen(Viewport {
        width: 32,
        height: 24,
        scale: 1.,
    }) {
        Ok(renderer) => renderer,
        Err(strata_render::RenderError::NoAdapter)
            if std::env::var_os("STRATA_REQUIRE_GPU").is_none() =>
        {
            return;
        }
        Err(error) => panic!("Metal: {error}"),
    };
    let engine = StrataEngine::new(Arc::new(Listener));
    engine.dispatch("file.new".into(), json!({"width":32,"height":24,"bit_depth":16,"working_space":"display_p3","background":[10001,30003,50001,32769]}).to_string()).unwrap();
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
    let state = engine.snapshot_json().unwrap();
    let original: Value = serde_json::from_str(
        &engine
            .dispatch("system.preferences.get".into(), "{}".into())
            .unwrap(),
    )
    .unwrap();
    engine.dispatch("system.preferences.set".into(), json!({"ai":{"mode":"ask_first","max_steps":20,"request_timeout_seconds":30,"turn_timeout_seconds":120},"memory":{"cpu_mib":1024,"gpu_mib":256,"disk_mib":16384}}).to_string()).unwrap();
    assert_eq!(frame(&engine), before);
    let status: Value = serde_json::from_str(
        &engine
            .dispatch("system.memory.trim".into(), "{}".into())
            .unwrap(),
    )
    .unwrap();
    assert_eq!(status["limits"]["cpu_bytes"], 1024_u64 * 1024 * 1024);
    assert_eq!(frame(&engine), before);
    assert_eq!(engine.snapshot_json().unwrap(), state);
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
        .dispatch(
            "system.preferences.set".into(),
            json!({"ai":original["ai"],"memory":original["memory"]}).to_string(),
        )
        .unwrap();
    assert_eq!(frame(&engine), before);
}
