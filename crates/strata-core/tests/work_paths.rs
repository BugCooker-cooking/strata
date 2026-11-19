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
