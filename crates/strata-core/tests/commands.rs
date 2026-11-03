use serde_json::json;
use strata_core::{Editor, Source};

fn editor() -> Editor {
    let mut editor = Editor::new();
    let request = editor.request(Source::User);
    editor
        .execute(
            request,
            "file.new",
            json!({"width":16,"height":16,"bit_depth":16}),
            |_| Ok(()),
        )
        .unwrap();
    editor
}
fn run(e: &mut Editor, id: &str, args: serde_json::Value) -> serde_json::Value {
    let req = e.request(Source::User);
    e.execute(req, id, args, |_| Ok(())).unwrap()
}

#[test]
fn mutation_history_and_ids_survive_undo() {
    let mut e = editor();
    let first = run(&mut e, "layer.create", json!({"name":"A"}));
    let id = first["layer_id"].clone();
    run(
        &mut e,
        "layer.rename",
        json!({"layer":{"id":id},"name":"renamed"}),
    );
    let revision = e.revision();
    run(&mut e, "edit.undo", json!({}));
    assert!(e.revision() > revision);
    assert_eq!(
        e.document().unwrap().layers().last().unwrap().info().name,
        "A"
    );
    run(&mut e, "edit.redo", json!({}));
    assert_eq!(
        e.document().unwrap().layers().last().unwrap().info().name,
        "renamed"
    );
}

#[test]
fn retry_does_not_repeat_and_changed_payload_rejected() {
    let mut e = editor();
    let req = e.request(Source::User);
    let a = e
        .execute(req.clone(), "layer.create", json!({"name":"A"}), |_| Ok(()))
        .unwrap();
    let b = e
        .execute(req.clone(), "layer.create", json!({"name":"A"}), |_| Ok(()))
        .unwrap();
    assert_eq!(a, b);
    assert_eq!(e.document().unwrap().layers().len(), 2);
    assert!(
        e.execute(req, "layer.create", json!({"name":"B"}), |_| Ok(()))
            .is_err()
    );
}

#[test]
fn concurrently_prepared_retries_commit_once_and_detect_conflicts() {
    let mut e = editor();
    let req = e.request(Source::User);
    let first = e
        .prepare(req.clone(), "layer.create", json!({"name":"once"}))
        .unwrap();
    let second = e
        .prepare(req.clone(), "layer.create", json!({"name":"once"}))
        .unwrap();
    let conflict = e
        .prepare(req, "layer.create", json!({"name":"conflict"}))
        .unwrap();
    let result = e.commit(first).unwrap();
    let revision = e.revision();
    assert_eq!(e.commit(second).unwrap(), result);
    assert!(matches!(
        e.commit(conflict),
        Err(strata_core::commands::CommandError::CallConflict)
    ));
    assert_eq!(e.revision(), revision);
    assert_eq!(e.document().unwrap().layers().len(), 2);
}

#[test]
fn stale_and_stopped_candidates_never_commit() {
    let mut e = editor();
    let req = e.request(Source::User);
    let stale = e
        .prepare(req, "layer.create", json!({"name":"old"}))
        .unwrap();
    run(&mut e, "layer.create", json!({"name":"new"}));
    assert!(e.commit(stale).is_err());
    let turn = e.begin_turn().unwrap();
    let req = e.request(Source::Ai { turn_id: turn });
    let stopped = e
        .prepare(req, "layer.create", json!({"name":"late"}))
        .unwrap();
    e.stop_turn();
    assert!(e.commit(stopped).is_err());
    assert_eq!(e.document().unwrap().layers().len(), 2);
}

#[test]
fn control_and_failure_keep_document() {
    let mut e = editor();
    let state = e.document().unwrap().state_id();
    let req = e.request(Source::User);
    assert!(
        e.execute(req, "layer.create", json!({"name":"bad GPU"}), |_| Err(
            "injected prepare failure".into()
        ))
        .is_err()
    );
    assert_eq!(e.document().unwrap().state_id(), state);
    e.begin_turn().unwrap();
    let req = e.request(Source::User);
    assert!(
        e.prepare(req, "layer.create", json!({"name":"locked"}))
            .is_err()
    );
}

#[test]
fn batch_result_references_and_atomic_failure() {
    let mut e = editor();
    let req = e.request(Source::User);
    e.execute(req,"document.batch",json!({"steps":[
        {"command":"layer.create","params":{"name":"batch"}},
        {"command":"layer.set_opacity","params":{"layer":{"id":{"$ref":{"step":0,"field":"layer_id"}}},"opacity":0.5}}
    ]}), |_|Ok(())).unwrap();
    assert_eq!(
        e.document()
            .unwrap()
            .layers()
            .last()
            .unwrap()
            .info()
            .opacity,
        0.5
    );
    let state = e.document().unwrap().state_id();
    let req = e.request(Source::User);
    assert!(
        e.execute(
            req,
            "document.batch",
            json!({"steps":[
                {"command":"layer.create","params":{"name":"discard"}},
                {"command":"layer.set_opacity","params":{"layer":{"name":"discard"},"opacity":2.0}}
            ]}),
            |_| Ok(())
        )
        .is_err()
    );
    assert_eq!(e.document().unwrap().state_id(), state);
    run(&mut e, "edit.undo", json!({}));
    assert_eq!(e.document().unwrap().layers().len(), 1);
}

#[test]
fn ambiguous_names_and_undo_branch_do_not_reuse_ids() {
    let mut e = editor();
    let first = run(&mut e, "layer.create", json!({"name":"Background"}));
    let req = e.request(Source::User);
    assert!(
        e.prepare(
            req,
            "layer.rename",
            json!({"layer":{"name":"Background"},"name":"ambiguous"})
        )
        .is_err()
    );
    run(&mut e, "edit.undo", json!({}));
    let second = run(&mut e, "layer.create", json!({"name":"new branch"}));
    assert_ne!(first["layer_id"], second["layer_id"]);
    let req = e.request(Source::User);
    assert!(e.prepare(req, "edit.redo", json!({})).is_err());
}
