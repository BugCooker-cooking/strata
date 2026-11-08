use serde_json::{Value, json};
use strata_core::{Editor, LayerRef, Source};
use strata_pixels::memory::{MemoryLimits, MemoryPool};
fn run(e: &mut Editor, id: &str, p: Value) -> Value {
    e.execute(e.request(Source::User), id, p, |_| Ok(()))
        .unwrap()
}
fn doc() -> Editor {
    let mut e = Editor::new();
    run(
        &mut e,
        "file.new",
        json!({"width":200,"height":150,"bit_depth":16}),
    );
    e
}
fn box_layer(e: &mut Editor, name: &str, w: u32, h: u32, x: i64, y: i64) -> Value {
    let layer = run(e, "layer.create", json!({"name":name}))["layer_id"].clone();
    run(
        e,
        "select.marquee",
        json!({"x":0,"y":0,"width":w,"height":h}),
    );
    run(
        e,
        "paint.fill",
        json!({"layer":{"id":layer},"color":[0.2,0.4,0.8,1]}),
    );
    run(e, "select.none", json!({}));
    run(
        e,
        "layer.set_position",
        json!({"layer":{"id":layer},"x":x,"y":y}),
    );
    layer
}
fn snap(e: &mut Editor, target: &Value, x: i64, y: i64, anchors: &str) -> Value {
    run(
        e,
        "layer.snap_position",
        json!({"layer":{"id":target},"x":x,"y":y,"tolerance":2,"guides":false,"artboard":false,"layers":true,"anchors":anchors}),
    )
}
fn archive(e: &Editor) -> strata_io::project::ProjectArchive {
    e.document().unwrap().to_archive().unwrap()
}
#[test]
fn layer_edges_and_centers_use_independent_nearest_positions_and_return_actual_reference_ids() {
    let mut e = doc();
    let target = box_layer(&mut e, "Moving", 10, 8, 3, 4);
    let reference = box_layer(&mut e, "Reference", 20, 12, 50, 40);
    let before = archive(&e);
    let cursor = e.snapshot().history_cursor;
    let r = snap(&mut e, &target, 41, 39, "edges_and_center");
    assert_eq!(r["x"], 40);
    assert_eq!(r["y"], 40);
    assert_eq!(r["snapped_x"]["target"], "layer_anchor");
    assert_eq!(r["snapped_x"]["reference_layer"], reference);
    assert_eq!(r["snapped_x"]["reference_anchor"], "minimum");
    assert_eq!(r["snapped_x"]["source_anchor"], "maximum");
    assert_eq!(r["snapped_x"]["residual"], 0.);
    assert_eq!(e.snapshot().history_cursor, cursor + 1);
    assert_eq!(archive(&e).tiles, before.tiles);
    run(&mut e, "edit.undo", json!({}));
    assert!(archive(&e) == before);
    let r = snap(&mut e, &target, 55, 42, "center");
    assert_eq!(r["x"], 55);
    assert_eq!(r["y"], 42);
    assert_eq!(r["snapped_x"]["reference_anchor"], "center");
    assert_eq!(r["snapped_y"]["reference_anchor"], "center");
}
#[test]
fn hidden_layers_ancestors_and_the_targets_own_subtree_cannot_offer_snap_candidates() {
    let mut e = doc();
    let target = box_layer(&mut e, "Moving", 10, 8, 3, 4);
    let secret = box_layer(&mut e, "Hidden", 10, 8, 50, 40);
    run(
        &mut e,
        "layer.set_visibility",
        json!({"layer":{"id":secret},"visible":false}),
    );
    let r = snap(&mut e, &target, 41, 39, "edges");
    assert_eq!(r["x"], 41);
    assert!(r["snapped_x"].is_null());
    assert!(r["snapped_y"].is_null());
    let group = run(
        &mut e,
        "layer.group",
        json!({"layers":[{"id":target}],"name":"Group"}),
    )["layer_id"]
        .clone();
    let r = snap(&mut e, &group, 1, 0, "edges");
    assert!(
        r["snapped_x"].is_null(),
        "group must not snap against its own child"
    );
    let r = snap(&mut e, &target, 1, 0, "edges");
    assert!(
        r["snapped_x"].is_null(),
        "child must not snap against its containing group"
    );
    run(
        &mut e,
        "layer.set_visibility",
        json!({"layer":{"id":group},"visible":false}),
    );
    run(
        &mut e,
        "layer.select",
        json!({"layer":{"name":"Background"}}),
    );
    let public = box_layer(&mut e, "Public", 8, 8, 100, 100);
    let r = snap(&mut e, &public, 2, 4, "edges");
    assert!(r["snapped_x"].is_null());
    assert!(r["snapped_y"].is_null());
}
#[test]
fn group_reference_bounds_ignore_hidden_descendants_but_visible_text_and_fills_are_real_candidates()
{
    let mut e = doc();
    let target = box_layer(&mut e, "Moving", 10, 8, 3, 4);
    let visible = box_layer(&mut e, "Visible", 10, 8, 50, 40);
    let hidden = box_layer(&mut e, "Hidden", 10, 8, 20, 40);
    let group = run(
        &mut e,
        "layer.group",
        json!({"layers":[{"id":visible},{"id":hidden}],"name":"Reference group"}),
    )["layer_id"]
        .clone();
    run(
        &mut e,
        "layer.set_visibility",
        json!({"layer":{"id":hidden},"visible":false}),
    );
    let r = snap(&mut e, &target, 11, 30, "edges");
    assert!(
        r["snapped_x"].is_null(),
        "hidden child must not enlarge group reference"
    );
    assert_ne!(r["snapped_x"]["reference_layer"], group);
    run(
        &mut e,
        "layer.select",
        json!({"layer":{"name":"Background"}}),
    );
    let fill = run(
        &mut e,
        "layer.fill.solid",
        json!({"name":"Fill","color":[0,0,0,0]}),
    )["layer_id"]
        .clone();
    run(
        &mut e,
        "layer.set_position",
        json!({"layer":{"id":fill},"x":-100,"y":-100}),
    );
    let r = snap(&mut e, &target, -109, -80, "edges");
    assert_eq!(r["x"], -110);
    assert_eq!(r["snapped_x"]["reference_layer"], fill);
    run(
        &mut e,
        "layer.set_visibility",
        json!({"layer":{"id":fill},"visible":false}),
    );
    run(
        &mut e,
        "type.create",
        json!({"name":"Transparent Type","spec":{"text":"Ab","font":"sans","font_size":16,"layout":{"type":"point"}},"color":[0,0,0,0],"x":120,"y":30}),
    );
    let bounds = run(
        &mut e,
        "layer.pixel_bounds",
        json!({"layer":{"name":"Transparent Type"}}),
    )["bounds"]
        .clone();
    let x = bounds["x"].as_i64().unwrap();
    let r = snap(&mut e, &target, x - 9, 0, "edges");
    assert_eq!(r["x"], x - 10);
    assert_eq!(
        r["snapped_x"]["reference_layer"],
        json!(e.snapshot().active_layer)
    );
}
#[test]
fn ties_are_stable_and_guide_then_artboard_preferences_remain_compatible() {
    let mut e = doc();
    let target = box_layer(&mut e, "Moving", 10, 8, 3, 4);
    let a = box_layer(&mut e, "A", 20, 12, 50, 40);
    let b = box_layer(&mut e, "B", 20, 12, 50, 40);
    let expected = a.as_str().unwrap().min(b.as_str().unwrap());
    let r = snap(&mut e, &target, 41, 39, "edges");
    assert_eq!(r["snapped_x"]["reference_layer"], expected);
    run(
        &mut e,
        "view.guides.add",
        json!({"name":"Preferred","axis":"vertical","position":50}),
    );
    let r = run(
        &mut e,
        "layer.snap_position",
        json!({"layer":{"id":target},"x":41,"y":39,"tolerance":2,"guides":true,"artboard":false,"layers":true,"anchors":"edges"}),
    );
    assert_eq!(r["snapped_x"]["target"], "guide");
    assert!(r["snapped_x"]["reference_layer"].is_null());
}
#[test]
fn odd_width_center_residual_and_negative_large_coordinates_are_explicit() {
    let mut e = doc();
    let target = box_layer(&mut e, "Odd", 9, 8, 3, 4);
    let reference = box_layer(&mut e, "Reference", 20, 12, 50, 40);
    let r = snap(&mut e, &target, 45, 42, "center");
    assert_eq!(r["x"], 46);
    assert_eq!(r["snapped_x"]["residual"], -0.5);
    assert_eq!(r["snapped_x"]["anchor"], 50.5);
    assert_eq!(r["snapped_x"]["reference_layer"], reference);
    let big = -700000000000i64;
    run(
        &mut e,
        "layer.set_position",
        json!({"layer":{"id":reference},"x":big,"y":40}),
    );
    let r = snap(&mut e, &target, big - 5, 42, "center");
    assert_eq!(r["x"], big - 5);
    assert_eq!(r["snapped_x"]["residual"], 0.5);
}
#[test]
fn cached_reference_measurement_is_reused_budgeted_and_cannot_survive_revision_or_setting_changes()
{
    let mut e = doc();
    let target = box_layer(&mut e, "Moving", 10, 8, 3, 4);
    box_layer(&mut e, "Reference", 20, 12, 50, 40);
    run(&mut e, "layer.select", json!({"layer":{"id":target}}));
    let before = archive(&e);
    let owner = e
        .begin_filter(LayerRef::Name("Moving".into()), "layer.snap_position")
        .unwrap();
    assert!(!e.snap_preview_measurement_ready(&owner, true));
    let measured = e.measure_snap_preview_with_layers(&owner, true).unwrap();
    assert!(e.snap_preview_measurement_ready(&measured, true));
    assert!(!e.snap_preview_measurement_ready(&measured, false));
    let current = e.document().unwrap().memory_pool().stats().transient_bytes;
    let second = e.measure_snap_preview_with_layers(&measured, true).unwrap();
    assert!(e.snap_preview_measurement_ready(&second, true));
    assert_eq!(
        e.document().unwrap().memory_pool().stats().transient_bytes,
        current
    );
    let candidate = e
        .preview_filter(
            &second,
            "layer.snap_position",
            json!({"x":41,"y":39,"guides":false,"artboard":false,"layers":true}),
        )
        .unwrap();
    assert_eq!(candidate.result()["x"], 40);
    assert!(archive(&e) == before);
    run(&mut e, "view.presentation.set", json!({"snap_layers":true}));
    assert!(!e.snap_preview_measurement_ready(&second, true));
    assert!(e.commit(candidate).is_err());
    assert!(archive(&e) == before);
}
#[test]
fn low_budget_and_reference_scan_work_limits_fail_before_changing_original_sources() {
    let pool = MemoryPool::new(MemoryLimits {
        cpu_bytes: 1024 * 1024,
        gpu_bytes: 0,
        disk_bytes: 8 * 1024 * 1024,
    });
    let mut e = Editor::with_memory_pool(pool.clone());
    run(
        &mut e,
        "file.new",
        json!({"width":16,"height":16,"bit_depth":8,"background":[10,20,30,255]}),
    );
    let before = archive(&e);
    assert!(
        e.execute(
            e.request(Source::User),
            "layer.snap_position",
            json!({"layer":{"name":"Background"},"x":1,"y":1,"layers":true}),
            |_| Ok(())
        )
        .is_err()
    );
    assert!(archive(&e) == before);
    assert_eq!(pool.stats().transient_bytes, 0);
    let mut e = doc();
    let target = box_layer(&mut e, "Moving", 10, 8, 3, 4);
    let mut data = archive(&e);
    let template = data.metadata["layers"][0].clone();
    for i in 0..2 {
        let mut fill = template.clone();
        fill["id"] = json!(format!("00000000-0000-4000-8000-{:012x}", i + 1));
        fill["name"] = json!(format!("Huge{i}"));
        fill["kind"] = json!({"kind":"solid","color":[1,2,3,65535]});
        fill["generated_size"] = json!([6000, 6000]);
        data.metadata["layers"].as_array_mut().unwrap().push(fill);
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("huge.strata");
    strata_io::project::write_project(&path, &data, false, false).unwrap();
    let mut opened = Editor::new();
    let token = opened.grant_read(&path, Source::User).unwrap();
    run(&mut opened, "file.open", json!({"resource":token}));
    let before = archive(&opened);
    assert!(
        opened
            .execute(
                opened.request(Source::User),
                "layer.snap_position",
                json!({"layer":{"id":target},"x":41,"y":39,"layers":true}),
                |_| Ok(())
            )
            .is_err()
    );
    assert!(archive(&opened) == before);
}
