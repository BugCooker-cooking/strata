use serde_json::{Value, json};
use strata_core::{Document, Editor, LayerRef, Source};
fn run(e: &mut Editor, id: &str, p: Value) -> Value {
    e.execute(e.request(Source::User), id, p, |_| Ok(()))
        .unwrap()
}
fn fixture(depth: u8) -> Editor {
    let mut e = Editor::new();
    run(
        &mut e,
        "file.new",
        json!({"width":32,"height":24,"bit_depth":depth}),
    );
    e
}
fn archive(e: &Editor) -> strata_io::project::ProjectArchive {
    e.document().unwrap().to_archive().unwrap()
}
fn add(e: &mut Editor, name: &str, axis: &str, position: f64) -> Value {
    run(
        e,
        "view.guides.add",
        json!({"name":name,"axis":axis,"position":position}),
    )["guide"]
        .clone()
}
fn boxed(depth: u8) -> Editor {
    let mut e = fixture(depth);
    run(
        &mut e,
        "select.marquee",
        json!({"x":2,"y":3,"width":4,"height":6}),
    );
    run(
        &mut e,
        "paint.fill",
        json!({"layer":{"name":"Background"},"color":[0.2,0.4,0.8,1]}),
    );
    run(
        &mut e,
        "layer.mask.add",
        json!({"layer":{"name":"Background"},"source":"white"}),
    );
    run(&mut e, "select.none", json!({}));
    e
}

