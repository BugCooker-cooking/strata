use serde_json::{Value, json};
use strata_core::{Document, Editor, Source, commands::CommandError};
fn run(e: &mut Editor, id: &str, p: Value) -> Value {
    e.execute(e.request(Source::User), id, p, |_| Ok(()))
        .unwrap()
}
fn new() -> Editor {
    let mut e = Editor::new();
    run(
        &mut e,
        "file.new",
        json!({"width":20,"height":20,"bit_depth":16}),
    );
    e
}
fn create(e: &mut Editor) -> Value {
    run(e,"path.create",json!({"name":"Outline","anchors":[{"position":[2,2],"outgoing":[4,-2]},{"position":[14,2],"incoming":[-4,-2]},{"position":[14,14]},{"position":[2,14]}]}))["path_id"].clone()
}

#[test]
fn path_samples_selection_and_masked_fill_with_editable_source_and_exact_undo() {
    let mut e = new();
    let prior = e.document().unwrap().export_pixels().unwrap();
    let id = create(&mut e);
    let target = json!({"id":id});
    let path = run(&mut e, "path.get", json!({"path":target}));
    assert_eq!(e.document().unwrap().export_pixels().unwrap(), prior);
    run(&mut e, "select.from_path", json!({"path":target}));
    let mask = e.document().unwrap().selection().unwrap().clone();
    assert_eq!(mask.sample(8, 8).unwrap(), 65535);
    assert_eq!(mask.sample(16, 8).unwrap(), 0);
    assert!(mask.sample(8, 1).unwrap() > 0);
    let history = e.snapshot().history_cursor;
    run(
        &mut e,
        "layer.fill.path",
        json!({"path":target,"name":"Fill","color":[12345,23456,34567,65535]}),
    );
    let doc = e.document().unwrap();
    assert_eq!(doc.layers().len(), 2);
    let fill = doc.layers().last().unwrap();
    assert_eq!(
        fill.mask().unwrap().sample(8, 1).unwrap(),
        mask.sample(8, 1).unwrap()
    );
    assert_eq!(
        doc.selection().unwrap().sample(8, 1).unwrap(),
        mask.sample(8, 1).unwrap()
    );
    assert_ne!(doc.export_pixels().unwrap(), prior);
    assert_eq!(e.snapshot().history_cursor, history + 1);
    run(&mut e, "edit.undo", json!({}));
    assert_eq!(e.document().unwrap().export_pixels().unwrap(), prior);
    assert_eq!(run(&mut e, "path.get", json!({"path":target})), path);
    let mut anchors = path["anchors"].clone();
    anchors[0]["position"] = json!([5, 6]);
    run(
        &mut e,
        "path.update",
        json!({"path":target,"anchors":anchors,"name":"Moved"}),
    );
    let moved = run(&mut e, "path.get", json!({"path":target}));
    assert_eq!(moved["anchors"][0]["outgoing"], json!([4.0, -2.0]));
    let rev = e.revision();
    let r = run(
        &mut e,
        "path.update",
        json!({"path":target,"anchors":anchors}),
    );
    assert_eq!(r["changed"], false);
    assert_eq!(e.revision(), rev);
    run(&mut e, "path.delete", json!({"path":target}));
    assert!(e.document().unwrap().paths().is_empty());
    run(&mut e, "edit.undo", json!({}));
    assert_eq!(run(&mut e, "path.get", json!({"path":target})), moved);
}
#[test]
fn project_roundtrip_version_bounds_and_image_geometry() {
    let mut e = new();
    let id = create(&mut e);
    let get = json!({"path":{"id":id}});
    let original = run(&mut e, "path.get", get.clone());
    let mut archive = e.document().unwrap().to_archive().unwrap();
    assert_eq!(archive.metadata["version"], 11);
    archive.metadata["version"] = json!(10);
    assert!(Document::from_archive(archive).is_err());
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("retained.strata");
    let write = e.grant_write(&path, false, Source::User).unwrap();
    run(&mut e, "file.save_as", json!({"resource":write}));
    let read = e.grant_read(&path, Source::User).unwrap();
    run(&mut e, "file.open", json!({"resource":read}));
    assert_eq!(run(&mut e, "path.get", get.clone()), original);
    run(
        &mut e,
        "image.resize",
        json!({"width":40,"height":30,"interpolation":"nearest"}),
    );
    let scaled = run(&mut e, "path.get", get.clone());
    assert_eq!(scaled["anchors"][0]["position"], json!([4.0, 3.0]));
    assert_eq!(scaled["anchors"][0]["outgoing"], json!([8.0, -3.0]));
    run(&mut e, "edit.undo", json!({}));
    run(&mut e, "image.orient", json!({"operation":"rotate_90_cw"}));
    let turned = run(&mut e, "path.get", get.clone());
    assert_eq!(turned["anchors"][0]["position"], json!([18.0, 2.0]));
    assert_eq!(turned["anchors"][0]["outgoing"], json!([2.0, 4.0]));
    run(&mut e, "edit.undo", json!({}));
    assert_eq!(run(&mut e, "path.get", get), original);
}
#[test]
fn invalid_stale_and_locked_edits_are_atomic_and_ai_can_undo_its_turn() {
    let mut e = new();
    let id = create(&mut e);
    let target = json!({"id":id});
    let stale = e.revision() - 1;
    let before = e.document().unwrap().to_archive().unwrap();
    for args in [
        json!({"path":target,"name":"bad","expected_revision":stale}),
        json!({"path":target,"anchors":[]}),
        json!({"path":target,"name":"a","unknown":true}),
        json!({"path":target,"anchors":[{"position":[0,0]},{"position":[1,1]},{"position":[1e12,0],"outgoing":[5,1]}]}),
    ] {
        assert!(
            e.execute(e.request(Source::User), "path.update", args, |_| Ok(()))
                .is_err()
        );
    }
    assert!(before == e.document().unwrap().to_archive().unwrap());
    let turn = e.begin_turn().unwrap();
    assert!(matches!(
        e.execute(
            e.request(Source::User),
            "path.delete",
            json!({"path":target}),
            |_| Ok(())
        ),
        Err(CommandError::ControlLocked)
    ));
    let req = e.request(Source::Ai { turn_id: turn });
    e.execute(
        req,
        "path.update",
        json!({"path":target,"name":"AI path"}),
        |_| Ok(()),
    )
    .unwrap();
    e.stop_turn();
    run(&mut e, "edit.undo_turn", json!({"turn_id":turn}));
    assert!(before == e.document().unwrap().to_archive().unwrap());
}

