use serde_json::{Value, json};
use strata_core::{ClipboardImage, Document, Editor, Source};
use strata_pixels::{PixelBuffer, PixelValue, memory::MemoryPool};
fn run(e: &mut Editor, id: &str, p: Value) -> Value {
    e.execute(e.request(Source::User), id, p, |_| Ok(()))
        .unwrap()
}
fn fixture() -> Editor {
    let mut e = Editor::new();
    run(
        &mut e,
        "file.new",
        json!({"width":32,"height":16,"bit_depth":16,"working_space":"display_p3"}),
    );
    let pixels = (0..256)
        .flat_map(|i| {
            [
                10000 + (i % 16) as u16 * 2000,
                20000,
                50001,
                if i % 3 == 0 { 0 } else { 32768 },
            ]
        })
        .collect();
    let image = ClipboardImage::new(
        PixelBuffer::Rgba16(pixels),
        16,
        16,
        0,
        0,
        e.document().unwrap().working_icc().to_vec(),
        &MemoryPool::global(),
    )
    .unwrap();
    let token = e.grant_clipboard_image(image, Source::User).unwrap();
    run(&mut e, "edit.paste_in_place", json!({"resource":token}));
    e
}
fn raw(e: &Editor, x: i64, y: i64) -> [u16; 4] {
    let layer = e.document().unwrap().layers().last().unwrap();
    match layer
        .pixels()
        .pixel(x - layer.info().x, y - layer.info().y)
        .unwrap()
    {
        PixelValue::Rgba16(v) => v,
        _ => panic!(),
    }
}
fn p(e: &Editor) -> Value {
    json!({"layer":{"id":e.snapshot().active_layer},"scope":"whole_layer","geometry":{"type":"components","translation":[40,3]},"interpolation":"bilinear"})
}
#[test]
fn translating_whole_layer_preserves_off_artboard_samples_masks_and_one_history() {
    let mut e = fixture();
    let target = e.snapshot().active_layer;
    run(
        &mut e,
        "select.marquee",
        json!({"x":2,"y":2,"width":4,"height":4}),
    );
    run(
        &mut e,
        "layer.mask.add",
        json!({"layer":{"id":target},"source":"from_selection"}),
    );
    let before = e.document().unwrap().to_archive().unwrap();
    let mask = e
        .document()
        .unwrap()
        .layers()
        .last()
        .unwrap()
        .mask()
        .unwrap()
        .clone();
    let original = raw(&e, 5, 7);
    let history = e.snapshot().history.len();
    let params = p(&e);
    let result = run(&mut e, "transform.apply", params);
    assert_eq!(result["scope"], "whole_layer");
    assert_eq!(raw(&e, 45, 10), original);
    assert_eq!(raw(&e, 5, 7), [0; 4]);
    assert_eq!(e.snapshot().history.len(), history + 1);
    let layer = e.document().unwrap().layers().last().unwrap();
    assert_eq!([layer.info().x, layer.info().y], [40, 3]);
    let anchor = layer.info().mask.as_ref().unwrap();
    assert_eq!(
        layer
            .mask()
            .unwrap()
            .sample(
                44 + anchor.anchor_x - layer.info().x,
                7 + anchor.anchor_y - layer.info().y
            )
            .unwrap(),
        mask.sample(4, 4).unwrap()
    );
    assert_eq!(
        e.document()
            .unwrap()
            .selection()
            .unwrap()
            .sample(4, 4)
            .unwrap(),
        65535
    );
    let reopened = Document::from_archive(e.document().unwrap().to_archive().unwrap()).unwrap();
    assert!(reopened.to_archive().unwrap() == e.document().unwrap().to_archive().unwrap());
    run(&mut e, "edit.undo", json!({}));
    assert!(e.document().unwrap().to_archive().unwrap() == before);
}
#[test]
fn integer_moves_keep_tile_versions_and_quarter_turns_at_large_offsets_keep_codes() {
    let mut e = fixture();
    let target = e.snapshot().active_layer;
    run(
        &mut e,
        "layer.set_position",
        json!({"layer":{"id":target},"x":900000000000i64,"y":-800000000000i64}),
    );
    let before = e.clone();
    let coord = strata_pixels::TileCoord::new(0, 0).unwrap();
    let version = e
        .document()
        .unwrap()
        .layers()
        .last()
        .unwrap()
        .pixels()
        .tile_version(coord);
    let params = json!({"layer":{"id":target},"scope":"whole_layer","geometry":{"type":"components","translation":[13,-29]}});
    run(&mut e, "transform.apply", params);
    assert_eq!(
        e.document()
            .unwrap()
            .layers()
            .last()
            .unwrap()
            .pixels()
            .tile_version(coord),
        version
    );
    assert_eq!(
        raw(&e, 900000000018, -800000000022),
        raw(&before, 900000000005, -799999999993)
    );
    run(&mut e, "edit.undo", json!({}));
    for rotation in [90, -270, 450] {
        let mut turned = e.clone();
        run(
            &mut turned,
            "transform.apply",
            json!({"layer":{"id":target},"scope":"whole_layer","geometry":{"type":"components","rotation_degrees":rotation,"pivot":[900000000000i64,-800000000000i64]}}),
        );
        assert_eq!(
            [
                turned.document().unwrap().layers().last().unwrap().info().x,
                turned.document().unwrap().layers().last().unwrap().info().y
            ],
            [899999999984, -800000000000]
        );
        for y in 0..16 {
            for x in 0..16 {
                assert_eq!(
                    raw(&turned, 900000000000 - y - 1, -800000000000 + x),
                    raw(&e, 900000000000 + x, -800000000000 + y)
                );
            }
        }
    }
}
#[test]
fn bounds_query_is_readonly_rejects_hidden_layers_and_empty_transform_is_atomic() {
    let mut e = fixture();
    let layer = e.snapshot().active_layer;
    let snapshot = serde_json::to_value(e.snapshot()).unwrap();
    let answer = run(&mut e, "layer.pixel_bounds", json!({"layer":{"id":layer}}));
    assert_eq!(
        answer["bounds"],
        json!({"x":0,"y":0,"width":16,"height":16})
    );
    assert_eq!(serde_json::to_value(e.snapshot()).unwrap(), snapshot);
    run(
        &mut e,
        "layer.set_visibility",
        json!({"layer":{"id":layer},"visible":false}),
    );
    assert!(
        e.execute(
            e.request(Source::User),
            "layer.pixel_bounds",
            json!({"layer":{"id":layer}}),
            |_| Ok(())
        )
        .is_err()
    );
    run(&mut e, "layer.create", json!({"name":"empty"}));
    let snapshot = serde_json::to_value(e.snapshot()).unwrap();
    let params = p(&e);
    assert!(
        e.execute(e.request(Source::User), "transform.apply", params, |_| Ok(
            ()
        ))
        .is_err()
    );
    assert_eq!(serde_json::to_value(e.snapshot()).unwrap(), snapshot);
}
#[test]
fn four_corners_and_components_produce_the_same_exact_horizontal_flip() {
    let original = fixture();
    let mut a = original.clone();
    let mut b = original.clone();
    let mut pa = p(&a);
    pa["geometry"] = json!({"type":"components","scale":[-1,1],"pivot":[8,8]});
    run(&mut a, "transform.apply", pa);
    let mut pb = p(&b);
    pb["geometry"] = json!({"type":"corners","source":[[0,0],[16,0],[16,16],[0,16]],"destination":[[16,0],[0,0],[0,16],[16,16]]});
    run(&mut b, "transform.apply", pb);
    for y in 0..16 {
        for x in 0..16 {
            assert_eq!(raw(&a, x, y), raw(&original, 15 - x, y));
            assert_eq!(raw(&b, x, y), raw(&a, x, y));
        }
    }
}
#[test]
fn invalid_geometry_locks_and_nonpixel_selection_targets_fail_atomically() {
    let mut e = fixture();
    let before = e.document().unwrap().to_archive().unwrap();
    let history = e.snapshot().history.len();
    for geometry in [
        json!({"type":"matrix","values":[0,0,0,0,0,0,0,0,0]}),
        json!({"type":"matrix","values":[1,0,0,0,1,0,0.2,0,-1]}),
        json!({"type":"components","scale":[0,1]}),
        json!({"type":"components","shear_degrees":[90,0]}),
    ] {
        let mut params = p(&e);
        params["geometry"] = geometry;
        assert!(
            e.execute(e.request(Source::User), "transform.apply", params, |_| Ok(
                ()
            ))
            .is_err()
        );
        assert!(e.document().unwrap().to_archive().unwrap() == before);
        assert_eq!(e.snapshot().history.len(), history);
    }
    let layer = e.snapshot().active_layer;
    run(
        &mut e,
        "layer.set_locks",
        json!({"layer":{"id":layer},"locks":{"position":true}}),
    );
    let revision = e.revision();
    let params = p(&e);
    assert!(
        e.execute(e.request(Source::User), "transform.apply", params, |_| Ok(
            ()
        ))
        .is_err()
    );
    assert_eq!(e.revision(), revision);
    run(
        &mut e,
        "layer.fill.solid",
        json!({"name":"generated","color":[65535,0,0,65535]}),
    );
    let revision = e.revision();
    let mut params = p(&e);
    params["scope"] = json!("selection");
    assert!(
        e.execute(e.request(Source::User), "transform.apply", params, |_| Ok(
            ()
        ))
        .is_err()
    );
    assert_eq!(e.revision(), revision);
}

