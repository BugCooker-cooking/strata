use serde_json::{Value, json};
use std::path::Path;
use strata_core::{Editor, Source, commands::CommandError};
use strata_pixels::PixelValue;
fn run(e: &mut Editor, id: &str, p: Value) -> Value {
    e.execute(e.request(Source::User), id, p, |_| Ok(()))
        .unwrap()
}
fn open(e: &mut Editor, p: &Path) {
    let r = e.grant_read(p, Source::User).unwrap();
    run(e, "file.open", json!({"resource":r,"discard_changes":true}));
}
fn export(e: &mut Editor, p: &Path) {
    let w = e.grant_write(p, false, Source::User).unwrap();
    run(e, "file.export_psd", json!({"resource":w}));
}
const FIXTURE: &[u8] = include_bytes!("fixtures/psd/group-mask.psd");

#[test]
fn export_16bit_grouped_masks_and_adjustments_round_trip_without_marking_saved() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input.psd");
    std::fs::write(&input, FIXTURE).unwrap();
    let mut e = Editor::new();
    open(&mut e, &input);
    run(
        &mut e,
        "layer.rename",
        json!({"layer":{"name":"细节"},"name":"图层🌱"}),
    );
    let before = e.snapshot();
    let prior = e.document().unwrap().to_archive().unwrap();
    let output = dir.path().join("roundtrip.psd");
    export(&mut e, &output);
    assert!(e.document().unwrap().to_archive().unwrap() == prior);
    assert_eq!(e.revision(), before.revision);
    assert_eq!(e.snapshot().history_cursor, before.history_cursor);
    assert!(e.dirty());
    assert!(e.project_path().is_none());
    assert_eq!(std::fs::read(input).unwrap(), FIXTURE);
    let mut copy = Editor::new();
    open(&mut copy, &output);
    let original = e.document().unwrap();
    let doc = copy.document().unwrap();
    assert_eq!(doc.layers().len(), 5);
    assert_eq!(
        (doc.working_icc(), doc.resolution_dpi(), doc.bit_depth()),
        (original.working_icc(), 300, 16)
    );
    assert_eq!(
        doc.export_pixels().unwrap(),
        original.export_pixels().unwrap()
    );
    for (a, b) in doc.layers().iter().zip(original.layers()) {
        assert_eq!(
            (
                &a.info().name,
                &a.info().kind,
                a.info().visible,
                a.info().pass_through
            ),
            (
                &b.info().name,
                &b.info().kind,
                b.info().visible,
                b.info().pass_through
            )
        );
    }
    let layer = &doc.layers()[1];
    assert_eq!(layer.info().parent, Some(doc.layers()[4].id()));
    assert_eq!((layer.info().x, layer.info().y), (-1, 1));
    assert_eq!(
        layer.pixels().pixel(0, 0).unwrap(),
        PixelValue::Rgba16([12345, 0, 45678, 0])
    );
    for y in -1..5 {
        for x in -3..6 {
            assert_eq!(
                layer.mask().unwrap().sample(x, y).unwrap(),
                original.layers()[1].mask().unwrap().sample(x, y).unwrap()
            );
        }
    }
    run(
        &mut copy,
        "adjust.exposure.set",
        json!({"layer":{"name":"Exposure"},"stops":2.0}),
    );
    run(&mut copy, "edit.undo", json!({}));
    assert_eq!(
        copy.document().unwrap().export_pixels().unwrap(),
        original.export_pixels().unwrap()
    );
}
#[test]
fn eight_bit_transparency_hidden_layers_and_strict_ai_write_grant() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("eight.psd");
    let mut e = Editor::new();
    run(
        &mut e,
        "file.new",
        json!({"width":7,"height":3,"bit_depth":8,"background":[40,90,130,96]}),
    );
    run(
        &mut e,
        "layer.create",
        json!({"name":"Hidden","fill":[22,125,64,0]}),
    );
    run(
        &mut e,
        "layer.set_visibility",
        json!({"layer":{"name":"Hidden"},"visible":false}),
    );
    let grant = e.grant_write(&p, false, Source::User).unwrap();
    let turn = e.begin_turn().unwrap();
    let source = Source::Ai { turn_id: turn };
    let invoke = |e: &mut Editor, p: Value| {
        e.execute(e.request(source.clone()), "file.export_psd", p, |_| Ok(()))
    };
    assert!(matches!(
        invoke(&mut e, json!({"resource":grant})),
        Err(CommandError::Permission)
    ));
    let token = e.grant_write(&p, false, source.clone()).unwrap();
    assert!(invoke(&mut e, json!({"resource":token,"flatten":true})).is_err());
    assert!(!p.exists());
    assert_eq!(
        invoke(&mut e, json!({"resource":token})).unwrap(),
        json!({"status":"completed","command":"file.export_psd"})
    );
    assert!(invoke(&mut e, json!({"resource":token})).is_err());
    let mut copy = Editor::new();
    open(&mut copy, &p);
    let doc = copy.document().unwrap();
    assert_eq!(doc.bit_depth(), 8);
    assert_eq!(doc.layers().len(), 2);
    assert!(!doc.layers()[1].info().visible);
    assert_eq!(
        doc.layers()[1].pixels().pixel(0, 0).unwrap(),
        PixelValue::Rgba8([22, 125, 64, 0])
    );
    assert_eq!(
        doc.layers()[0].pixels().pixel(0, 0).unwrap(),
        PixelValue::Rgba8([40, 90, 130, 96])
    );
}
#[test]
fn exports_bind_a_snapshot_and_reject_changed_targets_or_unsupported_content() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("existing.psd");
    std::fs::write(&path, b"existing file").unwrap();
    let mut e = Editor::new();
    run(
        &mut e,
        "file.new",
        json!({"width":9,"height":5,"bit_depth":8}),
    );
    let prepare = |e: &mut Editor| {
        let token = e.grant_write(&path, true, Source::User).unwrap();
        e.prepare(
            e.request(Source::User),
            "file.export_psd",
            json!({"resource":token}),
        )
        .unwrap()
    };
    let pending = prepare(&mut e);
    run(&mut e, "layer.create", json!({"name":"Changed"}));
    e.commit(pending).unwrap();
    let mut snapshot = Editor::new();
    open(&mut snapshot, &path);
    assert_eq!(e.document().unwrap().layers().len(), 2);
    assert_eq!(snapshot.document().unwrap().layers().len(), 1);
    let unchanged = std::fs::read(&path).unwrap();
    let pending = prepare(&mut e);
    run(
        &mut e,
        "file.new",
        json!({"width":2,"height":3,"bit_depth":8,"discard_changes":true}),
    );
    assert!(e.commit(pending).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), unchanged);
    let pending = prepare(&mut e);
    std::fs::write(&path, b"replacement").unwrap();
    assert!(e.commit(pending).is_err());
    run(
        &mut e,
        "type.create",
        json!({"name":"Editable","spec":{"text":"Keep","font":"sans","font_size":12,"layout":{"type":"point"}},"color":[1.0,1.0,1.0,1.0],"x":0,"y":0}),
    );
    let token = e.grant_write(&path, true, Source::User).unwrap();
    let revision = e.revision();
    let error = e
        .prepare(
            e.request(Source::User),
            "file.export_psd",
            json!({"resource":token}),
        )
        .err()
        .unwrap();
    assert!(error.to_string().contains("Editable"));
    assert_eq!(e.revision(), revision);
    assert_eq!(std::fs::read(&path).unwrap(), b"replacement");
}