#[test]
fn guide_crud_pages_ids_and_exact_noops_preserve_pixels_and_history() {
    let mut e = fixture(16);
    let original = archive(&e);
    let a = add(&mut e, "Baseline", "horizontal", -3.25);
    let b = add(&mut e, "Side", "vertical", 18.5);
    assert_ne!(a["id"], b["id"]);
    assert_eq!(e.document().unwrap().guides().len(), 2);
    assert_eq!(archive(&e).metadata["version"], 9);
    assert_eq!(archive(&e).tiles, original.tiles);
    let before = archive(&e);
    let revision = e.revision();
    let page = run(&mut e, "view.guides.list", json!({"limit":1}));
    assert_eq!(page["total"], 2);
    assert_eq!(page["next_offset"], 1);
    assert_eq!(page["guides"][0], a);
    let last = run(&mut e, "view.guides.list", json!({"offset":1,"limit":1}));
    assert_eq!(last["next_offset"], Value::Null);
    assert_eq!(last["guides"][0], b);
    assert_eq!(e.revision(), revision);
    assert!(archive(&e) == before);
    let noop = run(
        &mut e,
        "view.guides.update",
        json!({"guide":{"id":a["id"]},"axis":"horizontal","position":-3.25,"name":"Baseline"}),
    );
    assert_eq!(noop["changed"], false);
    assert_eq!(e.revision(), revision);
    assert!(archive(&e) == before);
    run(
        &mut e,
        "view.guides.update",
        json!({"guide":{"id":a["id"]},"position":4.125,"name":"new name"}),
    );
    run(&mut e, "edit.undo", json!({}));
    assert!(archive(&e) == before);
    run(
        &mut e,
        "view.guides.delete",
        json!({"guide":{"id":b["id"]}}),
    );
    assert_eq!(e.document().unwrap().guides().len(), 1);
    run(&mut e, "edit.undo", json!({}));
    assert!(archive(&e) == before);
    run(&mut e, "view.guides.clear", json!({}));
    assert!(e.document().unwrap().guides().is_empty());
    run(&mut e, "edit.undo", json!({}));
    assert!(archive(&e) == before);
    run(&mut e, "edit.undo", json!({}));
    run(&mut e, "edit.undo", json!({}));
    assert!(archive(&e) == original);
}
#[test]
fn presentation_is_editor_local_versioned_and_cannot_let_late_guide_edits_bypass_lock() {
    let mut e = Editor::new();
    let initial = run(&mut e, "view.presentation.get", json!({}));
    assert_eq!(initial["snap_enabled"], false);
    let result = run(
        &mut e,
        "view.presentation.set",
        json!({"rulers_visible":false,"snap_enabled":true,"snap_tolerance":8,"expected_version":0}),
    );
    assert_eq!(result["version"], 1);
    assert_eq!(e.revision(), 0);
    assert!(e.document().is_none());
    let same = run(
        &mut e,
        "view.presentation.set",
        json!({"snap_enabled":true}),
    );
    assert_eq!(same["version"], 1);
    run(
        &mut e,
        "file.new",
        json!({"width":32,"height":24,"bit_depth":16}),
    );
    let before = archive(&e);
    let revision = e.revision();
    let pending = e
        .prepare(
            e.request(Source::User),
            "view.guides.add",
            json!({"name":"late","axis":"horizontal","position":3}),
        )
        .unwrap();
    run(
        &mut e,
        "view.presentation.set",
        json!({"guides_locked":true}),
    );
    assert_eq!(e.revision(), revision);
    assert!(archive(&e) == before);
    assert!(e.commit(pending).is_err());
    assert!(archive(&e) == before);
    assert!(
        e.execute(
            e.request(Source::User),
            "view.guides.add",
            json!({"name":"locked","axis":"vertical","position":5}),
            |_| Ok(())
        )
        .is_err()
    );
    let turn = e.begin_turn().unwrap();
    run(
        &mut e,
        "view.presentation.set",
        json!({"guides_visible":false}),
    );
    assert!(
        e.execute(
            e.request(Source::User),
            "view.guides.clear",
            json!({}),
            |_| Ok(())
        )
        .is_err()
    );
    let r = e.request(Source::Ai { turn_id: turn });
    assert!(
        e.execute(r, "view.presentation.get", json!({}), |_| Ok(()))
            .is_ok()
    );
    e.stop_turn();
    assert!(archive(&e) == before);
}
#[test]
fn annotation_commit_and_undo_preserve_newer_layer_selection_without_preparing_pixels() {
    let mut e = fixture(16);
    run(&mut e, "layer.create", json!({"name":"Other"}));
    let prepared = e
        .prepare(
            e.request(Source::User),
            "view.guides.add",
            json!({"name":"line","axis":"vertical","position":5}),
        )
        .unwrap();
    assert!(!prepared.changes_display());
    run(
        &mut e,
        "layer.select",
        json!({"layer":{"name":"Background"}}),
    );
    let selected = e.snapshot().active_layer;
    e.commit(prepared).unwrap();
    assert_eq!(e.snapshot().active_layer, selected);
    assert_eq!(e.document().unwrap().active_layer(), selected);
    run(&mut e, "layer.select", json!({"layer":{"name":"Other"}}));
    let selected = e.snapshot().active_layer;
    let undo = e
        .prepare(e.request(Source::User), "edit.undo", json!({}))
        .unwrap();
    assert!(!undo.changes_display());
    e.commit(undo).unwrap();
    assert_eq!(e.snapshot().active_layer, selected);
    assert_eq!(e.document().unwrap().active_layer(), selected);
    assert!(e.document().unwrap().guides().is_empty());
}
#[test]
fn snap_uses_independent_nearest_anchor_and_reports_integer_residual_without_touching_codes() {
    for depth in [8, 16] {
        let mut e = boxed(depth);
        add(&mut e, "X", "vertical", 20.5);
        add(&mut e, "Y", "horizontal", -10.);
        let before = archive(&e);
        let layer = e.document().unwrap().layers()[0].clone();
        let result = run(
            &mut e,
            "layer.snap_position",
            json!({"layer":{"name":"Background"},"x":17,"y":-15,"tolerance":4,"artboard":false}),
        );
        assert_eq!(result["x"], 17);
        assert_eq!(result["y"], -16);
        assert_eq!(result["snapped_x"]["residual"], -0.5);
        assert_eq!(result["snapped_y"]["residual"], 0.);
        assert_eq!(archive(&e).tiles, before.tiles);
        let moved = &e.document().unwrap().layers()[0];
        assert_eq!(moved.info().mask, layer.info().mask);
        assert_eq!(moved.pixels().tile_count(), layer.pixels().tile_count());
        let revision = e.revision();
        let current = archive(&e);
        let noop = run(
            &mut e,
            "layer.snap_position",
            json!({"layer":{"name":"Background"},"x":17,"y":-16,"tolerance":0,"guides":false,"artboard":false}),
        );
        assert_eq!(noop["changed"], false);
        assert_eq!(e.revision(), revision);
        assert!(archive(&e) == current);
        run(&mut e, "edit.undo", json!({}));
        assert!(archive(&e) == before);
    }
}
#[test]
fn tie_priority_is_deterministic_and_group_snapping_ignores_hidden_bounds_but_moves_subtree() {
    let mut e = fixture(8);
    let group = run(&mut e, "layer.create_group", json!({"name":"G"}))["layer_id"].clone();
    run(
        &mut e,
        "layer.fill.solid",
        json!({"name":"Fill","color":[255,0,0,255]}),
    );
    run(
        &mut e,
        "layer.create",
        json!({"name":"Hidden","fill":[0,0,255,255]}),
    );
    run(
        &mut e,
        "layer.set_position",
        json!({"layer":{"name":"Hidden"},"x":1000,"y":1000}),
    );
    run(
        &mut e,
        "layer.set_visibility",
        json!({"layer":{"name":"Hidden"},"visible":false}),
    );
    let first = add(&mut e, "duplicate", "vertical", 50.);
    let second = add(&mut e, "duplicate", "vertical", 50.);
    let expected = first["id"]
        .as_str()
        .unwrap()
        .min(second["id"].as_str().unwrap());
    let result = run(
        &mut e,
        "layer.snap_position",
        json!({"layer":{"id":group},"x":19,"y":0,"tolerance":2,"artboard":false,"anchors":"edges"}),
    );
    assert_eq!(result["x"], 18);
    assert_eq!(result["snapped_x"]["guide_id"], expected);
    let hidden = e
        .document()
        .unwrap()
        .layers()
        .iter()
        .find(|l| l.info().name == "Hidden")
        .unwrap();
    assert_eq!(hidden.info().x, 1018);
    assert!(!hidden.info().visible);
    let before = archive(&e);
    assert!(
        e.execute(
            e.request(Source::User),
            "view.guides.update",
            json!({"guide":{"name":"duplicate"},"position":3}),
            |_| Ok(())
        )
        .is_err()
    );
    assert!(archive(&e) == before);
}
#[test]
fn guides_survive_real_save_recovery_and_image_edits_as_fixed_canvas_references() {
    let mut e = boxed(16);
    add(&mut e, "X", "vertical", 7.25);
    add(&mut e, "Y", "horizontal", -3.5);
    let guides = e.snapshot().guides;
    let before = archive(&e);
    let mut old = before.clone();
    old.metadata["version"] = json!(8);
    assert!(Document::from_archive(old).is_err());
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("guides.strata");
    let token = e.grant_write(&path, false, Source::User).unwrap();
    run(&mut e, "file.save_as", json!({"resource":token}));
    let mut opened = Editor::new();
    let token = opened.grant_read(&path, Source::User).unwrap();
    run(&mut opened, "file.open", json!({"resource":token}));
    assert!(archive(&opened) == before);
    run(
        &mut opened,
        "image.resize",
        json!({"width":16,"height":12,"interpolation":"nearest"}),
    );
    run(
        &mut opened,
        "image.orient",
        json!({"operation":"rotate_90_cw"}),
    );
    assert_eq!(opened.snapshot().guides, guides);
    let store = strata_io::recovery::RecoveryStore::new(&dir.path().join("recovery")).unwrap();
    let entry = opened
        .checkpoint_recovery(&store, "guide")
        .unwrap()
        .unwrap();
    let path = store.path(&entry.session_id).unwrap();
    let mut recovered = Editor::new();
    let token = recovered.grant_read(&path, Source::User).unwrap();
    run(&mut recovered, "file.recover", json!({"resource":token}));
    assert_eq!(recovered.snapshot().guides, guides);
}
#[test]
fn snap_preview_cache_is_readonly_and_bound_to_layer_settings_and_original_truth() {
    let mut e = boxed(16);
    add(&mut e, "line", "vertical", 12.);
    let before = archive(&e);
    let owner = e
        .begin_filter(LayerRef::Name("Background".into()), "layer.snap_position")
        .unwrap();
    let measured = e.measure_snap_preview(&owner).unwrap();
    let ready = e
        .preview_filter(
            &measured,
            "layer.snap_position",
            json!({"x":9,"y":0,"tolerance":2,"artboard":false}),
        )
        .unwrap();
    assert!(ready.changes_display());
    assert!(archive(&e) == before);
    run(
        &mut e,
        "view.presentation.set",
        json!({"snap_enabled":true}),
    );
    assert!(e.commit(ready).is_err());
    assert!(archive(&e) == before);
    let owner = e
        .begin_filter(LayerRef::Name("Background".into()), "layer.snap_position")
        .unwrap();
    let measured = e.measure_snap_preview(&owner).unwrap();
    let ready = e
        .preview_filter(
            &measured,
            "layer.snap_position",
            json!({"x":9,"y":0,"tolerance":2,"artboard":false}),
        )
        .unwrap();
    e.commit(ready).unwrap();
    assert_eq!(e.document().unwrap().layers()[0].info().x, 10);
    run(&mut e, "edit.undo", json!({}));
    assert!(archive(&e) == before);
}
#[test]
fn editable_text_numeric_transform_preview_uses_same_geometry_and_late_selection_is_rejected() {
    let mut e = fixture(16);
    run(
        &mut e,
        "type.create",
        json!({"name":"Type","spec":{"text":"Ab","font":"sans","font_size":16,"layout":{"type":"point"}},"color":[0,0,0,0],"x":2,"y":18}),
    );
    let before = archive(&e);
    let owner = e
        .begin_filter(LayerRef::Name("Type".into()), "transform.apply")
        .unwrap();
    let candidate = e
        .preview_filter(
            &owner,
            "transform.apply",
            json!({"scope":"whole_layer","geometry":{"type":"components","rotation_degrees":13}}),
        )
        .unwrap();
    assert!(archive(&e) == before);
    run(
        &mut e,
        "layer.select",
        json!({"layer":{"name":"Background"}}),
    );
    assert!(e.commit(candidate).is_err());
}
#[test]
fn invalid_guides_locks_and_display_failure_are_atomic_and_guide_only_history_is_undoable() {
    let mut e = boxed(16);
    let guide = add(&mut e, "line", "vertical", 8.);
    let before = archive(&e);
    for params in [
        json!({"name":"bad\nname","axis":"vertical","position":4}),
        json!({"name":"bad","axis":"wrong","position":4}),
        json!({"name":"bad","axis":"vertical","position":1e13}),
        json!({"name":"x".repeat(257),"axis":"horizontal","position":1}),
    ] {
        assert!(
            e.execute(e.request(Source::User), "view.guides.add", params, |_| Ok(
                ()
            ))
            .is_err()
        );
        assert!(archive(&e) == before);
    }
    assert!(
        e.execute(
            e.request(Source::User),
            "view.guides.update",
            json!({"guide":{"id":guide["id"]},"position":5}),
            |_| Err("display rejected".into())
        )
        .is_err()
    );
    assert!(archive(&e) == before);
    run(
        &mut e,
        "layer.set_locks",
        json!({"layer":{"name":"Background"},"locks":{"position":true}}),
    );
    let locked = archive(&e);
    assert!(
        e.execute(
            e.request(Source::User),
            "layer.snap_position",
            json!({"layer":{"name":"Background"},"x":4,"y":3}),
            |_| Ok(())
        )
        .is_err()
    );
    assert!(archive(&e) == locked);
    let mut invalid = before;
    invalid.metadata["guides"][0]["id"] = json!("00000000-0000-0000-0000-000000000000");
    assert!(Document::from_archive(invalid).is_err());
}

