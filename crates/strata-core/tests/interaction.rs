use serde_json::json;
use strata_core::{Editor, LayerRef, Source};
fn editor() -> Editor {
    let mut e = Editor::new();
    let r = e.request(Source::User);
    e.execute(
        r,
        "file.new",
        json!({"width":16,"height":16,"bit_depth":8}),
        |_| Ok(()),
    )
    .unwrap();
    e
}
#[test]
fn drag_cancel_and_commit_have_one_history_entry() {
    let mut e = editor();
    let state = e.document().unwrap().state_id();
    let gesture = e.begin_move(LayerRef::Name("Background".into())).unwrap();
    let first = e.preview_move(&gesture, -10, 5).unwrap();
    assert_eq!(first.document().unwrap().layers()[0].info().x, -10);
    assert_eq!(e.document().unwrap().state_id(), state);
    drop(first);
    let _cancelled = gesture;
    let gesture = e.begin_move(LayerRef::Name("Background".into())).unwrap();
    let last = e.preview_move(&gesture, -20, 10).unwrap();
    e.commit(last).unwrap();
    assert_eq!(e.snapshot().history.len(), 1);
    assert_eq!(e.document().unwrap().layers()[0].info().x, -20);
}
#[test]
fn stale_gesture_rejects_updates_and_tail_turn_is_grouped() {
    let mut e = editor();
    let gesture = e.begin_move(LayerRef::Name("Background".into())).unwrap();
    let turn = e.begin_turn().unwrap();
    assert!(e.preview_move(&gesture, 2, 3).is_err());
    let r = e.request(Source::Ai { turn_id: turn });
    e.execute(r, "layer.create", json!({"name":"AI"}), |_| Ok(()))
        .unwrap();
    e.pause_turn().unwrap();
    let r = e.request(Source::User);
    e.execute(
        r,
        "layer.set_visibility",
        json!({"layer":{"name":"AI"},"visible":false}),
        |_| Ok(()),
    )
    .unwrap();
    e.stop_turn();
    let r = e.request(Source::User);
    e.execute(r, "edit.undo_turn", json!({"turn_id":turn}), |_| Ok(()))
        .unwrap();
    assert_eq!(e.document().unwrap().layers().len(), 1);
}
