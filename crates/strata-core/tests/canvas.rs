use serde_json::{Value, json};
use strata_core::{Document, Editor, Source};
use strata_pixels::TileCoord;
fn run(e: &mut Editor, id: &str, params: Value) -> Value {
    e.execute(e.request(Source::User), id, params, |_| Ok(()))
        .unwrap()
}
fn fixture(depth: u8) -> Editor {
    let mut e = Editor::new();
    let color = if depth == 16 {
        [10001, 20003, 50001, 32769]
    } else {
        [43, 97, 211, 127]
    };
    run(
        &mut e,
        "file.new",
        json!({"width":16,"height":12,"bit_depth":depth,"working_space":"display_p3","background":color}),
    );
    e
}
#[test]
fn canvas_resize_keeps_locked_nested_sources_masks_and_outside_codes_and_undoes_exactly() {
    for depth in [8, 16] {
        let mut e = fixture(depth);
        run(&mut e, "select.all", json!({}));
        run(&mut e, "select.none", json!({}));
        run(
            &mut e,
            "select.marquee",
            json!({"x":2,"y":3,"width":10,"height":6}),
        );
        let layer = e.snapshot().active_layer;
        run(
            &mut e,
            "layer.mask.add",
            json!({"layer":{"id":layer},"source":"from_selection"}),
        );
        run(
            &mut e,
            "layer.mask.set_enabled",
            json!({"layer":{"id":layer},"enabled":false}),
        );
        let group = run(
            &mut e,
            "layer.group",
            json!({"layers":[{"id":layer}],"name":"locked group"}),
        )["layer_id"]
            .clone();
        run(
            &mut e,
            "layer.set_locks",
            json!({"layer":{"id":group},"locks":{"all":true}}),
        );
        let before = e.document().unwrap().to_archive().unwrap();
        let pixel = e
            .document()
            .unwrap()
            .layers()
            .iter()
            .find(|l| Some(l.id()) == layer)
            .unwrap();
        let version = pixel.pixels().tile_version(TileCoord::new(0, 0).unwrap());
        let outside = pixel.pixels().pixel(15, 11).unwrap();
        let old_info = pixel.info().clone();
        let history = e.snapshot().history.len();
        let answer = run(
            &mut e,
            "image.canvas_size",
            json!({"width":8,"height":6,"anchor":"top_left"}),
        );
        assert_eq!(answer["preserved_external_content"], true);
        assert_eq!(e.snapshot().history.len(), history + 1);
        let doc = e.document().unwrap();
        let pixel = doc.layers().iter().find(|l| Some(l.id()) == layer).unwrap();
        assert_eq!(pixel.info(), &old_info);
        assert_eq!(pixel.pixels().pixel(15, 11).unwrap(), outside);
        assert_eq!(
            pixel.pixels().tile_version(TileCoord::new(0, 0).unwrap()),
            version
        );
        assert!(!pixel.info().mask.as_ref().unwrap().enabled);
        assert_eq!(doc.selection().unwrap().layout().bounds.width, 8);
        assert_eq!(doc.selection().unwrap().sample(7, 5).unwrap(), 65535);
        assert_eq!(doc.selection().unwrap().sample(8, 5).unwrap(), 0);
        let archive = doc.to_archive().unwrap();
        let content = |a: &strata_io::project::ProjectArchive| {
            a.tiles
                .iter()
                .filter(|t| t.layer != "selection" && t.layer != "previous_selection")
                .cloned()
                .collect::<Vec<_>>()
        };
        assert_eq!(content(&before), content(&archive));
        assert_eq!(
            Document::from_archive(archive.clone())
                .unwrap()
                .to_archive()
                .unwrap(),
            archive
        );
        run(
            &mut e,
            "image.canvas_size",
            json!({"width":20,"height":18,"anchor":"top_left"}),
        );
        assert_eq!(
            e.document()
                .unwrap()
                .selection()
                .unwrap()
                .sample(9, 5)
                .unwrap(),
            0
        );
        run(&mut e, "select.reselect", json!({}));
        assert_eq!(
            e.document()
                .unwrap()
                .selection()
                .unwrap()
                .layout()
                .bounds
                .width,
            20
        );
        for _ in 0..3 {
            run(&mut e, "edit.undo", json!({}));
        }
        assert_eq!(e.document().unwrap().to_archive().unwrap(), before);
    }
}
#[test]
fn all_nine_anchors_have_explicit_integer_canvas_origins() {
    for (anchor, x, y) in [
        ("top_left", 0, 0),
        ("top", -2, 0),
        ("top_right", -3, 0),
        ("left", 0, -2),
        ("center", -2, -2),
        ("right", -3, -2),
        ("bottom_left", 0, -3),
        ("bottom", -2, -3),
        ("bottom_right", -3, -3),
    ] {
        let mut e = fixture(16);
        run(
            &mut e,
            "image.canvas_size",
            json!({"width":19,"height":15,"anchor":anchor}),
        );
        let f = e.document().unwrap().frame();
        assert_eq!((f.x, f.y), (x, y), "{anchor}");
        assert_eq!(
            (
                e.document().unwrap().layers()[0].info().x,
                e.document().unwrap().layers()[0].info().y
            ),
            (0, 0)
        );
    }
}
#[test]
fn legacy_gradient_style_and_spatial_references_freeze_and_survive_manifest_v5() {
    let mut e = fixture(16);
    run(
        &mut e,
        "layer.set_styles",
        json!({"layer":{"name":"Background"},"styles":{"gradient_overlay":{"start":[1000,2000,3000,65535],"end":[60000,62000,63000,65535],"angle":0,"scale":1,"opacity":0.7}}}),
    );
    run(
        &mut e,
        "layer.fill.gradient",
        json!({"name":"legacy fill","start":[1000,3000,5000,18000],"end":[50000,45000,40000,18000],"angle":35,"scale":1}),
    );
    run(
        &mut e,
        "layer.add_tonal_adjustment",
        json!({"name":"local tone","adjustment":{"type":"shadows_highlights","shadows":80,"highlights":20,"radius":8}}),
    );
    let mut legacy = e.document().unwrap().to_archive().unwrap();
    legacy.metadata["version"] = json!(4);
    for info in legacy.metadata["layers"].as_array_mut().unwrap() {
        let map = info.as_object_mut().unwrap();
        map.remove("spatial_bounds");
        map.remove("generated_size");
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("legacy-v4.strata");
    strata_io::project::write_project(&path, &legacy, false, false).unwrap();
    let resource = e.grant_read(&path, Source::User).unwrap();
    run(
        &mut e,
        "file.open",
        json!({"resource":resource,"discard_changes":true}),
    );
    let before = e.document().unwrap().to_archive().unwrap();
    let coord = TileCoord::new(0, 0).unwrap();
    let mut compositor = strata_core::composite::Compositor::new(2);
    let reference = compositor.tile(e.document().unwrap(), coord).unwrap();
    run(
        &mut e,
        "image.canvas_size",
        json!({"width":24,"height":20,"anchor":"top_left"}),
    );
    let doc = e.document().unwrap();
    assert_eq!(&compositor.tile(doc, coord).unwrap()[..], &reference[..]);
    assert_eq!(doc.layers()[0].info().generated_size, Some([16, 12]));
    assert_eq!(doc.layers()[1].info().generated_size, Some([16, 12]));
    assert_eq!(
        doc.layers()[2].info().spatial_bounds.unwrap(),
        strata_pixels::CoverageRect {
            x: 0,
            y: 0,
            width: 16,
            height: 12
        }
    );
    let archive = doc.to_archive().unwrap();
    assert_eq!(archive.metadata["version"], 5);
    let reopened = Document::from_archive(archive.clone()).unwrap();
    assert_eq!(
        &compositor.tile(&reopened, coord).unwrap()[..],
        &reference[..]
    );
    let mut wrong_version = archive;
    wrong_version.metadata["version"] = json!(4);
    assert!(Document::from_archive(wrong_version).is_err());
    run(&mut e, "edit.undo", json!({}));
    assert_eq!(e.document().unwrap().to_archive().unwrap(), before);
}
#[test]
fn invalid_canvas_parameters_and_revoked_candidates_leave_frame_and_history_unchanged() {
    let mut e = fixture(16);
    let before = e.document().unwrap().to_archive().unwrap();
    let history = e.snapshot().history.len();
    for params in [
        json!({"width":0,"height":1}),
        json!({"width":100001,"height":1}),
        json!({"width":100000,"height":100000}),
        json!({"width":20,"height":20,"anchor":"guess"}),
        json!({"width":20,"height":20,"delete_pixels":true}),
    ] {
        assert!(
            e.execute(
                e.request(Source::User),
                "image.canvas_size",
                params,
                |_| Ok(())
            )
            .is_err()
        );
        assert_eq!(e.document().unwrap().to_archive().unwrap(), before);
        assert_eq!(e.snapshot().history.len(), history);
    }
    let prepared = e
        .prepare(
            e.request(Source::User),
            "image.canvas_size",
            json!({"width":32,"height":24}),
        )
        .unwrap();
    e.cancel_pending_work();
    assert!(e.commit(prepared).is_err());
    assert_eq!(e.document().unwrap().to_archive().unwrap(), before);
    assert_eq!(e.snapshot().history.len(), history);
}