#[test]
fn guide_limit_pagination_rejects_duplicate_metadata_and_long_names_before_mutation() {
    let mut e = fixture(16);
    add(&mut e, "seed", "vertical", 0.);
    let mut data = archive(&e);
    let template = data.metadata["guides"][0].clone();
    let guides = (0..1024)
        .map(|i| {
            let mut g = template.clone();
            g["id"] = json!(format!("00000000-0000-4000-8000-{:012x}", i + 1));
            g["name"] = json!(format!("Line{i}"));
            g["position"] = json!(i as f64 / 4.);
            g
        })
        .collect::<Vec<_>>();
    data.metadata["guides"] = Value::Array(guides);
    let document = Document::from_archive(data.clone()).unwrap();
    assert_eq!(document.guides().len(), 1024);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("full.strata");
    strata_io::project::write_project(&path, &data, false, false).unwrap();
    let mut opened = Editor::new();
    let token = opened.grant_read(&path, Source::User).unwrap();
    run(&mut opened, "file.open", json!({"resource":token}));
    let before = archive(&opened);
    assert!(
        opened
            .execute(
                opened.request(Source::User),
                "view.guides.add",
                json!({"name":"extra","axis":"horizontal","position":4}),
                |_| Ok(())
            )
            .is_err()
    );
    assert!(archive(&opened) == before);
    let mut offset = 0usize;
    let mut count = 0;
    loop {
        let page = run(
            &mut opened,
            "view.guides.list",
            json!({"offset":offset,"limit":64}),
        );
        assert!(page["guides"].as_array().unwrap().len() <= 64);
        count += page["guides"].as_array().unwrap().len();
        match page["next_offset"].as_u64() {
            Some(next) => offset = next as usize,
            None => break,
        }
    }
    assert_eq!(count, 1024);
    let empty = run(
        &mut opened,
        "view.guides.list",
        json!({"offset":1024,"limit":64}),
    );
    assert!(empty["guides"].as_array().unwrap().is_empty());
    let mut invalid = data;
    invalid.metadata["guides"][1]["id"] = invalid.metadata["guides"][0]["id"].clone();
    assert!(Document::from_archive(invalid).is_err());
}
#[test]
fn snap_references_at_large_negative_offsets_have_exact_integer_positions_and_no_pixel_rewrite() {
    let mut e = boxed(16);
    let x = -800000000000i64;
    let y = 700000000000i64;
    run(
        &mut e,
        "layer.set_position",
        json!({"layer":{"name":"Background"},"x":x,"y":y}),
    );
    add(&mut e, "farX", "vertical", (x + 100) as f64 + 0.5);
    add(&mut e, "farY", "horizontal", (y - 100) as f64);
    let before = archive(&e);
    let result = run(
        &mut e,
        "layer.snap_position",
        json!({"layer":{"name":"Background"},"x":x+98,"y":y-104,"tolerance":2,"artboard":false,"anchors":"edges"}),
    );
    assert_eq!(result["x"], x + 98);
    assert_eq!(result["y"], y - 103);
    assert_eq!(result["snapped_x"]["residual"], 0.5);
    assert_eq!(archive(&e).tiles, before.tiles);
    run(&mut e, "edit.undo", json!({}));
    assert!(archive(&e) == before);
}

