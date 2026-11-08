use serde_json::{Value, json};
use strata_core::{Editor, Source};
fn run(e: &mut Editor, id: &str, p: Value) -> Value {
    e.execute(e.request(Source::User), id, p, |_| Ok(()))
        .unwrap()
}
fn fixture() -> Editor {
    let mut e = Editor::new();
    run(
        &mut e,
        "file.new",
        json!({"width":80,"height":40,"bit_depth":16}),
    );
    run(
        &mut e,
        "view.guides.add",
        json!({"name":"Absolute","axis":"vertical","position":17.5}),
    );
    run(
        &mut e,
        "view.presentation.set",
        json!({"ruler_unit":"percent"}),
    );
    e
}
#[test]
fn owned_origin_draft_prepares_same_strict_view_action_without_touching_document_or_guides() {
    let mut e = fixture();
    let before = e.document().unwrap().to_archive().unwrap();
    let state = e.snapshot();
    let draft = e.begin_ruler_origin().unwrap();
    let prepared = e.prepare_ruler_origin(&draft, [-12.5, 30.25]).unwrap();
    assert_eq!(prepared.result()["ruler_origin"], json!([-12.5, 30.25]));
    assert!(!prepared.changes_display());
    assert_eq!(e.snapshot().presentation, state.presentation);
    e.commit(prepared).unwrap();
    let moved = e.snapshot();
    assert_eq!(moved.presentation.ruler_origin, [-12.5, 30.25]);
    assert_eq!(moved.presentation.version, state.presentation.version + 1);
    assert_eq!(moved.history_cursor, state.history_cursor);
    assert_eq!(moved.revision, state.revision);
    assert_eq!(moved.dirty, state.dirty);
    assert!(e.document().unwrap().to_archive().unwrap() == before);
    let same = e.begin_ruler_origin().unwrap();
    e.commit(e.prepare_ruler_origin(&same, [-12.5, 30.25]).unwrap())
        .unwrap();
    assert_eq!(
        e.snapshot().presentation.version,
        moved.presentation.version
    );
    run(&mut e, "edit.undo", json!({}));
    assert_eq!(e.snapshot().presentation, moved.presentation);
    assert!(e.document().unwrap().guides().is_empty());
}
#[test]
fn origin_owner_rejects_late_document_revision_view_control_or_cancelled_work() {
    for change in ["document", "revision", "presentation", "epoch", "work"] {
        let mut e = fixture();
        let draft = e.begin_ruler_origin().unwrap();
        let pending = e.prepare_ruler_origin(&draft, [20., 10.]).unwrap();
        let work = strata_core::commands::WorkCancellation::default();
        let cancellable = draft.with_work_cancellation(&work);
        match change {
            "document" => {
                run(
                    &mut e,
                    "file.new",
                    json!({"width":4,"height":4,"bit_depth":8,"discard_changes":true}),
                );
            }
            "revision" => {
                run(
                    &mut e,
                    "layer.rename",
                    json!({"layer":{"name":"Background"},"name":"new"}),
                );
            }
            "presentation" => {
                run(
                    &mut e,
                    "view.presentation.set",
                    json!({"rulers_visible":false}),
                );
            }
            "epoch" => {
                e.begin_turn().unwrap();
            }
            "work" => work.cancel(),
            _ => unreachable!(),
        }
        let before = e.document().unwrap().to_archive().unwrap();
        let presentation = e.snapshot().presentation;
        assert!(
            e.prepare_ruler_origin(&cancellable, [20., 10.]).is_err(),
            "{change}"
        );
        if change != "work" {
            assert!(e.commit(pending).is_err(), "{change}");
        }
        assert_eq!(e.snapshot().presentation, presentation);
        assert!(e.document().unwrap().to_archive().unwrap() == before);
    }
}
#[test]
fn origin_is_view_permission_during_ai_and_invalid_coordinates_are_atomic() {
    let mut e = fixture();
    e.begin_turn().unwrap();
    let before = e.document().unwrap().to_archive().unwrap();
    let draft = e.begin_ruler_origin().unwrap();
    for origin in [[f64::NAN, 0.], [f64::INFINITY, 0.], [1e12 + 1., 0.]] {
        assert!(e.prepare_ruler_origin(&draft, origin).is_err());
    }
    let prepared = e.prepare_ruler_origin(&draft, [1.25, -2.5]).unwrap();
    e.commit(prepared).unwrap();
    assert_eq!(e.snapshot().presentation.ruler_origin, [1.25, -2.5]);
    assert!(e.document().unwrap().to_archive().unwrap() == before);
    e.stop_turn();
}
