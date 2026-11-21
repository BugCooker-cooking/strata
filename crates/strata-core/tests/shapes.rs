use serde_json::{Value, json};
use strata_core::{Document, Editor, Source};
use strata_pixels::PixelBuffer;

fn run(e: &mut Editor, id: &str, p: Value) -> Value {
    e.execute(e.request(Source::User), id, p, |_| Ok(()))
        .unwrap()
}
fn setup(depth: u8) -> Editor {
    let mut e = Editor::new();
    run(
        &mut e,
        "file.new",
        json!({"width":32,"height":32,"bit_depth":depth}),
    );
    e
}
fn geometry(kind: Value) -> Value {
    json!({"start":[4,4],"end":[24,24],"primitive":kind})
}
fn alpha(doc: &Document, x: usize, y: usize) -> u16 {
    let i = 4 * (y * doc.frame().width as usize + x) + 3;
    match doc.export_pixels().unwrap() {
        PixelBuffer::Rgba8(v) => u16::from(v[i]) * 257,
        PixelBuffer::Rgba16(v) => v[i],
    }
}

#[test]
fn every_primitive_matches_the_preview_and_creates_one_undoable_vector_layer() {
    for (kind, n) in [
        (json!({"kind":"rectangle"}), 4),
        (json!({"kind":"rectangle","radius":3}), 8),
        (json!({"kind":"ellipse"}), 4),
        (json!({"kind":"triangle"}), 3),
        (json!({"kind":"polygon","sides":7}), 7),
        (json!({"kind":"line","weight":4}), 4),
    ] {
        let mut e = setup(16);
        run(
            &mut e,
            "select.marquee",
            json!({"x":0,"y":0,"width":2,"height":2}),
        );
        let before = e.document().unwrap().to_archive().unwrap();
        let history = e.snapshot().history_cursor;
        let g = geometry(kind.clone());
        let preview = run(&mut e, "shape.outline", json!({"geometry":g}));
        let result = run(
            &mut e,
            "shape.create",
            json!({"name":"Object","geometry":g,"color":[12345,32769,55000,65535]}),
        );
        let stored = run(&mut e, "path.get", json!({"path":{"id":result["path_id"]}}));
        assert_eq!(stored["anchors"], preview["anchors"]);
        assert_eq!(stored["anchors"].as_array().unwrap().len(), n);
        assert_eq!(e.snapshot().history_cursor, history + 1);
        let doc = e.document().unwrap();
        assert_eq!(
            doc.layers()
                .last()
                .unwrap()
                .info()
                .vector_mask
                .as_ref()
                .unwrap()
                .path,
            doc.paths()[0].id
        );
        assert_eq!(alpha(doc, 14, 14), 65535, "{kind}");
        assert_eq!(alpha(doc, 28, 28), 0);
        if kind["kind"] == "ellipse" {
            assert_eq!(alpha(doc, 4, 4), 0);
            assert_eq!(stored["anchors"][0]["position"], json!([14., 4.]));
        }
        if kind["kind"] == "line" {
            assert_eq!(alpha(doc, 5, 20), 0);
        }
        assert_eq!(doc.selection().unwrap().sample(0, 0).unwrap(), 65535);
        run(&mut e, "edit.undo", json!({}));
        assert!(before == e.document().unwrap().to_archive().unwrap());
        run(&mut e, "edit.redo", json!({}));
        assert_eq!(e.document().unwrap().paths().len(), 1);
    }
}