#[test]
fn moved_disabled_linked_mask_flips_in_canvas_coordinates_and_remains_disabled() {
    let mut e = fixture();
    let target = e.snapshot().active_layer;
    run(
        &mut e,
        "select.marquee",
        json!({"x":2,"y":3,"width":5,"height":4}),
    );
    run(
        &mut e,
        "layer.mask.add",
        json!({"layer":{"id":target},"source":"from_selection"}),
    );
    run(
        &mut e,
        "layer.mask.set_enabled",
        json!({"layer":{"id":target},"enabled":false}),
    );
    run(
        &mut e,
        "layer.set_position",
        json!({"layer":{"id":target},"x":-129,"y":25}),
    );
    let before = e.document().unwrap().to_archive().unwrap();
    run(
        &mut e,
        "transform.apply",
        json!({"layer":{"id":target},"scope":"whole_layer","geometry":{"type":"components","scale":[-1,1],"pivot":[-121,33]}}),
    );
    let layer = e.document().unwrap().layers().last().unwrap();
    let info = layer.info().mask.as_ref().unwrap();
    assert!(!info.enabled);
    for y in 0..16 {
        for x in 0..16 {
            let sample = layer
                .mask()
                .unwrap()
                .sample(
                    -129 + x + info.anchor_x - layer.info().x,
                    25 + y + info.anchor_y - layer.info().y,
                )
                .unwrap();
            assert_eq!(
                sample,
                if (2..7).contains(&(15 - x)) && (3..7).contains(&y) {
                    65535
                } else {
                    0
                }
            );
        }
    }
    run(&mut e, "edit.undo", json!({}));
    assert!(e.document().unwrap().to_archive().unwrap() == before);
}

