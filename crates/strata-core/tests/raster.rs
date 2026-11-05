use serde_json::{Value, json};
use strata_core::{Editor, LayerKind, Source};
use strata_pixels::PixelBuffer;
fn run(e: &mut Editor, id: &str, p: Value) -> Value {
    e.execute(e.request(Source::User), id, p, |_| Ok(()))
        .unwrap()
}
fn editor() -> Editor {
    let mut e = Editor::new();
    run(
        &mut e,
        "file.new",
        json!({"width":8,"height":8,"bit_depth":16}),
    );
    e
}
fn close(a: &PixelBuffer, b: &PixelBuffer) {
    match (a, b) {
        (PixelBuffer::Rgba16(a), PixelBuffer::Rgba16(b)) => {
            assert!(a.iter().zip(b).all(|(a, b)| a.abs_diff(*b) <= 3))
        }
        _ => assert_eq!(a, b),
    }
}
#[test]
fn rasterizing_generated_and_group_layers_preserves_editable_history_and_outside_pixels() {
    let mut e = editor();
    let fill = run(
        &mut e,
        "layer.fill.gradient",
        json!({"name":"Gradient","start":[0,0,0,65535],"end":[65535,65535,65535,65535]}),
    )["layer_id"]
        .clone();
    run(
        &mut e,
        "layer.set_position",
        json!({"layer":{"id":fill},"x":-3,"y":-2}),
    );
    let before = e.document().unwrap().to_archive().unwrap();
    let pixels = e.document().unwrap().export_pixels().unwrap();
    run(&mut e, "layer.rasterize", json!({"layer":{"id":fill}}));
    assert!(matches!(
        e.document().unwrap().layers().last().unwrap().info().kind,
        LayerKind::Pixel
    ));
    assert_eq!(e.document().unwrap().layers().last().unwrap().info().x, -3);
    close(&e.document().unwrap().export_pixels().unwrap(), &pixels);
    run(&mut e, "edit.undo", json!({}));
    assert_eq!(e.document().unwrap().to_archive().unwrap(), before);
    let group = run(
        &mut e,
        "layer.group",
        json!({"layers":[{"id":fill}],"name":"group"}),
    )["layer_id"]
        .clone();
    run(&mut e, "layer.rasterize", json!({"layer":{"id":group}}));
    close(&e.document().unwrap().export_pixels().unwrap(), &pixels);
}
#[test]
fn applied_pixel_mask_changes_only_alpha_and_is_exactly_undoable() {
    let mut e = editor();
    run(
        &mut e,
        "layer.create",
        json!({"name":"color","fill":[32768,32769,50001,65535]}),
    );
    run(
        &mut e,
        "select.marquee",
        json!({"x":0,"y":0,"width":4,"height":8}),
    );
    run(
        &mut e,
        "layer.mask.add",
        json!({"layer":{"name":"color"},"source":"from_selection"}),
    );
    let before = e.document().unwrap().to_archive().unwrap();
    let pixels = e.document().unwrap().export_pixels().unwrap();
    run(
        &mut e,
        "layer.mask.apply",
        json!({"layer":{"name":"color"}}),
    );
    close(&e.document().unwrap().export_pixels().unwrap(), &pixels);
    assert_eq!(
        e.document()
            .unwrap()
            .layers()
            .last()
            .unwrap()
            .pixels()
            .pixel(7, 0)
            .unwrap(),
        strata_pixels::PixelValue::Rgba16([32768, 32769, 50001, 0])
    );
    run(&mut e, "edit.undo", json!({}));
    assert_eq!(e.document().unwrap().to_archive().unwrap(), before);
}
#[test]
fn merge_variants_keep_composite_and_have_distinct_source_retention() {
    for id in [
        "layer.merge_down",
        "layer.merge_visible",
        "layer.flatten",
        "layer.stamp_visible",
    ] {
        let mut e = editor();
        run(
            &mut e,
            "layer.create",
            json!({"name":"Lower","fill":[10000,20000,30000,65535]}),
        );
        run(
            &mut e,
            "layer.create",
            json!({"name":"Upper","fill":[40000,30000,20000,32768]}),
        );
        let before = e.document().unwrap().to_archive().unwrap();
        let pixels = e.document().unwrap().export_pixels().unwrap();
        let count = e.document().unwrap().layers().len();
        run(
            &mut e,
            id,
            if id == "layer.merge_down" {
                json!({"layer":{"name":"Upper"}})
            } else {
                json!({})
            },
        );
        let after = e.document().unwrap().layers().len();
        if id == "layer.stamp_visible" {
            assert_eq!(after, count + 1);
        } else {
            close(&e.document().unwrap().export_pixels().unwrap(), &pixels);
            assert_eq!(
                after,
                if id == "layer.merge_down" {
                    count - 1
                } else {
                    1
                }
            );
        }
        run(&mut e, "edit.undo", json!({}));
        assert_eq!(e.document().unwrap().to_archive().unwrap(), before);
    }
}

#[test]
fn adjustment_rasterization_requires_explicit_input_bake_and_preserves_render() {
    let mut e = editor();
    run(
        &mut e,
        "layer.create",
        json!({"name":"photo","fill":[20000,20000,20000,65535]}),
    );
    let adjustment = run(
        &mut e,
        "layer.add_adjustment",
        json!({"name":"exposure","type":"exposure","stops":0.5}),
    )["layer_id"]
        .clone();
    let before = e.document().unwrap().to_archive().unwrap();
    let output = e.document().unwrap().export_pixels().unwrap();
    assert!(
        e.execute(
            e.request(Source::User),
            "layer.rasterize",
            json!({"layer":{"id":adjustment}}),
            |_| Ok(())
        )
        .is_err()
    );
    assert_eq!(e.document().unwrap().to_archive().unwrap(), before);
    run(
        &mut e,
        "layer.rasterize",
        json!({"layer":{"id":adjustment},"bake_backdrop":true}),
    );
    assert_eq!(e.document().unwrap().layers().len(), 1);
    assert!(matches!(
        e.document().unwrap().layers()[0].info().kind,
        LayerKind::Pixel
    ));
    close(&e.document().unwrap().export_pixels().unwrap(), &output);
    run(&mut e, "edit.undo", json!({}));
    assert_eq!(e.document().unwrap().to_archive().unwrap(), before);
}