#[test]
fn numeric_reedit_recolor_and_independent_duplicate_survive_reopening_and_transform() {
    let mut e = setup(8);
    let original = geometry(json!({"kind":"ellipse"}));
    let r = run(
        &mut e,
        "shape.create",
        json!({"name":"Logo","color":[255,0,0,255],"geometry":original}),
    );
    let id = json!({"id":r["layer_id"]});
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("design.strata");
    let w = e.grant_write(&p, false, Source::User).unwrap();
    run(&mut e, "file.save_as", json!({"resource":w}));
    let read = e.grant_read(&p, Source::User).unwrap();
    run(&mut e, "file.open", json!({"resource":read}));
    let rev = e.revision();
    let no = run(
        &mut e,
        "shape.update",
        json!({"layer":id,"geometry":original}),
    );
    assert_eq!(no["changed"], false);
    assert_eq!(e.revision(), rev);
    let dup = run(&mut e, "layer.duplicate", json!({"layer":id}));
    let other = json!({"id":dup["layer_id"]});
    let copy = run(
        &mut e,
        "shape.update",
        json!({"layer":other,"color":[10,220,125,255],"geometry":geometry(json!({"kind":"rectangle","radius":6}))}),
    );
    assert_ne!(r["path_id"], copy["path_id"]);
    assert_eq!(e.document().unwrap().paths().len(), 2);
    let before = run(&mut e, "path.get", json!({"path":{"id":r["path_id"]}}));
    assert_eq!(before["shape"]["geometry"]["primitive"]["kind"], "ellipse");
    run(&mut e, "layer.delete", json!({"layer":other}));
    assert_eq!(e.document().unwrap().paths().len(), 1);
    run(
        &mut e,
        "image.resize",
        json!({"width":64,"height":48,"interpolation":"nearest"}),
    );
    run(
        &mut e,
        "shape.update",
        json!({"layer":id,"geometry":geometry(json!({"kind":"triangle"}))}),
    );
    let result = run(&mut e, "path.get", json!({"path":{"id":r["path_id"]}}));
    assert_eq!(result["anchors"][0]["position"], json!([28., 6.]));
    assert_eq!(alpha(e.document().unwrap(), 28, 24), 65535);
    run(
        &mut e,
        "path.split",
        json!({"path":{"id":r["path_id"]},"segment":1,"t":0.5}),
    );
    let free = run(&mut e, "path.get", json!({"path":{"id":r["path_id"]}}));
    assert!(free.get("shape").is_none());
    run(&mut e, "edit.undo", json!({}));
    let edit = run(&mut e, "path.get", json!({"path":{"id":r["path_id"]}}));
    assert_eq!(edit, result);
}

#[test]
fn reverse_drag_clamped_radius_and_invalid_commands_preserve_document_state() {
    let mut e = setup(8);
    let before = e.document().unwrap().to_archive().unwrap();
    let rev = e.revision();
    let rounded = run(
        &mut e,
        "shape.outline",
        json!({"geometry":{"start":[24,24],"end":[4,4],"primitive":{"kind":"rectangle","radius":100}}}),
    );
    assert_eq!(rounded["anchors"].as_array().unwrap().len(), 4);
    let valid = geometry(json!({"kind":"ellipse"}));
    for invalid in [
        json!({"name":"X","color":[0,0,0,255],"geometry":geometry(json!({"kind":"polygon","sides":2}))}),
        json!({"name":"X","color":[0,0,0,255],"geometry":geometry(json!({"kind":"line","weight":0}))}),
        json!({"name":"X","color":[0,0,0,255],"geometry":geometry(json!({"kind":"ellipse","radius":5}))}),
        json!({"name":"X","color":[0,4000,0,65535],"geometry":valid}),
        json!({"name":"X","color":[0,0,0,255],"geometry":valid,"expected_revision":rev+4}),
        json!({"name":"X","color":[0,0,0,255],"geometry":{"start":[1,2],"end":[1,3],"primitive":{"kind":"rectangle"}}}),
    ] {
        assert!(
            e.execute(
                e.request(Source::User),
                "shape.create",
                invalid.clone(),
                |_| Ok(())
            )
            .is_err(),
            "{invalid}"
        );
    }
    assert_eq!(e.revision(), rev);
    assert!(before == e.document().unwrap().to_archive().unwrap());
}