#[test]
fn eight_bit_export_preserves_existing_file_when_mask_needs_sixteen_bits() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("masked.psd");
    let target = dir.path().join("previous.psd");
    std::fs::write(&input, FIXTURE).unwrap();
    std::fs::write(&target, b"keep").unwrap();
    let mut e = Editor::new();
    open(&mut e, &input);
    run(&mut e, "image.convert_bit_depth", json!({"bit_depth":8}));
    let resource = e.grant_write(&target, true, Source::User).unwrap();
    let err = e
        .execute(
            e.request(Source::User),
            "file.export_psd",
            json!({"resource":resource}),
            |_| Ok(()),
        )
        .unwrap_err();
    assert!(err.to_string().contains("16-bit document"));
    assert_eq!(std::fs::read(&target).unwrap(), b"keep");
    assert_eq!(
        e.document().unwrap().layers()[1]
            .mask()
            .unwrap()
            .sample(0, 2)
            .unwrap(),
        10001
    );
}

#[test]
fn psd_tools_path_fixture_keeps_unicode_open_segments_and_linked_handles_at_both_depths() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("independent.psd");
    std::fs::write(&source, include_bytes!("fixtures/psd/saved-path.psd")).unwrap();
    for depth in [8, 16] {
        let mut editor = Editor::new();
        open(&mut editor, &source);
        if depth == 16 {
            run(
                &mut editor,
                "image.convert_bit_depth",
                json!({"bit_depth":16}),
            );
        }
        let by_name = json!({"path":{"name":"輪廓 Café"}});
        let original = run(&mut editor, "path.get", by_name.clone());
        assert_eq!(original["open_contours"], json!([1]));
        let knots = original["subpaths"][0].as_array().unwrap();
        assert_eq!(knots.len(), 2);
        assert_eq!(knots[0]["position"], json!([0., 1.]));
        assert_eq!(knots[0]["incoming"], json!([-2., -1.]));
        assert_eq!(knots[0]["outgoing"], json!([2., 1.]));
        assert_eq!(knots[0]["linked"], true);
        run(
            &mut editor,
            "path.update",
            json!({"path":{"id":original["id"]},"name":"🌱 轮廓"}),
        );
        let output = temp.path().join(format!("roundtrip-{depth}.psd"));
        let before = editor.revision();
        export(&mut editor, &output);
        assert_eq!(before, editor.revision());
        let mut copy = Editor::new();
        open(&mut copy, &output);
        let reopened = run(&mut copy, "path.get", json!({"path":{"name":"🌱 轮廓"}}));
        assert_eq!(reopened["subpaths"], original["subpaths"]);
        assert_eq!(reopened["anchors"], original["anchors"]);
        assert_eq!(reopened["open_contours"], original["open_contours"]);
        assert_ne!(reopened["id"], original["id"]); // The PSD starts a fresh document.
    }
}

