use serde_json::json;
use strata_core::{Editor, Source};
fn run(e: &mut Editor, id: &str, args: serde_json::Value) -> serde_json::Value {
    let r = e.request(Source::User);
    e.execute(r, id, args, |_| Ok(())).unwrap()
}
#[test]
fn project_save_reopen_and_snapshot_dirty_state() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("project.strata");
    let mut e = Editor::new();
    run(
        &mut e,
        "file.new",
        json!({"width":2,"height":3,"bit_depth":16}),
    );
    let layer = run(
        &mut e,
        "layer.create",
        json!({"name":"filled","fill":[32768,32769,50001,65535]}),
    );
    let id = e.document().unwrap().id();
    let pixels = e.document().unwrap().export_pixels().unwrap();
    let grant = e.grant_write(&path, false, Source::User).unwrap();
    run(&mut e, "file.save_as", json!({"resource":grant}));
    assert!(!e.dirty());
    run(
        &mut e,
        "layer.rename",
        json!({"layer":{"id":layer["layer_id"]},"name":"new"}),
    );
    assert!(e.dirty());
    run(&mut e, "edit.undo", json!({}));
    assert!(!e.dirty());
    run(&mut e, "edit.redo", json!({}));
    let grant = e.grant_read(&path, Source::User).unwrap();
    run(
        &mut e,
        "file.open",
        json!({"resource":grant,"discard_changes":true}),
    );
    assert_eq!(e.document().unwrap().id(), id);
    assert_eq!(e.document().unwrap().export_pixels().unwrap(), pixels);
    assert!(!e.dirty());
}

#[test]
fn saved_active_layer_tracks_selection_without_content_history() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("selection.strata");
    let mut e = Editor::new();
    run(
        &mut e,
        "file.new",
        json!({"width":2,"height":2,"bit_depth":8}),
    );
    let first = e.document().unwrap().layers()[0].id();
    run(&mut e, "layer.create", json!({"name":"second"}));
    let revision = e.revision();
    let history = e.snapshot().history.len();
    run(&mut e, "layer.select", json!({"layer":{"id":first}}));
    assert_eq!(e.revision(), revision);
    assert_eq!(e.snapshot().history.len(), history);
    let resource = e.grant_write(&path, false, Source::User).unwrap();
    run(&mut e, "file.save_as", json!({"resource":resource}));
    let resource = e.grant_read(&path, Source::User).unwrap();
    run(&mut e, "file.open", json!({"resource":resource}));
    assert_eq!(e.snapshot().active_layer, Some(first));
    assert!(!e.dirty());
}
#[test]
fn save_snapshot_during_edit_does_not_clear_dirty() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("project.strata");
    let mut e = Editor::new();
    run(
        &mut e,
        "file.new",
        json!({"width":2,"height":3,"bit_depth":8}),
    );
    let grant = e.grant_write(&path, false, Source::User).unwrap();
    let request = e.request(Source::User);
    let save = e
        .prepare(request, "file.save_as", json!({"resource":grant}))
        .unwrap();
    run(&mut e, "layer.create", json!({"name":"late edit"}));
    e.commit(save).unwrap();
    assert!(e.dirty());
    run(&mut e, "edit.undo", json!({}));
    assert!(!e.dirty());
}
#[test]
fn file_capabilities_cannot_be_forged_or_used_by_another_source() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("out.strata");
    let mut e = Editor::new();
    run(
        &mut e,
        "file.new",
        json!({"width":2,"height":3,"bit_depth":8}),
    );
    let r = e.request(Source::User);
    assert!(
        e.prepare(r, "file.save_as", json!({"resource":path}))
            .is_err()
    );
    let grant = e.grant_write(&path, false, Source::User).unwrap();
    let turn = e.begin_turn().unwrap();
    let r = e.request(Source::Ai { turn_id: turn });
    assert!(
        e.prepare(r, "file.save_as", json!({"resource":grant}))
            .is_err()
    );
    assert!(!path.exists());
}
#[test]
fn failed_open_and_external_batch_keep_old_document() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bad.png");
    std::fs::write(&path, b"bad").unwrap();
    let mut e = Editor::new();
    run(
        &mut e,
        "file.new",
        json!({"width":2,"height":3,"bit_depth":8}),
    );
    let state = e.document().unwrap().state_id();
    let grant = e.grant_read(&path, Source::User).unwrap();
    let r = e.request(Source::User);
    assert!(
        e.prepare(
            r,
            "file.open",
            json!({"resource":grant,"discard_changes":true})
        )
        .is_err()
    );
    let r = e.request(Source::User);
    assert!(e.prepare(r,"document.batch",json!({"steps":[{"command":"layer.create","params":{"name":"no"}},{"command":"file.save","params":{"resource":"no"}}]})).is_err());
    assert_eq!(e.document().unwrap().state_id(), state);
}

#[test]
fn working_space_conversion_preserves_depth_alpha_and_undo_restores_exactly() {
    let mut e = Editor::new();
    run(
        &mut e,
        "file.new",
        json!({"width":2,"height":2,"bit_depth":16}),
    );
    run(
        &mut e,
        "layer.create",
        json!({"name":"precise","fill":[32768,32769,50001,12345]}),
    );
    let before = e.document().unwrap().export_pixels().unwrap();
    let icc = e.document().unwrap().working_icc().to_vec();
    run(&mut e, "color.convert", json!({"space":"display_p3"}));
    assert_eq!(e.document().unwrap().bit_depth(), 16);
    let strata_pixels::PixelValue::Rgba16(pixel) = e
        .document()
        .unwrap()
        .layers()
        .last()
        .unwrap()
        .pixels()
        .pixel(0, 0)
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(pixel[3], 12345);
    run(&mut e, "edit.undo", json!({}));
    assert_eq!(e.document().unwrap().working_icc(), icc);
    assert_eq!(e.document().unwrap().export_pixels().unwrap(), before);
}