#[test]
fn shape_can_extend_beyond_original_artboard_but_destructive_crop_is_retained() {
    let mut e = setup(16);
    let g = json!({"start":[-5,0],"end":[9,12],"primitive":{"kind":"rectangle"}});
    let r = run(
        &mut e,
        "shape.create",
        json!({"geometry":g,"name":"Outside","color":[65535,0,0,65535]}),
    );
    let id = json!({"id":r["layer_id"]});
    let bounds = e
        .document()
        .unwrap()
        .generated_canvas_bounds(e.document().unwrap().layers().last().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(bounds.x, -5);
    run(
        &mut e,
        "image.crop",
        json!({"rect":{"x":2,"y":1,"width":6,"height":8},"pixels":"delete"}),
    );
    run(
        &mut e,
        "shape.update",
        json!({"layer":id,"geometry":{"start":[-10,-10],"end":[32,32],"primitive":{"kind":"rectangle"}}}),
    );
    run(
        &mut e,
        "image.canvas_size",
        json!({"width":30,"height":24,"anchor":"top_left"}),
    );
    assert_eq!(alpha(e.document().unwrap(), 2, 4), 65535);
    assert_eq!(alpha(e.document().unwrap(), 8, 4), 0);
}

#[test]
fn layered_psd_keeps_standard_shape_and_revalidates_numeric_parameters_at_both_depths() {
    let dir = tempfile::tempdir().unwrap();
    for depth in [8u8, 16] {
        let max = if depth == 8 { 255 } else { 65535 };
        let color = if depth == 8 {
            [123, 50, 71, 128]
        } else {
            [12345, 1234, 3243, 32769]
        };
        let mut e = setup(depth);
        let g = json!({"start":[3.25,2.125],"end":[24.75,22.5],"primitive":{"kind":"rectangle","radius":4.25}});
        let result = run(
            &mut e,
            "shape.create",
            json!({"geometry":g,"name":"圆角図形","color":color}),
        );
        run(
            &mut e,
            "layer.set_fill_opacity",
            json!({"layer":{"id":result["layer_id"]},"fill_opacity":0.8}),
        );
        let before = e.document().unwrap().export_pixels().unwrap();
        let f = dir.path().join(format!("shape-{depth}.psd"));
        let write = e.grant_write(&f, false, Source::User).unwrap();
        run(&mut e, "file.export_psd", json!({"resource":write}));
        let mut copy = Editor::new();
        let read = copy.grant_read(&f, Source::User).unwrap();
        run(&mut copy, "file.open", json!({"resource":read}));
        let doc = copy.document().unwrap();
        assert!(
            matches!(doc.layers()[1].info().kind,strata_core::LayerKind::Solid{color:c} if c==color)
        );
        assert_eq!(doc.layers()[1].info().fill_opacity, 0.8);
        assert_eq!(doc.paths().len(), 1);
        assert_eq!(
            doc.paths()[0].shape.as_ref().unwrap().geometry.primitive,
            strata_core::ShapeKind::Rectangle { radius: 4.25 }
        );
        assert_eq!(doc.export_pixels().unwrap(), before);
        run(
            &mut copy,
            "shape.update",
            json!({"layer":{"name":"圆角図形"},"geometry":geometry(json!({"kind":"triangle"})),"color":[max,0,0,max]}),
        );
        assert_eq!(copy.document().unwrap().paths()[0].anchors.len(), 3);
        // The hint must not override another editor's changed standard fill opacity.
        let mut bytes = std::fs::read(&f).unwrap();
        let i = bytes.windows(4).rposition(|w| w == b"iOpa").unwrap();
        bytes[i + 8] = 17;
        std::fs::write(&f, bytes).unwrap();
        let read = copy.grant_read(&f, Source::User).unwrap();
        run(
            &mut copy,
            "file.open",
            json!({"resource":read,"discard_changes":true}),
        );
        let info = copy.document().unwrap().layers()[1].info();
        assert_eq!(info.fill_opacity, 17. / 255.);
        assert!(matches!(info.kind,strata_core::LayerKind::Solid{color:c} if c[3]==max));
    }
}

#[test]
fn removed_shape_binding_keeps_independent_path_and_manifest_verifies_source_geometry() {
    let mut e = setup(8);
    let result = run(
        &mut e,
        "shape.create",
        json!({"name":"Polygon","geometry":geometry(json!({"kind":"polygon","sides":7})),"color":[20,40,60,240]}),
    );
    let mut archive = e.document().unwrap().to_archive().unwrap();
    let x = archive.metadata["paths"][0]["anchors"][0]["position"][0]
        .as_f64()
        .unwrap();
    archive.metadata["paths"][0]["anchors"][0]["position"][0] = json!(x + 2e-13);
    assert!(Document::from_archive(archive.clone()).is_ok());
    archive.metadata["paths"][0]["anchors"][0]["position"][0] = json!(x + 0.01);
    assert!(Document::from_archive(archive).is_err());
    run(
        &mut e,
        "layer.vector_mask.remove",
        json!({"layer":{"id":result["layer_id"]}}),
    );
    run(
        &mut e,
        "layer.delete",
        json!({"layer":{"id":result["layer_id"]}}),
    );
    assert_eq!(e.document().unwrap().paths().len(), 1);
}
