use crate::*;
struct Listener;
impl EngineListener for Listener {
    fn on_event(&self, _: EngineEventDto) {}
}
#[test]
fn required_metal_revert_reloads_saved_source_restores_scene_and_removes_only_current_checkpoint() {
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
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("saved.strata");
    e.configure_recovery(dir.path().join("recovery").display().to_string())
        .unwrap();
    let other = StrataEngine::new(Arc::new(Listener));
    other
        .configure_recovery(dir.path().join("recovery").display().to_string())
        .unwrap();
    other
        .dispatch(
            "file.new".into(),
            json!({"width":2,"height":2,"bit_depth":8}).to_string(),
        )
        .unwrap();
    other.checkpoint_recovery("other document".into()).unwrap();
    let other_session = other
        .lock()
        .unwrap()
        .editor
        .snapshot()
        .session_id
        .to_string();
    e.dispatch("file.new".into(),json!({"width":32,"height":24,"bit_depth":16,"working_space":"display_p3","background":[10001,30003,50001,32769]}).to_string()).unwrap();
    let write = e.grant_write(path.display().to_string(), false).unwrap();
    e.dispatch("file.save_as".into(), json!({"resource":write}).to_string())
        .unwrap();
    {
        let mut inner = e.lock().unwrap();
        let scene = renderer
            .prepare_document(inner.editor.document().unwrap(), true)
            .unwrap();
        renderer.commit_document(scene).unwrap();
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
    let disk = std::fs::read(&path).unwrap();
    e.dispatch(
        "layer.set_position".into(),
        json!({"layer":{"name":"Background"},"x":8,"y":6}).to_string(),
    )
    .unwrap();
    assert_ne!(frame(&e), before);
    e.checkpoint_recovery("revert fixture".into()).unwrap();
    let entries: Value = serde_json::from_str(&e.recovery_entries_json().unwrap()).unwrap();
    assert_eq!(entries.as_array().unwrap().len(), 2);
    let read = e.grant_read(path.display().to_string()).unwrap();
    let expected_revision = e.lock().unwrap().editor.revision();
    e.dispatch(
        "file.revert".into(),
        json!({"resource":read,"discard_changes":true,"expected_revision":expected_revision})
            .to_string(),
    )
    .unwrap();
    assert_eq!(frame(&e), before);
    let inner = e.lock().unwrap();
    assert!(inner.editor.document().unwrap().to_archive().unwrap() == archive);
    assert!(inner.editor.snapshot().history.is_empty());
    assert!(!inner.editor.dirty());
    drop(inner);
    assert_eq!(std::fs::read(&path).unwrap(), disk);
    let entries: Value = serde_json::from_str(&e.recovery_entries_json().unwrap()).unwrap();
    assert_eq!(entries.as_array().unwrap().len(), 1);
    assert_eq!(entries[0]["info"]["session_id"], other_session);
}
