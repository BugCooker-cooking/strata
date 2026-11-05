use serde_json::{Value, json};
use strata_core::{Editor, LayerKind, Source};
fn run(e: &mut Editor, id: &str, p: Value) -> Value {
    e.execute(e.request(Source::User), id, p, |_| Ok(()))
        .unwrap()
}
fn editor() -> Editor {
    let mut e = Editor::new();
    run(
        &mut e,
        "file.new",
        json!({"width":4,"height":4,"bit_depth":16,"background":[1000,2000,3000,65535]}),
    );
    e
}

#[test]
fn groups_render_isolated_and_visibility_opacity_masks_undo_and_save_work() {
    let mut e = editor();
    let front = run(
        &mut e,
        "layer.create",
        json!({"name":"front","fill":[20000,20000,20000,65535]}),
    )["layer_id"]
        .clone();
    let before = e.document().unwrap().export_pixels().unwrap();
    let group = run(
        &mut e,
        "layer.group",
        json!({"layers":[{"id":front}],"name":"group"}),
    )["layer_id"]
        .clone();
    assert_eq!(e.document().unwrap().export_pixels().unwrap(), before);
    run(
        &mut e,
        "layer.set_opacity",
        json!({"layer":{"id":group},"opacity":0.5}),
    );
    let faded = e.document().unwrap().export_pixels().unwrap();
    assert_ne!(faded, before);
    run(
        &mut e,
        "layer.set_visibility",
        json!({"layer":{"id":group},"visible":false}),
    );
    let hidden = e.document().unwrap().export_pixels().unwrap();
    assert_ne!(hidden, faded);
    run(&mut e, "edit.undo", json!({}));
    assert_eq!(e.document().unwrap().export_pixels().unwrap(), faded);
    run(
        &mut e,
        "layer.mask.add",
        json!({"layer":{"id":group},"source":"black"}),
    );
    assert_eq!(e.document().unwrap().export_pixels().unwrap(), hidden);
    let archive = e.document().unwrap().to_archive().unwrap();
    let doc = strata_core::Document::from_archive(archive.clone()).unwrap();
    assert_eq!(doc.to_archive().unwrap(), archive);
    assert_eq!(doc.export_pixels().unwrap(), hidden);
}
#[test]
fn nested_groups_move_duplicate_and_delete_entire_subtrees_with_stable_history() {
    let mut e = editor();
    let group = run(
        &mut e,
        "layer.group",
        json!({"layers":[{"name":"Background"}],"name":"inner"}),
    )["layer_id"]
        .clone();
    let outer = run(
        &mut e,
        "layer.group",
        json!({"layers":[{"id":group}],"name":"outer"}),
    )["layer_id"]
        .clone();
    let original = e.document().unwrap().layers()[0].id();
    assert_eq!(e.document().unwrap().layer_depth(original), 2);
    run(
        &mut e,
        "layer.set_position",
        json!({"layer":{"id":outer},"x":-10,"y":20}),
    );
    assert_eq!(
        (
            e.document().unwrap().layers()[0].info().x,
            e.document().unwrap().layers()[0].info().y
        ),
        (-10, 20)
    );
    let copy = run(&mut e, "layer.duplicate", json!({"layer":{"id":outer}}))["layer_id"].clone();
    assert_eq!(e.document().unwrap().layers().len(), 6);
    assert!(
        e.document()
            .unwrap()
            .layers()
            .iter()
            .filter(|l| matches!(l.info().kind, LayerKind::Group))
            .count()
            == 4
    );
    let before = e.document().unwrap().to_archive().unwrap();
    run(&mut e, "layer.delete", json!({"layer":{"id":copy}}));
    assert_eq!(e.document().unwrap().layers().len(), 3);
    run(&mut e, "edit.undo", json!({}));
    assert_eq!(e.document().unwrap().to_archive().unwrap(), before);
}
#[test]
fn reparent_cycles_depth_orphan_and_locked_subtree_fail_without_changing_state() {
    let mut e = editor();
    let a = run(&mut e, "layer.create_group", json!({"name":"a"}))["layer_id"].clone();
    let b = run(&mut e, "layer.create_group", json!({"name":"b"}))["layer_id"].clone();
    let before = e.document().unwrap().to_archive().unwrap();
    for p in [
        json!({"layer":{"id":a},"parent":{"id":b}}),
        json!({"layer":{"id":a},"parent":{"id":a}}),
        json!({"layer":{"id":a},"parent":{"name":"Background"}}),
    ] {
        assert!(
            e.execute(e.request(Source::User), "layer.reparent", p, |_| Ok(()))
                .is_err()
        );
        assert_eq!(e.document().unwrap().to_archive().unwrap(), before);
    }
    let mut corrupt = before.clone();
    corrupt.metadata["layers"][0]["parent"] = json!("00000000-0000-4000-8000-000000000001");
    assert!(strata_core::Document::from_archive(corrupt).is_err());
    run(
        &mut e,
        "layer.set_locks",
        json!({"layer":{"id":b},"locks":{"position":true}}),
    );
    assert!(
        e.execute(
            e.request(Source::User),
            "layer.set_position",
            json!({"layer":{"id":a},"x":1,"y":0}),
            |_| Ok(())
        )
        .is_err()
    );
}
#[test]
fn new_children_and_ungroup_preserve_root_z_order() {
    let mut e = editor();
    let group = run(&mut e, "layer.create_group", json!({"name":"group"}))["layer_id"].clone();
    run(
        &mut e,
        "layer.create",
        json!({"name":"child","fill":[40000,40000,40000,65535]}),
    );
    let before = e.document().unwrap().export_pixels().unwrap();
    run(&mut e, "layer.ungroup", json!({"layer":{"id":group}}));
    assert_eq!(e.document().unwrap().export_pixels().unwrap(), before);
    assert_eq!(e.document().unwrap().layers().len(), 2);
    assert!(
        e.document()
            .unwrap()
            .layers()
            .iter()
            .all(|l| l.info().parent.is_none())
    );
}

