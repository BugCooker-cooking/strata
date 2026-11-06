use serde_json::{Value, json};
use strata_core::{Document, Editor, Source};
fn run(e: &mut Editor, id: &str, params: Value) -> Value {
    e.execute(e.request(Source::User), id, params, |_| Ok(()))
        .unwrap()
}
fn fixture(depth: u8) -> Editor {
    let mut e = Editor::new();
    run(
        &mut e,
        "file.new",
        json!({"width":16,"height":12,"bit_depth":depth,"background":if depth==16{[10001,20003,50001,32769]}else{[43,97,211,127]}}),
    );
    e
}
fn p(rect: Value) -> Value {
    json!({"rect":rect,"pixels":"preserve"})
}
#[test]
fn crop_keeps_all_source_and_disabled_mask_codes_and_reveal_restores_outside_content() {
    for depth in [8, 16] {
        let mut e = fixture(depth);
        run(
            &mut e,
            "select.marquee",
            json!({"x":2,"y":3,"width":10,"height":6}),
        );
        run(
            &mut e,
            "layer.mask.add",
            json!({"layer":{"name":"Background"},"source":"from_selection"}),
        );
        run(
            &mut e,
            "layer.mask.set_enabled",
            json!({"layer":{"name":"Background"},"enabled":false}),
        );
        let before = e.document().unwrap().to_archive().unwrap();
        let original = e.document().unwrap().export_pixels().unwrap();
        let history = e.snapshot().history.len();
        let answer = run(
            &mut e,
            "image.crop",
            p(json!({"x":4,"y":3,"width":8,"height":6})),
        );
        assert_eq!(answer["frame"]["x"], 4);
        assert_eq!(answer["preserved_external_content"], true);
        assert_eq!(e.snapshot().history.len(), history + 1);
        let cropped = e.document().unwrap().to_archive().unwrap();
        let codes = |a: &strata_io::project::ProjectArchive| {
            a.tiles
                .iter()
                .filter(|t| t.layer != "selection" && t.layer != "previous_selection")
                .cloned()
                .collect::<Vec<_>>()
        };
        assert_eq!(codes(&cropped), codes(&before));
        assert!(
            !e.document().unwrap().layers()[0]
                .info()
                .mask
                .as_ref()
                .unwrap()
                .enabled
        );
        assert_eq!(
            e.document().unwrap().selection().unwrap().layout().bounds.x,
            4
        );
        let reopened = Document::from_archive(cropped.clone()).unwrap();
        assert_eq!(reopened.to_archive().unwrap(), cropped);
        run(&mut e, "image.reveal_all", json!({}));
        assert_eq!(e.document().unwrap().frame().x, 0);
        assert_eq!(
            (
                e.document().unwrap().frame().width,
                e.document().unwrap().frame().height
            ),
            (16, 12)
        );
        assert_eq!(e.document().unwrap().export_pixels().unwrap(), original);
        assert_eq!(
            e.document()
                .unwrap()
                .selection()
                .unwrap()
                .sample(2, 3)
                .unwrap(),
            0
        );
        run(&mut e, "edit.undo", json!({}));
        assert_eq!(e.document().unwrap().to_archive().unwrap(), cropped);
        run(&mut e, "edit.undo", json!({}));
        assert_eq!(e.document().unwrap().to_archive().unwrap(), before);
    }
}
#[test]
fn exact_reduced_ratio_fits_inside_box_and_bad_ratios_or_delete_flags_are_atomic() {
    let mut e = fixture(16);
    let before = e.document().unwrap().to_archive().unwrap();
    let history = e.snapshot().history.len();
    let mut params = p(json!({"x":-3,"y":5,"width":13,"height":10}));
    params["aspect_ratio"] = json!([8, 6]);
    let result = run(&mut e, "image.crop", params);
    assert_eq!(
        result["frame"],
        json!({"name":"Artboard 1","x":-3,"y":5,"width":12,"height":9})
    );
    run(&mut e, "edit.undo", json!({}));
    for params in [
        json!({"rect":{"x":0,"y":0,"width":3,"height":3},"pixels":"preserve","aspect_ratio":[4,3]}),
        json!({"rect":{"x":0,"y":0,"width":10,"height":10},"pixels":"delete"}),
        json!({"rect":{"x":0,"y":0,"width":10,"height":10},"pixels":"preserve","angle":12}),
        json!({"rect":{"x":0,"y":0,"width":10,"height":10},"pixels":"preserve","aspect_ratio":[0,1]}),
    ] {
        assert!(
            e.execute(e.request(Source::User), "image.crop", params, |_| Ok(()))
                .is_err()
        );
        assert_eq!(e.document().unwrap().to_archive().unwrap(), before);
        assert_eq!(e.snapshot().history_cursor, history);
    }
}
#[test]
fn reveal_handles_nested_visible_generated_sources_styles_locks_and_negative_offsets() {
    let mut e = fixture(16);
    run(
        &mut e,
        "layer.fill.solid",
        json!({"name":"source","color":[30001,10003,50001,65535]}),
    );
    run(
        &mut e,
        "layer.set_position",
        json!({"layer":{"name":"source"},"x":-20,"y":-10}),
    );
    let group = run(
        &mut e,
        "layer.group",
        json!({"layers":[{"name":"source"}],"name":"styled group"}),
    )["layer_id"]
        .clone();
    run(
        &mut e,
        "layer.set_styles",
        json!({"layer":{"id":group},"styles":{"drop_shadow":{"color":[0,0,0,65535],"blur":2,"offset_x":5,"offset_y":-3}}}),
    );
    run(
        &mut e,
        "layer.set_locks",
        json!({"layer":{"id":group},"locks":{"all":true}}),
    );
    run(
        &mut e,
        "layer.select",
        json!({"layer":{"name":"Background"}}),
    );
    run(
        &mut e,
        "layer.fill.solid",
        json!({"name":"hidden","color":[10001,30003,50001,65535]}),
    );
    run(
        &mut e,
        "layer.set_position",
        json!({"layer":{"name":"hidden"},"x":500,"y":300}),
    );
    let hidden = run(
        &mut e,
        "layer.group",
        json!({"layers":[{"name":"hidden"}],"name":"hidden group"}),
    )["layer_id"]
        .clone();
    run(
        &mut e,
        "layer.set_visibility",
        json!({"layer":{"id":hidden},"visible":false}),
    );
    let before = e.document().unwrap().to_archive().unwrap();
    run(&mut e, "image.reveal_all", json!({}));
    let frame = e.document().unwrap().frame();
    assert_eq!((frame.x, frame.y), (-23, -21));
    assert_eq!((frame.width, frame.height), (39, 33));
    assert_eq!(
        e.document().unwrap().to_archive().unwrap().tiles,
        before.tiles
    );
    let doc = Document::from_archive(e.document().unwrap().to_archive().unwrap()).unwrap();
    assert_eq!(doc.frame(), frame);
    run(&mut e, "edit.undo", json!({}));
    assert_eq!(e.document().unwrap().to_archive().unwrap(), before);
}
#[test]
fn empty_reveal_keeps_frame_and_over_budget_extents_never_publish() {
    let mut e = Editor::new();
    run(
        &mut e,
        "file.new",
        json!({"width":8,"height":6,"bit_depth":16}),
    );
    let before = e.document().unwrap().frame().clone();
    run(&mut e, "image.reveal_all", json!({}));
    assert_eq!(e.document().unwrap().frame(), &before);
    run(
        &mut e,
        "layer.create",
        json!({"name":"far","fill":[10000,20000,30000,65535]}),
    );
    run(
        &mut e,
        "layer.set_position",
        json!({"layer":{"name":"far"},"x":200000,"y":0}),
    );
    let before = e.document().unwrap().to_archive().unwrap();
    let revision = e.revision();
    assert!(
        e.execute(
            e.request(Source::User),
            "image.reveal_all",
            json!({}),
            |_| Ok(())
        )
        .is_err()
    );
    assert_eq!(e.revision(), revision);
    assert_eq!(e.document().unwrap().to_archive().unwrap(), before);
    let request = e.request(Source::User);
    e.cancel_pending_work();
    assert!(
        e.prepare(
            request,
            "image.crop",
            p(json!({"x":0,"y":0,"width":4,"height":3}))
        )
        .is_err()
    );
}
