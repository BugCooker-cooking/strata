use serde_json::{Value, json};
use strata_core::{Editor, Source};
use strata_pixels::{
    TileCoord,
    memory::{MemoryLimits, MemoryPool},
};
fn run(e: &mut Editor, id: &str, p: Value) -> Value {
    e.execute(e.request(Source::User), id, p, |_| Ok(()))
        .unwrap()
}
fn editor(depth: u8, background: [u16; 4]) -> Editor {
    let mut e = Editor::new();
    run(
        &mut e,
        "file.new",
        json!({"width":16,"height":16,"bit_depth":depth,"background":background}),
    );
    e
}
fn rgba(e: &Editor, x: i64, y: i64) -> [u16; 4] {
    match e.document().unwrap().layers()[0]
        .pixels()
        .pixel(x, y)
        .unwrap()
    {
        strata_pixels::PixelValue::Rgba8(p) => p.map(u16::from),
        strata_pixels::PixelValue::Rgba16(p) => p,
    }
}
fn stroke() -> Value {
    json!({"layer":{"name":"Background"},"points":[{"x":8.5,"y":8.5}],"size":6,"hardness":1,"color":[1,1,1,1]})
}
#[test]
fn paint_uses_linear_light_and_protected_16bit_codes_stay_exact() {
    for depth in [8, 16] {
        let mut e = editor(depth, [0, 0, 0, if depth == 16 { 65535 } else { 255 }]);
        let mut p = stroke();
        p["opacity"] = json!(0.5);
        run(&mut e, "paint.stroke", p);
        let max: f64 = if depth == 16 { 65535.0 } else { 255.0 };
        let expected = (0.7353569830524495 * max).round() as u16;
        let center = rgba(&e, 8, 8);
        assert!(center[0].abs_diff(expected) <= 3);
        assert_eq!(center[3], max as u16);
        assert_eq!(rgba(&e, 0, 0), [0, 0, 0, max as u16]);
        assert_eq!(e.snapshot().history.len(), 1);
        run(&mut e, "edit.undo", json!({}));
        assert_eq!(rgba(&e, 8, 8), [0, 0, 0, max as u16]);
        run(&mut e, "edit.redo", json!({}));
        assert_eq!(rgba(&e, 8, 8), center);
    }
    let mut e = editor(16, [32768, 32769, 50001, 65535]);
    run(
        &mut e,
        "select.marquee",
        json!({"x":8,"y":0,"width":8,"height":16}),
    );
    let mut p = stroke();
    p["size"] = json!(30);
    p["hardness"] = json!(0.3);
    p["color"] = json!([0.1, 0.6, 0.2, 1]);
    run(&mut e, "paint.stroke", p);
    for y in 0..16 {
        for x in 0..8 {
            assert_eq!(rgba(&e, x, y), [32768, 32769, 50001, 65535]);
        }
    }
    assert_ne!(rgba(&e, 8, 8), [32768, 32769, 50001, 65535]);
}
#[test]
fn dab_spacing_is_event_density_independent_and_opacity_caps_flow_accumulation() {
    let mut sparse = editor(16, [0, 0, 0, 0]);
    let mut dense = editor(16, [0, 0, 0, 0]);
    let mut p = stroke();
    p["size"] = json!(4);
    p["hardness"] = json!(0.5);
    p["opacity"] = json!(0.5);
    p["flow"] = json!(0.2);
    p["spacing"] = json!(0.25);
    p["points"] = json!([{"x":1.5,"y":8.5},{"x":14.5,"y":8.5}]);
    run(&mut sparse, "paint.stroke", p.clone());
    p["points"] = json!(
        (1..=14)
            .map(|x| json!({"x":f64::from(x)+0.5,"y":8.5}))
            .collect::<Vec<_>>()
    );
    run(&mut dense, "paint.stroke", p);
    assert!(
        sparse.document().unwrap().export_pixels().unwrap()
            == dense.document().unwrap().export_pixels().unwrap(),
        "event density changed stroke samples"
    );
    let mut high = editor(16, [0, 0, 0, 0]);
    let mut low = editor(16, [0, 0, 0, 0]);
    let mut p = stroke();
    p["points"] = json!([{"x":8.5,"y":8.5},{"x":9.5,"y":8.5},{"x":8.5,"y":8.5}]);
    p["size"] = json!(8);
    p["spacing"] = json!(0.125);
    p["opacity"] = json!(0.5);
    run(&mut high, "paint.stroke", p.clone());
    p["flow"] = json!(0.1);
    run(&mut low, "paint.stroke", p);
    assert_eq!(rgba(&high, 8, 8)[3], 32768);
    assert!(rgba(&low, 8, 8)[3] < 20000);
}
#[test]
fn pressure_controls_size_and_opacity_and_soft_boundary_is_antialiased() {
    let mut small = editor(16, [0, 0, 0, 0]);
    let mut large = editor(16, [0, 0, 0, 0]);
    let mut p = stroke();
    p["size"] = json!(8);
    p["points"] = json!([{"x":8.5,"y":8.5,"pressure":0.5}]);
    p["pressure_opacity"] = json!(false);
    run(&mut small, "paint.stroke", p.clone());
    p["pressure_size"] = json!(false);
    run(&mut large, "paint.stroke", p);
    assert_eq!(rgba(&small, 11, 8)[3], 0);
    assert!(rgba(&large, 11, 8)[3] > 60000);
    let mut pressure = editor(16, [0, 0, 0, 0]);
    let mut p = stroke();
    p["pressure_size"] = json!(false);
    p["points"] = json!([{"x":8.5,"y":8.5,"pressure":0.25}]);
    run(&mut pressure, "paint.stroke", p);
    assert!(rgba(&pressure, 8, 8)[3].abs_diff(16384) <= 1);
    let mut soft = editor(16, [0, 0, 0, 0]);
    let mut p = stroke();
    p["hardness"] = json!(0.0);
    run(&mut soft, "paint.stroke", p);
    assert!(rgba(&soft, 8, 8)[3] > rgba(&soft, 9, 8)[3]);
    assert!(rgba(&soft, 9, 8)[3] > rgba(&soft, 10, 8)[3]);
    assert!(rgba(&soft, 10, 8)[3] > 0);
    assert_eq!(rgba(&soft, 12, 8)[3], 0);
}
#[test]
fn eraser_preserves_hidden_rgb_and_transparency_lock_constrains_paint() {
    let mut e = editor(16, [32768, 32769, 50001, 40000]);
    let mut p = stroke();
    p["opacity"] = json!(0.5);
    run(&mut e, "paint.erase", p);
    assert_eq!(rgba(&e, 8, 8), [32768, 32769, 50001, 20000]);
    assert_eq!(rgba(&e, 0, 0), [32768, 32769, 50001, 40000]);
    run(
        &mut e,
        "layer.set_locks",
        json!({"layer":{"name":"Background"},"locks":{"transparency":true}}),
    );
    let before = rgba(&e, 8, 8);
    run(&mut e, "paint.stroke", stroke());
    assert_eq!(rgba(&e, 8, 8)[3], before[3]);
    assert_ne!(&rgba(&e, 8, 8)[..3], &before[..3]);
    let revision = e.revision();
    assert!(
        e.execute(e.request(Source::User), "paint.erase", stroke(), |_| Ok(()))
            .is_err()
    );
    assert_eq!(e.revision(), revision);
    let mut transparent = editor(16, [32768, 32769, 50001, 0]);
    run(
        &mut transparent,
        "layer.set_locks",
        json!({"layer":{"name":"Background"},"locks":{"transparency":true}}),
    );
    run(&mut transparent, "paint.stroke", stroke());
    assert_eq!(rgba(&transparent, 8, 8), [32768, 32769, 50001, 0]);
}
#[test]
fn mask_paint_follows_linked_anchor_and_roundtrips_without_changing_pixel_source() {
    let mut e = editor(16, [32768, 32769, 50001, 65535]);
    run(
        &mut e,
        "layer.mask.add",
        json!({"layer":{"name":"Background"},"source":"white"}),
    );
    let original = e.document().unwrap().layers()[0].pixels().clone();
    run(
        &mut e,
        "layer.set_position",
        json!({"layer":{"name":"Background"},"x":3,"y":-1}),
    );
    let mut p = stroke();
    p["target"] = json!("mask");
    p["value"] = json!(0);
    p["size"] = json!(2);
    p["points"] = json!([{"x":4.5,"y":2.5}]);
    run(&mut e, "paint.stroke", p);
    let doc = e.document().unwrap();
    let mask = doc.layers()[0].mask().unwrap();
    assert_eq!(mask.sample(1, 3).unwrap(), 0);
    assert_eq!(mask.sample(4, 2).unwrap(), 65535);
    for c in original.tile_coords() {
        assert_eq!(
            original.tile_version(*c),
            doc.layers()[0].pixels().tile_version(*c)
        );
    }
    let archive = doc.to_archive().unwrap();
    let reopened = strata_core::Document::from_archive(archive).unwrap();
    assert_eq!(
        reopened.layers()[0].mask().unwrap().sample(1, 3).unwrap(),
        0
    );
    run(&mut e, "edit.undo", json!({}));
    assert_eq!(
        e.document().unwrap().layers()[0]
            .mask()
            .unwrap()
            .sample(1, 3)
            .unwrap(),
        65535
    );
    run(&mut e, "edit.redo", json!({}));
    assert_eq!(
        e.document().unwrap().layers()[0]
            .mask()
            .unwrap()
            .sample(1, 3)
            .unwrap(),
        0
    );
}
#[test]
fn cross_tile_stroke_changes_only_touched_blocks_and_rejects_invalid_or_locked_targets() {
    let mut e = Editor::new();
    run(
        &mut e,
        "file.new",
        json!({"width":520,"height":16,"bit_depth":16,"background":[32768,32769,50001,65535]}),
    );
    let untouched = e.document().unwrap().layers()[0]
        .pixels()
        .tile_version(TileCoord::new(2, 0).unwrap());
    let mut p = stroke();
    p["points"] = json!([{"x":250.5,"y":8.5},{"x":265.5,"y":8.5}]);
    p["size"] = json!(4);
    run(&mut e, "paint.stroke", p);
    assert_ne!(rgba(&e, 255, 8), [32768, 32769, 50001, 65535]);
    assert_ne!(rgba(&e, 256, 8), [32768, 32769, 50001, 65535]);
    assert_eq!(
        e.document().unwrap().layers()[0]
            .pixels()
            .tile_version(TileCoord::new(2, 0).unwrap()),
        untouched
    );
    for change in [
        json!({"size":0}),
        json!({"points":[]}),
        json!({"flow":2}),
        json!({"spacing":0}),
        json!({"target":"mask"}),
        json!({"points":[{"x":1e12,"y":0},{"x":-1e12,"y":0}]}),
    ] {
        let mut p = stroke();
        p.as_object_mut()
            .unwrap()
            .extend(change.as_object().unwrap().clone());
        let revision = e.revision();
        assert!(
            e.execute(e.request(Source::User), "paint.stroke", p, |_| Ok(()))
                .is_err()
        );
        assert_eq!(e.revision(), revision);
    }
    run(
        &mut e,
        "layer.set_locks",
        json!({"layer":{"name":"Background"},"locks":{"pixels":true}}),
    );
    assert!(
        e.execute(
            e.request(Source::User),
            "paint.stroke",
            stroke(),
            |_| Ok(())
        )
        .is_err()
    );
}
#[test]
fn preparation_cancels_mid_stroke_and_low_budget_failure_never_changes_document() {
    let pool = MemoryPool::new(MemoryLimits {
        cpu_bytes: 64 * 1024 * 1024,
        gpu_bytes: 0,
        disk_bytes: 64 * 1024 * 1024,
    });
    let mut e = Editor::with_memory_pool(pool.clone());
    run(
        &mut e,
        "file.new",
        json!({"width":2048,"height":2048,"bit_depth":16}),
    );
    let revision = e.revision();
    let snapshot = e.clone();
    let request = e.request(Source::User);
    let worker = std::thread::spawn(move || {
        let mut p = stroke();
        p["size"] = json!(4096);
        p["points"] = json!([{"x":1024.5,"y":1024.5}]);
        snapshot.prepare(request, "paint.stroke", p)
    });
    let start = std::time::Instant::now();
    while pool.stats().transient_bytes < 5 * 1024 * 1024
        && start.elapsed() < std::time::Duration::from_secs(2)
    {
        std::thread::yield_now();
    }
    e.cancel_pending_work();
    assert!(matches!(
        worker.join().unwrap(),
        Err(strata_core::commands::CommandError::Expired)
    ));
    assert_eq!(e.revision(), revision);
    assert_eq!(e.document().unwrap().layers()[0].pixels().tile_count(), 0);
    let mut limited = Editor::with_memory_pool(MemoryPool::new(MemoryLimits {
        cpu_bytes: 1024 * 1024,
        gpu_bytes: 0,
        disk_bytes: 0,
    }));
    run(
        &mut limited,
        "file.new",
        json!({"width":16,"height":16,"bit_depth":16}),
    );
    let revision = limited.revision();
    assert!(
        limited
            .execute(
                limited.request(Source::User),
                "paint.stroke",
                stroke(),
                |_| Ok(())
            )
            .is_err()
    );
    assert_eq!(limited.revision(), revision);
}

