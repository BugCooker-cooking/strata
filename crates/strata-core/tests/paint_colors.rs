use serde_json::{Value, json};
use strata_core::{Editor, Source};
fn run(e: &mut Editor, id: &str, p: Value) -> Value {
    e.execute(e.request(Source::User), id, p, |_| Ok(()))
        .unwrap()
}
#[test]
fn palette_defaults_set_swap_reset_and_retries_work_without_a_document() {
    let mut e = Editor::new();
    assert_eq!(
        run(&mut e, "paint.colors.get", json!({})),
        json!({"foreground":[0.,0.,0.,1.],"background":[1.,1.,1.,1.],"version":0})
    );
    let request = e.request(Source::User);
    let params = json!({"foreground":[0.123456789,0.2,0.3,0.4],"expected_version":0});
    let result = e
        .execute(request.clone(), "paint.colors.set", params.clone(), |_| {
            Ok(())
        })
        .unwrap();
    assert_eq!(result["version"], 1);
    assert_eq!(result["foreground"][0], 0.123456789);
    assert_eq!(result["background"], json!([1., 1., 1., 1.]));
    assert_eq!(
        e.execute(request, "paint.colors.set", params, |_| Ok(()))
            .unwrap(),
        result
    );
    let swapped = run(&mut e, "paint.colors.swap", json!({}));
    assert_eq!(swapped["foreground"], result["background"]);
    assert_eq!(swapped["background"], result["foreground"]);
    let reset = run(&mut e, "paint.colors.reset", json!({}));
    assert_eq!(reset["version"], 3);
    assert_eq!(reset["foreground"], json!([0., 0., 0., 1.]));
    assert!(e.snapshot().history.is_empty());
    assert_eq!(e.revision(), 0);
    assert!(!e.dirty());
    assert!(e.document().is_none());
}
#[test]
fn stale_generation_and_strict_parameters_never_overwrite_new_palette() {
    let mut e = Editor::new();
    let a = e
        .prepare(e.request(Source::User), "paint.colors.swap", json!({}))
        .unwrap();
    let b = e
        .prepare(
            e.request(Source::User),
            "paint.colors.set",
            json!({"foreground":[1.,0.,0.,1.]}),
        )
        .unwrap();
    e.commit(a).unwrap();
    assert!(e.commit(b).is_err());
    let state = run(&mut e, "paint.colors.get", json!({}));
    for p in [
        json!({}),
        json!({"foreground":[-0.1,0.,0.,1.]}),
        json!({"background":[1.,0.,0.,1.0001]}),
        json!({"foreground":[0.,0.,0.]}),
        json!({"foreground":[0.,0.,0.,1.],"version":99}),
        json!({"foreground":[0.,0.,0.,1.],"expected_version":0}),
    ] {
        assert!(
            e.execute(e.request(Source::User), "paint.colors.set", p, |_| Ok(()))
                .is_err()
        );
        assert_eq!(run(&mut e, "paint.colors.get", json!({})), state);
    }
    let old = e.request(Source::User);
    run(&mut e, "paint.colors.reset", json!({}));
    assert!(e.prepare(old, "paint.colors.swap", json!({})).is_err());
}
#[test]
fn document_samples_history_locks_profiles_and_current_layer_remain_independent() {
    let mut e = Editor::new();
    run(
        &mut e,
        "file.new",
        json!({"width":8,"height":8,"bit_depth":16,"background":[10001,30003,50001,32769]}),
    );
    let first = e.snapshot().active_layer;
    let second = run(&mut e, "layer.create", json!({"name":"second"}))["layer_id"].clone();
    run(
        &mut e,
        "layer.set_locks",
        json!({"layer":{"id":first},"locks":{"all":true}}),
    );
    let before = e.document().unwrap().to_archive().unwrap();
    let revision = e.revision();
    let history = e.snapshot().history.len();
    let set = e
        .prepare(
            e.request(Source::User),
            "paint.colors.set",
            json!({"foreground":[0.2,0.3,0.4,0.5]}),
        )
        .unwrap();
    let get = e
        .prepare(e.request(Source::User), "paint.colors.get", json!({}))
        .unwrap();
    run(&mut e, "layer.select", json!({"layer":{"id":first}}));
    e.commit(set).unwrap();
    assert_eq!(e.snapshot().active_layer, first);
    assert!(
        e.commit(get).is_err(),
        "palette query is bound to its original generation"
    );
    let get = e
        .prepare(e.request(Source::User), "paint.colors.get", json!({}))
        .unwrap();
    run(&mut e, "layer.select", json!({"layer":{"id":second}}));
    e.commit(get).unwrap();
    assert_eq!(
        serde_json::to_value(e.snapshot().active_layer).unwrap(),
        second
    );
    assert_eq!(
        e.document().unwrap().to_archive().unwrap().tiles,
        before.tiles
    );
    assert_eq!(e.revision(), revision);
    assert_eq!(e.snapshot().history.len(), history);
    let colors = run(&mut e, "paint.colors.get", json!({}));
    run(
        &mut e,
        "file.new",
        json!({"width":4,"height":4,"working_space":"display_p3","bit_depth":8,"discard_changes":true}),
    );
    assert_eq!(run(&mut e, "paint.colors.get", json!({})), colors);
    assert!(
        e.document()
            .unwrap()
            .to_archive()
            .unwrap()
            .metadata
            .get("paint_colors")
            .is_none()
    );
    let before = e.document().unwrap().to_archive().unwrap();
    let dirty = e.dirty();
    run(&mut e, "paint.colors.swap", json!({}));
    assert_eq!(e.document().unwrap().to_archive().unwrap(), before);
    assert_eq!(e.dirty(), dirty);
}
#[test]
fn native_user_palette_is_locked_during_ai_ownership_and_turn_undo_keeps_tool_state() {
    let mut e = Editor::new();
    run(
        &mut e,
        "file.new",
        json!({"width":8,"height":8,"bit_depth":16}),
    );
    let turn = e.begin_turn().unwrap();
    assert!(
        e.execute(
            e.request(Source::User),
            "paint.colors.reset",
            json!({}),
            |_| Ok(())
        )
        .is_err()
    );
    let p = e.request(Source::Ai { turn_id: turn });
    e.execute(
        p,
        "paint.colors.set",
        json!({"foreground":[1.,0.,0.,1.]}),
        |_| Ok(()),
    )
    .unwrap();
    assert!(e.snapshot().history.is_empty());
    e.pause_turn().unwrap();
    assert!(
        e.execute(
            e.request(Source::User),
            "paint.colors.swap",
            json!({}),
            |_| Ok(())
        )
        .is_err()
    );
    e.execute(
        e.request(Source::Ai { turn_id: turn }),
        "paint.colors.get",
        json!({}),
        |_| Ok(()),
    )
    .unwrap();
    e.resume_turn().unwrap();
    e.execute(
        e.request(Source::Ai { turn_id: turn }),
        "layer.create",
        json!({"name":"AI layer"}),
        |_| Ok(()),
    )
    .unwrap();
    e.stop_turn();
    run(&mut e, "edit.undo_turn", json!({"turn_id":turn}));
    assert_eq!(
        run(&mut e, "paint.colors.get", json!({}))["foreground"],
        json!([1., 0., 0., 1.])
    );
    assert_eq!(e.snapshot().layers.len(), 1);
}
