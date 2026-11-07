use serde_json::{Value, json};
use strata_core::{
    Editor, LayerRef, Source,
    commands::{CommandError, TransformScope, WorkCancellation},
};

fn run(e: &mut Editor, id: &str, params: Value) -> Value {
    e.execute(e.request(Source::User), id, params, |_| Ok(()))
        .unwrap()
}
fn fixture() -> Editor {
    let mut e = Editor::new();
    run(
        &mut e,
        "file.new",
        json!({"width":16,"height":12,"bit_depth":16,"background":[12001,30003,50001,32769]}),
    );
    e
}
fn layer(e: &Editor) -> LayerRef {
    LayerRef::Id(e.snapshot().active_layer.unwrap())
}
fn params(e: &Editor, scope: TransformScope) -> Value {
    json!({"layer":layer(e),"scope":scope,"geometry":{"type":"corners","source":[[0,0],[16,0],[16,12],[0,12]],"destination":[[-1,1],[14,-2],[18,14],[2,13]]},"interpolation":"bilinear"})
}
#[test]
fn owned_whole_layer_bounds_include_negative_offboard_and_hidden_codes_without_edits() {
    let mut e = fixture();
    let id = e.snapshot().active_layer;
    run(
        &mut e,
        "paint.erase",
        json!({"layer":{"id":id},"points":[{"x":8,"y":6}],"size":64,"hardness":1}),
    );
    assert_eq!(
        e.document().unwrap().layers()[0]
            .pixels()
            .pixel(0, 0)
            .unwrap(),
        strata_pixels::PixelValue::Rgba16([12001, 30003, 50001, 0])
    );
    run(
        &mut e,
        "layer.set_position",
        json!({"layer":{"id":id},"x":-9,"y":-4}),
    );
    run(
        &mut e,
        "image.crop",
        json!({"rect":{"x":0,"y":0,"width":4,"height":4},"pixels":"preserve"}),
    );
    let before = e.document().unwrap().to_archive().unwrap();
    let interaction = e
        .begin_canvas_transform(layer(&e), TransformScope::WholeLayer)
        .unwrap();
    let bounds = e
        .canvas_transform_bounds(&interaction)
        .unwrap()
        .bounds
        .unwrap();
    assert_eq!(
        (bounds.x, bounds.y, bounds.width, bounds.height),
        (-9, -4, 16, 12)
    );
    assert!(e.document().unwrap().to_archive().unwrap() == before);
}
#[test]
fn selected_draft_uses_exact_selection_frame_and_keeps_original_until_one_apply() {
    let mut e = fixture();
    run(
        &mut e,
        "select.marquee",
        json!({"x":3,"y":2,"width":7,"height":6}),
    );
    run(&mut e, "select.feather", json!({"radius":1}));
    let before = e.document().unwrap().to_archive().unwrap();
    let scope = TransformScope::Selection;
    let interaction = e.begin_canvas_transform(layer(&e), scope).unwrap();
    let bounds = e
        .canvas_transform_bounds(&interaction)
        .unwrap()
        .bounds
        .unwrap();
    assert!(
        bounds.x <= 3
            && bounds.y <= 2
            && bounds.x + i64::from(bounds.width) >= 10
            && bounds.y + i64::from(bounds.height) >= 8
    );
    let parameters = params(&e, scope);
    let candidate = e
        .prepare_canvas_transform(&interaction, parameters.clone())
        .unwrap();
    assert!(e.document().unwrap().to_archive().unwrap() == before);
    let mut direct = e.clone();
    run(&mut direct, "transform.apply", parameters);
    let cursor = e.snapshot().history_cursor;
    assert_eq!(e.commit(candidate).unwrap()["changed"], true);
    assert_eq!(e.snapshot().history_cursor, cursor + 1);
    let mut actual = e.document().unwrap().to_archive().unwrap();
    let expected = direct.document().unwrap().to_archive().unwrap();
    // Independent successful commands intentionally allocate distinct StateIds.
    actual.metadata["state"] = expected.metadata["state"].clone();
    assert!(actual == expected);
    run(&mut e, "edit.undo", json!({}));
    assert!(e.document().unwrap().to_archive().unwrap() == before);
}
#[test]
fn layer_scope_and_operation_are_fixed_before_pointer_input() {
    let mut e = fixture();
    let original = layer(&e);
    run(&mut e, "layer.create", json!({"name":"second"}));
    let other = layer(&e);
    run(&mut e, "layer.select", json!({"layer":original}));
    let interaction = e
        .begin_canvas_transform(original, TransformScope::WholeLayer)
        .unwrap();
    let before = e.document().unwrap().to_archive().unwrap();
    let mut p = params(&e, TransformScope::Selection);
    assert!(e.prepare_canvas_transform(&interaction, p.clone()).is_err());
    p["scope"] = json!("whole_layer");
    p["layer"] = json!(other);
    assert!(e.prepare_canvas_transform(&interaction, p).is_err());
    assert!(
        e.prepare_crop(
            &interaction,
            json!({"rect":{"x":1,"y":1,"width":8,"height":8}})
        )
        .is_err()
    );
    let crop = e.begin_crop().unwrap();
    assert!(e.canvas_transform_bounds(&crop).is_err());
    assert!(
        e.prepare_canvas_transform(&crop, params(&e, TransformScope::WholeLayer))
            .is_err()
    );
    assert!(e.document().unwrap().to_archive().unwrap() == before);
    let revision = e.revision();
    let candidate = e
        .prepare_canvas_transform(&interaction, params(&e, TransformScope::WholeLayer))
        .unwrap();
    run(&mut e, "layer.select", json!({"layer":other}));
    assert_eq!(e.revision(), revision);
    assert!(e.commit(candidate).is_err());
    assert!(e.canvas_transform_bounds(&interaction).is_err());
    assert!(
        e.prepare_canvas_transform(&interaction, params(&e, TransformScope::WholeLayer))
            .is_err()
    );
}
#[test]
fn local_cancel_does_not_revoke_newer_editor_work_and_stale_contexts_cannot_apply() {
    let mut e = fixture();
    let interaction = e
        .begin_canvas_transform(layer(&e), TransformScope::WholeLayer)
        .unwrap();
    let work = WorkCancellation::default();
    let local = interaction.with_work_cancellation(&work);
    work.cancel();
    assert!(matches!(
        e.canvas_transform_bounds(&local),
        Err(CommandError::Expired)
    ));
    assert!(matches!(
        e.prepare_canvas_transform(&local, params(&e, TransformScope::WholeLayer)),
        Err(CommandError::Expired)
    ));
    let target = layer(&e);
    run(
        &mut e,
        "layer.rename",
        json!({"layer":target,"name":"new revision"}),
    );
    assert!(e.canvas_transform_bounds(&interaction).is_err());
    assert!(
        e.prepare_canvas_transform(&interaction, params(&e, TransformScope::WholeLayer))
            .is_err()
    );
}
#[test]
fn all_identity_input_forms_are_exact_history_free_and_preserve_newer_layer_selection() {
    let corners = json!([[0, 0], [16, 0], [16, 12], [0, 12]]);
    for scope in [TransformScope::WholeLayer, TransformScope::Selection] {
        for geometry in [
            json!({"type":"components"}),
            json!({"type":"affine","values":[1,0,0,1,0,0]}),
            json!({"type":"matrix","values":[1,0,0,0,1,0,0,0,1]}),
            json!({"type":"corners","source":corners,"destination":corners}),
        ] {
            let mut e = fixture();
            let original = layer(&e);
            run(&mut e, "layer.create", json!({"name":"other"}));
            let other = layer(&e);
            run(&mut e, "layer.select", json!({"layer":original}));
            if scope == TransformScope::Selection {
                run(&mut e, "select.all", json!({}));
                run(&mut e, "select.feather", json!({"radius":1}));
            }
            let before = e.document().unwrap().to_archive().unwrap();
            let cursor = e.snapshot().history_cursor;
            let revision = e.revision();
            let dirty = e.dirty();
            let candidate = e
                .prepare(
                    e.request(Source::User),
                    "transform.apply",
                    json!({"layer":original,"scope":scope,"geometry":geometry}),
                )
                .unwrap();
            run(&mut e, "layer.select", json!({"layer":other}));
            let selected = e.document().unwrap().to_archive().unwrap();
            assert_eq!(e.commit(candidate).unwrap()["changed"], false);
            assert_eq!(
                e.snapshot().active_layer,
                Some(match other {
                    LayerRef::Id(id) => id,
                    _ => unreachable!(),
                })
            );
            assert_eq!(e.revision(), revision);
            assert_eq!(e.snapshot().history_cursor, cursor);
            assert_eq!(e.dirty(), dirty);
            assert!(e.document().unwrap().to_archive().unwrap() == selected);
            assert!(e.document().unwrap().to_archive().unwrap().tiles == before.tiles);
        }
    }
}
#[test]
fn failed_display_and_bad_or_invisible_targets_keep_the_original() {
    let mut e = fixture();
    let before = e.document().unwrap().to_archive().unwrap();
    let interaction = e
        .begin_canvas_transform(layer(&e), TransformScope::WholeLayer)
        .unwrap();
    let request = params(&e, TransformScope::WholeLayer);
    assert!(
        e.execute(
            e.request(Source::User),
            "transform.apply",
            request,
            |_| Err("display allocation failed".into())
        )
        .is_err()
    );
    assert!(e.document().unwrap().to_archive().unwrap() == before);
    assert!(e.canvas_transform_bounds(&interaction).is_ok());
    let target = layer(&e);
    run(
        &mut e,
        "layer.set_visibility",
        json!({"layer":target,"visible":false}),
    );
    assert!(
        e.begin_canvas_transform(target.clone(), TransformScope::WholeLayer)
            .is_err()
    );
    run(
        &mut e,
        "layer.set_visibility",
        json!({"layer":target,"visible":true}),
    );
    for locks in [
        json!({"all":true}),
        json!({"position":true}),
        json!({"pixels":true}),
        json!({"transparency":true}),
    ] {
        run(
            &mut e,
            "layer.set_locks",
            json!({"layer":target,"locks":locks}),
        );
        assert!(
            e.begin_canvas_transform(target.clone(), TransformScope::WholeLayer)
                .is_err()
        );
        run(
            &mut e,
            "layer.set_locks",
            json!({"layer":target,"locks":{}}),
        );
    }
}
