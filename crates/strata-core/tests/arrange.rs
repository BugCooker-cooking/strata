use serde_json::{Value, json};
use strata_core::{Editor, Source};
fn run(e: &mut Editor, id: &str, p: Value) -> Value {
    e.execute(e.request(Source::User), id, p, |_| Ok(()))
        .unwrap()
}
#[test]
fn align_and_distribute_use_sibling_geometry_and_undo_exactly() {
    let mut e = Editor::new();
    run(
        &mut e,
        "file.new",
        json!({"width":10,"height":10,"bit_depth":16}),
    );
    for (name, x, y) in [("a", 0, 1), ("b", 20, 5), ("c", 70, 9)] {
        run(
            &mut e,
            "layer.fill.solid",
            json!({"name":name,"color":[1,2,3,65535]}),
        );
        run(
            &mut e,
            "layer.set_position",
            json!({"layer":{"name":name},"x":x,"y":y}),
        );
    }
    let before = e.document().unwrap().to_archive().unwrap();
    let targets = json!([{"name":"a"},{"name":"b"},{"name":"c"}]);
    run(
        &mut e,
        "layer.align",
        json!({"layers":targets,"mode":"top"}),
    );
    assert!(
        e.document().unwrap().layers()[1..]
            .iter()
            .all(|l| l.info().y == 1)
    );
    run(
        &mut e,
        "layer.distribute",
        json!({"layers":targets,"mode":"horizontal_centers"}),
    );
    assert_eq!(e.document().unwrap().layers()[2].info().x, 35);
    run(&mut e, "edit.undo", json!({}));
    run(&mut e, "edit.undo", json!({}));
    assert_eq!(e.document().unwrap().to_archive().unwrap(), before);
}
#[test]
fn arranging_locked_or_overlapping_tree_targets_is_atomic() {
    let mut e = Editor::new();
    run(
        &mut e,
        "file.new",
        json!({"width":10,"height":10,"bit_depth":16,"background":[1,2,3,65535]}),
    );
    let group = run(
        &mut e,
        "layer.group",
        json!({"layers":[{"name":"Background"}],"name":"group"}),
    )["layer_id"]
        .clone();
    let before = e.revision();
    assert!(
        e.execute(
            e.request(Source::User),
            "layer.align",
            json!({"layers":[{"name":"Background"},{"id":group}],"mode":"left"}),
            |_| Ok(())
        )
        .is_err()
    );
    assert_eq!(e.revision(), before);
    run(
        &mut e,
        "layer.set_locks",
        json!({"layer":{"name":"Background"},"locks":{"position":true}}),
    );
    let before = e.document().unwrap().to_archive().unwrap();
    assert!(
        e.execute(
            e.request(Source::User),
            "layer.align",
            json!({"layers":[{"id":group}],"mode":"right","reference":"artboard"}),
            |_| Ok(())
        )
        .is_err()
    );
    assert_eq!(e.document().unwrap().to_archive().unwrap(), before);
}