#[test]
fn splitting_curved_and_closing_segments_preserves_curve_and_undo() {
    fn point(path: &strata_core::WorkPath, i: usize, t: f64) -> [f64; 2] {
        let a = &path.anchors[i];
        let b = &path.anchors[(i + 1) % path.anchors.len()];
        std::array::from_fn(|k| {
            let u = 1.0 - t;
            u * u * u * a.position[k]
                + 3.0 * u * u * t * (a.position[k] + a.outgoing[k])
                + 3.0 * u * t * t * (b.position[k] + b.incoming[k])
                + t * t * t * b.position[k]
        })
    }
    let mut e = new();
    let id = create(&mut e);
    let original = e.document().unwrap().paths()[0].clone();
    for i in [0, 3] {
        run(
            &mut e,
            "path.split",
            json!({"path":{"id":id},"segment":i,"t":0.37}),
        );
        let edited = &e.document().unwrap().paths()[0];
        assert_eq!(edited.anchors.len(), 5);
        for j in 0..=100 {
            let t = f64::from(j) / 100.0;
            let expected = point(&original, i, t);
            let actual = if t <= 0.37 {
                point(edited, i, t / 0.37)
            } else {
                point(edited, i + 1, (t - 0.37) / 0.63)
            };
            for k in 0..2 {
                assert!((actual[k] - expected[k]).abs() < 1e-10);
            }
        }
        run(&mut e, "edit.undo", json!({}));
        assert_eq!(e.document().unwrap().paths()[0], original);
    }
}