#[test]
fn original_guide_pointer_owner_pins_epoch_target_and_local_cancellation() {
    let mut e = fixture(16);
    let a = add(&mut e, "A", "vertical", 5.);
    let b = add(&mut e, "B", "horizontal", 7.);
    let owner = e
        .begin_guide(Some(strata_core::GuideRef::Name("A".into())))
        .unwrap();
    let before = archive(&e);
    assert!(
        e.prepare_guide(&owner, json!({"guide":{"id":b["id"]},"position":8}))
            .is_err()
    );
    assert!(archive(&e) == before);
    let local = strata_core::commands::WorkCancellation::default();
    let scoped = owner.with_work_cancellation(&local);
    local.cancel();
    assert!(
        e.prepare_guide(&scoped, json!({"guide":{"id":a["id"]},"position":9}))
            .is_err()
    );
    e.begin_turn().unwrap();
    e.stop_turn();
    assert!(
        e.prepare_guide(&owner, json!({"guide":{"id":a["id"]},"position":9}))
            .is_err()
    );
    assert!(archive(&e) == before);
}

#[test]
fn out_of_range_source_and_expired_snap_queries_do_not_round_far_content_to_an_unrelated_anchor() {
    let mut e = boxed(16);
    add(&mut e, "X", "vertical", 10.5);
    run(
        &mut e,
        "layer.set_position",
        json!({"layer":{"name":"Background"},"x":4000000000000000i64,"y":0}),
    );
    let before = archive(&e);
    assert!(
        e.execute(
            e.request(Source::User),
            "layer.snap_position",
            json!({"layer":{"name":"Background"},"x":9,"y":0}),
            |_| Ok(())
        )
        .is_err()
    );
    assert!(archive(&e) == before);
    let request = e
        .request(Source::User)
        .with_timeout(std::time::Duration::ZERO);
    assert!(
        e.prepare(
            request,
            "layer.snap_position",
            json!({"layer":{"name":"Background"},"x":9,"y":0})
        )
        .is_err()
    );
    assert!(archive(&e) == before);
}