#[test]
fn unrepresentable_saved_path_fails_before_overwriting_the_target() {
    let mut e = Editor::new();
    run(
        &mut e,
        "file.new",
        json!({"width":4,"height":4,"bit_depth":8}),
    );
    run(
        &mut e,
        "path.create",
        json!({"name":"Outside","anchors":[{"position":[64,0]},{"position":[0,2]}],"open_contours":[0]}),
    );
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("old.psd");
    std::fs::write(&dest, b"previous").unwrap();
    let token = e.grant_write(&dest, true, Source::User).unwrap();
    let before = e.revision();
    assert!(
        e.execute(
            e.request(Source::User),
            "file.export_psd",
            json!({"resource":token}),
            |_| Ok(())
        )
        .is_err()
    );
    assert_eq!(std::fs::read(dest).unwrap(), b"previous");
    assert_eq!(e.revision(), before);
}

#[test]
fn path_coordinates_follow_the_exported_artboard_origin_and_fixed_point_precision() {
    let mut editor = Editor::new();
    run(
        &mut editor,
        "file.new",
        json!({"width":40,"height":35,"bit_depth":8}),
    );
    run(
        &mut editor,
        "path.create",
        json!({"name":"Offset","open_contours":[0],"anchors":[
        {"position":[2.123456,7.345678],"outgoing":[1.23456,-2.34567]},
        {"position":[23.765432,25.123456],"incoming":[-4.12345,2.34567]}]}),
    );
    run(
        &mut editor,
        "image.crop",
        json!({"rect":{"x":5,"y":3,"width":27,"height":17},"pixels":"preserve"}),
    );
    let source = run(&mut editor, "path.get", json!({"path":{"name":"Offset"}}));
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("crop.psd");
    export(&mut editor, &dest);
    let mut restored = Editor::new();
    open(&mut restored, &dest);
    let target = run(&mut restored, "path.get", json!({"path":{"name":"Offset"}}));
    for (a, b) in source["anchors"]
        .as_array()
        .unwrap()
        .iter()
        .zip(target["anchors"].as_array().unwrap())
    {
        for k in 0..2 {
            let origin = [5.0, 3.0][k];
            let half_unit = [27.0, 17.0][k] / 33554432.;
            for control in ["position", "incoming", "outgoing"] {
                let absolute = |node: &Value| {
                    node["position"][k].as_f64().unwrap()
                        + if control == "position" {
                            0.0
                        } else {
                            node[control][k].as_f64().unwrap()
                        }
                };
                assert!((absolute(b) - (absolute(a) - origin)).abs() <= half_unit + 1e-13);
            }
        }
    }
}