fn marquee(e: &mut Editor, x: i64, y: i64, w: u32, h: u32, mode: &str) {
    run(
        e,
        "select.marquee",
        json!({"x":x,"y":y,"width":w,"height":h,"mode":mode}),
    );
}
#[test]
fn compound_trace_keeps_holes_nested_islands_and_corner_contacts_through_save_undo_and_fill() {
    let mut e = Editor::new();
    run(
        &mut e,
        "file.new",
        json!({"width":270,"height":16,"bit_depth":16}),
    );
    marquee(&mut e, 244, 0, 22, 13, "replace");
    marquee(&mut e, 248, 2, 15, 9, "subtract");
    marquee(&mut e, 251, 4, 4, 4, "add"); // island inside a hole
    marquee(&mut e, 266, 13, 3, 2, "add"); // touches the large component at one vertex
    let mask = e.document().unwrap().selection().unwrap().clone();
    let rev = e.revision();
    let id = run(
        &mut e,
        "path.from_selection",
        json!({"name":"Four rings","expected_revision":rev}),
    )["path_id"]
        .clone();
    let target = json!({"id":id});
    let path = run(&mut e, "path.get", json!({"path":target}));
    assert_eq!(path["subpaths"].as_array().unwrap().len(), 3);
    assert_eq!(e.revision(), rev + 1);
    let summary = run(&mut e, "path.list", json!({}));
    assert_eq!(summary["paths"][0]["anchor_count"], 16);
    assert_eq!(summary["paths"][0]["contour_count"], 4);
    let mut archive = e.document().unwrap().to_archive().unwrap();
    assert_eq!(archive.metadata["version"], 12);
    archive.metadata["version"] = json!(11);
    assert!(Document::from_archive(archive).is_err());
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("compound.strata");
    let grant = e.grant_write(&file, false, Source::User).unwrap();
    run(&mut e, "file.save_as", json!({"resource":grant}));
    let read = e.grant_read(&file, Source::User).unwrap();
    run(&mut e, "file.open", json!({"resource":read}));
    assert_eq!(run(&mut e, "path.get", json!({"path":target})), path);
    run(&mut e, "select.none", json!({}));
    run(&mut e, "select.from_path", json!({"path":target}));
    let actual = e.document().unwrap().selection().unwrap();
    for y in 0..16 {
        for x in 0..270 {
            assert_eq!(
                mask.sample(x, y).unwrap(),
                actual.sample(x, y).unwrap(),
                "{x},{y}"
            );
        }
    }
    run(
        &mut e,
        "layer.fill.path",
        json!({"path":target,"name":"Compound fill","color":[12345,12345,12345,65535]}),
    );
    let filled = e
        .document()
        .unwrap()
        .layers()
        .last()
        .unwrap()
        .mask()
        .unwrap();
    for (x, y) in [(244, 1), (250, 3), (252, 5), (267, 13), (0, 0)] {
        assert_eq!(filled.sample(x, y).unwrap(), mask.sample(x, y).unwrap());
    }
    run(
        &mut e,
        "path.split",
        json!({"path":target,"contour":2,"segment":3,"t":0.5}),
    );
    let split = run(&mut e, "path.get", json!({"path":target}));
    assert_eq!(split["subpaths"][1].as_array().unwrap().len(), 5);
    assert_eq!(split["anchors"], path["anchors"]);
    run(&mut e, "edit.undo", json!({}));
    assert_eq!(run(&mut e, "path.get", json!({"path":target})), path);
    run(
        &mut e,
        "path.update",
        json!({"path":target,"name":"Renamed"}),
    );
    assert_eq!(
        run(&mut e, "path.get", json!({"path":target}))["subpaths"],
        path["subpaths"]
    );
    run(
        &mut e,
        "image.orient",
        json!({"operation":"flip_horizontal"}),
    );
    let reflected = run(&mut e, "path.get", json!({"path":target}));
    for (a, b) in reflected["subpaths"]
        .as_array()
        .unwrap()
        .iter()
        .zip(path["subpaths"].as_array().unwrap())
    {
        assert_eq!(
            a[0]["position"][0].as_f64().unwrap(),
            270. - b[0]["position"][0].as_f64().unwrap()
        );
    }
}
#[test]
fn tracing_threshold_is_explicit_and_invalid_inputs_leave_history_unchanged() {
    let mut e = new();
    let init = e.revision();
    assert!(
        e.execute(
            e.request(Source::User),
            "path.from_selection",
            json!({"name":"Empty"}),
            |_| Ok(())
        )
        .is_err()
    );
    assert_eq!(e.revision(), init);
    run(
        &mut e,
        "select.ellipse",
        json!({"x":1,"y":2,"width":17,"height":15}),
    );
    let input = e.document().unwrap().selection().unwrap().clone();
    for threshold in [1, 32768, 65535] {
        let args = json!({"name":format!("Level {threshold}"),"threshold":threshold});
        let id = run(&mut e, "path.from_selection", args)["path_id"].clone();
        run(&mut e, "select.from_path", json!({"path":{"id":id}}));
        let m = e.document().unwrap().selection().unwrap();
        for x in 0..20 {
            for y in 0..20 {
                assert_eq!(
                    m.sample(x, y).unwrap(),
                    if input.sample(x, y).unwrap() >= threshold {
                        65535
                    } else {
                        0
                    }
                );
            }
        }
        run(&mut e, "edit.undo", json!({}));
        run(&mut e, "edit.undo", json!({}));
    }
    let exact = run(&mut e, "path.from_selection", json!({"name":"Exact"}))["path_id"].clone();
    let simplified = run(
        &mut e,
        "path.from_selection",
        json!({"name":"Simplified","tolerance":1.0}),
    )["path_id"]
        .clone();
    let count = |e: &mut Editor, id: Value| {
        run(e, "path.get", json!({"path":{"id":id}}))["anchors"]
            .as_array()
            .unwrap()
            .len()
    };
    assert!(count(&mut e, simplified) < count(&mut e, exact));
    let revision = e.revision();
    for p in [
        json!({"name":"bad","threshold":0}),
        json!({"name":"bad","tolerance":11}),
        json!({"name":"bad","expected_revision":0}),
    ] {
        assert!(
            e.execute(
                e.request(Source::User),
                "path.from_selection",
                p,
                |_| Ok(())
            )
            .is_err()
        );
        assert_eq!(e.revision(), revision);
    }
}