#[test]
fn negative_canvas_stroke_survives_real_project_and_recovery_roundtrips() {
    let initial = editor(16, [32768, 32769, 50001, 65535]);
    let mut archive = initial.document().unwrap().to_archive().unwrap();
    archive.metadata["frame"]["x"] = json!(-129);
    archive.metadata["frame"]["y"] = json!(-7);
    for layer in archive.metadata["layers"].as_array_mut().unwrap() {
        layer["x"] = json!(-129);
        layer["y"] = json!(-7);
    }
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("negative.strata");
    strata_io::project::write_project(&input, &archive, false, false).unwrap();
    let mut e = Editor::new();
    let read = e.grant_read(&input, Source::User).unwrap();
    run(&mut e, "file.open", json!({"resource":read}));
    let original = e.document().unwrap().export_pixels().unwrap();
    let mut p = stroke();
    p["points"] = json!([{"x":-120.5,"y":-0.5}]);
    run(&mut e, "paint.stroke", p);
    assert_eq!(rgba(&e, 8, 6), [65535, 65535, 65535, 65535]);
    assert_eq!(rgba(&e, 0, 0), [32768, 32769, 50001, 65535]);
    let after = e.document().unwrap().export_pixels().unwrap();
    let store = strata_io::recovery::RecoveryStore::new(&root.path().join("recovery")).unwrap();
    let recovery = e.checkpoint_recovery(&store, "painted").unwrap().unwrap();
    let mut restored = Editor::new();
    let read = restored
        .grant_read(&store.path(&recovery.session_id).unwrap(), Source::User)
        .unwrap();
    run(&mut restored, "file.recover", json!({"resource":read}));
    assert!(restored.document().unwrap().export_pixels().unwrap() == after);
    assert!(restored.dirty());
    let output = root.path().join("painted.strata");
    let write = e.grant_write(&output, false, Source::User).unwrap();
    run(&mut e, "file.save_as", json!({"resource":write}));
    let mut reopened = Editor::new();
    let read = reopened.grant_read(&output, Source::User).unwrap();
    run(&mut reopened, "file.open", json!({"resource":read}));
    assert!(reopened.document().unwrap().export_pixels().unwrap() == after);
    run(&mut e, "edit.undo", json!({}));
    assert!(e.document().unwrap().export_pixels().unwrap() == original);
}

#[test]
fn mask_eraser_uses_background_value_and_parent_pixel_lock_blocks_brush_targets() {
    let mut e = editor(16, [32768, 32769, 50001, 65535]);
    run(
        &mut e,
        "layer.mask.add",
        json!({"layer":{"name":"Background"},"source":"black"}),
    );
    let mut p = stroke();
    p["target"] = json!("mask");
    p["value"] = json!(1);
    run(&mut e, "paint.erase", p.clone());
    assert_eq!(
        e.document().unwrap().layers()[0]
            .mask()
            .unwrap()
            .sample(8, 8)
            .unwrap(),
        65535
    );
    run(
        &mut e,
        "layer.group",
        json!({"layers":[{"name":"Background"}],"name":"parent"}),
    );
    run(
        &mut e,
        "layer.set_locks",
        json!({"layer":{"name":"parent"},"locks":{"pixels":true}}),
    );
    let revision = e.revision();
    for params in [p, stroke()] {
        assert!(
            e.execute(e.request(Source::User), "paint.stroke", params, |_| Ok(()))
                .is_err()
        );
        assert_eq!(e.revision(), revision);
    }
}