#[test]
fn budget_failure_and_cancellation_during_resampling_leave_editor_unchanged() {
    use strata_pixels::memory::MemoryLimits;
    let limits = MemoryLimits {
        cpu_bytes: 8 * 1024 * 1024,
        gpu_bytes: 0,
        disk_bytes: 64 * 1024 * 1024,
    };
    let mut limited = Editor::with_memory_pool(MemoryPool::new(limits));
    run(
        &mut limited,
        "file.new",
        json!({"width":16,"height":16,"bit_depth":16,"background":[10000,20000,30000,32768]}),
    );
    let revision = limited.revision();
    let mut params = p(&limited);
    params["geometry"] = json!({"type":"components","rotation_degrees":20});
    assert!(
        limited
            .execute(
                limited.request(Source::User),
                "transform.apply",
                params,
                |_| Ok(())
            )
            .is_err()
    );
    assert_eq!(limited.revision(), revision);
    let pool = MemoryPool::new(MemoryLimits {
        cpu_bytes: 128 * 1024 * 1024,
        ..limits
    });
    let mut e = Editor::with_memory_pool(pool.clone());
    run(
        &mut e,
        "file.new",
        json!({"width":512,"height":512,"bit_depth":16,"background":[10000,20000,30000,32768]}),
    );
    let revision = e.revision();
    let source = e.clone();
    let request = e.request(Source::User);
    let worker = std::thread::spawn(move || {
        source.prepare(request,"transform.apply",json!({"layer":{"name":"Background"},"scope":"whole_layer","geometry":{"type":"components","scale":[2.1,2.1]}}))
    });
    let start = std::time::Instant::now();
    while pool.stats().transient_bytes < 12 * 1024 * 1024
        && start.elapsed() < std::time::Duration::from_secs(3)
    {
        std::thread::yield_now();
    }
    assert!(
        pool.stats().transient_bytes >= 12 * 1024 * 1024,
        "worker must reach actual raster work before cancelling"
    );
    e.cancel_pending_work();
    assert!(worker.join().unwrap().is_err());
    assert_eq!(e.revision(), revision);
    assert!(e.snapshot().history.is_empty());
    assert_eq!(pool.stats().transient_bytes, 0);
}