#[test]
fn parent_lock_blocks_descendant_attributes_and_creation_and_legacy_versions_reject_tree() {
    let mut e = editor();
    let group = run(
        &mut e,
        "layer.group",
        json!({"layers":[{"name":"Background"}],"name":"group"}),
    )["layer_id"]
        .clone();
    run(
        &mut e,
        "layer.set_locks",
        json!({"layer":{"id":group},"locks":{"all":true}}),
    );
    let revision = e.revision();
    for (id, p) in [
        (
            "layer.rename",
            json!({"layer":{"name":"Background"},"name":"cannot"}),
        ),
        ("layer.create", json!({"name":"cannot"})),
        (
            "layer.add_adjustment",
            json!({"name":"cannot","type":"exposure"}),
        ),
    ] {
        assert!(
            e.execute(e.request(Source::User), id, p, |_| Ok(()))
                .is_err()
        );
        assert_eq!(e.revision(), revision);
    }
    let mut archive = e.document().unwrap().to_archive().unwrap();
    archive.metadata["version"] = json!(1);
    assert!(strata_core::Document::from_archive(archive).is_err());
}

#[test]
fn grouping_adjustments_defaults_to_passthrough_and_preserves_the_input_backdrop() {
    let mut e = editor();
    let mut ids = Vec::new();
    for name in ["A", "B", "C"] {
        ids.push(
            run(
                &mut e,
                "layer.add_adjustment",
                json!({"name":name,"type":"exposure","stops":0.25}),
            )["layer_id"]
                .clone(),
        );
    }
    let before = e.document().unwrap().export_pixels().unwrap();
    let group = run(
        &mut e,
        "layer.group",
        json!({"layers":ids.iter().map(|id|json!({"id":id})).collect::<Vec<_>>(),"name":"tone"}),
    )["layer_id"]
        .clone();
    assert_eq!(e.document().unwrap().export_pixels().unwrap(), before);
    run(
        &mut e,
        "layer.set_pass_through",
        json!({"layer":{"id":group},"pass_through":false}),
    );
    assert_ne!(e.document().unwrap().export_pixels().unwrap(), before);
    run(
        &mut e,
        "layer.set_pass_through",
        json!({"layer":{"id":group},"pass_through":true}),
    );
    run(&mut e, "layer.ungroup", json!({"layer":{"id":group}}));
    assert_eq!(e.document().unwrap().export_pixels().unwrap(), before);
}