#[test]
fn simplified_trace_keeps_a_small_hole_near_the_boundary() {
    let mut e = new();
    run(
        &mut e,
        "select.ellipse",
        json!({"x":0,"y":0,"width":20,"height":20}),
    );
    marquee(&mut e, 2, 6, 1, 1, "subtract");
    let trace = run(
        &mut e,
        "path.from_selection",
        json!({"name":"Hole","tolerance":8.0}),
    );
    let target = json!({"id":trace["path_id"]});
    assert_eq!(
        run(&mut e, "path.get", json!({"path":target}))["subpaths"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    run(&mut e, "select.from_path", json!({"path":target}));
    assert_eq!(
        e.document()
            .unwrap()
            .selection()
            .unwrap()
            .sample(2, 6)
            .unwrap(),
        0
    );
    assert_eq!(
        e.document()
            .unwrap()
            .selection()
            .unwrap()
            .sample(10, 10)
            .unwrap(),
        65535
    );
}

#[test]
fn tracing_an_inverted_selection_respects_the_implicit_background_and_artboard_edges() {
    let mut e = new();
    marquee(&mut e, 0, 0, 7, 6, "replace");
    run(&mut e, "select.inverse", json!({}));
    let mask = e.document().unwrap().selection().unwrap().clone();
    let path = run(&mut e, "path.from_selection", json!({"name":"Complement"}));
    run(
        &mut e,
        "select.from_path",
        json!({"path":{"id":path["path_id"]}}),
    );
    for y in 0..20 {
        for x in 0..20 {
            assert_eq!(
                e.document()
                    .unwrap()
                    .selection()
                    .unwrap()
                    .sample(x, y)
                    .unwrap(),
                mask.sample(x, y).unwrap()
            );
        }
    }
    run(&mut e, "select.all", json!({}));
    let all = run(&mut e, "path.from_selection", json!({"name":"Full frame"}));
    let geometry = run(&mut e, "path.get", json!({"path":{"id":all["path_id"]}}));
    assert_eq!(geometry["anchors"].as_array().unwrap().len(), 4);
    assert_eq!(geometry["anchors"][2]["position"], json!([20.0, 20.0]));
}
