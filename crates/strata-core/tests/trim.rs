use serde_json::{Value, json};
use strata_core::{ClipboardImage, Document, Editor, Source};
use strata_pixels::{PixelBuffer, TileCoord};
fn run(e: &mut Editor, id: &str, p: Value) -> Value {
    e.execute(e.request(Source::User), id, p, |_| Ok(()))
        .unwrap()
}
fn fixture(depth: u8) -> Editor {
    let mut e = Editor::new();
    run(
        &mut e,
        "file.new",
        json!({"width":300,"height":40,"bit_depth":depth,"working_space":"display_p3"}),
    );
    let pixels = (0..300 * 40)
        .flat_map(|i| {
            let (x, y) = (i % 300, i / 300);
            [
                10001,
                30003,
                50001,
                if (253..271).contains(&x) && (7..29).contains(&y) {
                    if depth == 16 { 1 } else { 257 }
                } else {
                    0
                },
            ]
        })
        .collect::<Vec<u16>>();
    let buffer = if depth == 16 {
        PixelBuffer::Rgba16(pixels)
    } else {
        PixelBuffer::Rgba8(pixels.into_iter().map(|v| (v / 257) as u8).collect())
    };
    let image = ClipboardImage::new(
        buffer,
        300,
        40,
        0,
        0,
        e.document().unwrap().working_icc().to_vec(),
        &e.document().unwrap().memory_pool(),
    )
    .unwrap();
    let r = e.grant_clipboard_image(image, Source::User).unwrap();
    run(&mut e, "edit.paste_in_place", json!({"resource":r}));
    e
}
#[test]
fn alpha_trim_preserves_tiny_sixteen_bit_alpha_ignores_hidden_rgb_and_keeps_sources() {
    for depth in [8, 16] {
        let mut e = fixture(depth);
        let target = e.snapshot().active_layer;
        run(&mut e, "select.all", json!({}));
        run(
            &mut e,
            "layer.set_locks",
            json!({"layer":{"id":target},"locks":{"all":true}}),
        );
        let before = e.document().unwrap().to_archive().unwrap();
        let version = e
            .document()
            .unwrap()
            .layers()
            .last()
            .unwrap()
            .pixels()
            .tile_version(TileCoord::new(1, 0).unwrap());
        let result = run(&mut e, "image.trim", json!({}));
        assert_eq!(result["changed"], true);
        assert_eq!(
            result["frame"],
            json!({"name":"Artboard 1","x":253,"y":7,"width":18,"height":22})
        );
        let doc = e.document().unwrap();
        assert_eq!(
            doc.to_archive()
                .unwrap()
                .tiles
                .iter()
                .filter(|t| !t.layer.contains("selection"))
                .collect::<Vec<_>>(),
            before
                .tiles
                .iter()
                .filter(|t| !t.layer.contains("selection"))
                .collect::<Vec<_>>()
        );
        assert_eq!(
            doc.layers()
                .last()
                .unwrap()
                .pixels()
                .tile_version(TileCoord::new(1, 0).unwrap()),
            version
        );
        assert_eq!(doc.selection().unwrap().layout().bounds.x, 253);
        let archive = doc.to_archive().unwrap();
        assert_eq!(
            Document::from_archive(archive.clone())
                .unwrap()
                .to_archive()
                .unwrap(),
            archive
        );
        run(&mut e, "edit.undo", json!({}));
        assert_eq!(e.document().unwrap().to_archive().unwrap(), before);
    }
}
#[test]
fn selected_edges_and_nonzero_negative_frame_are_explicit_and_tight_noop_keeps_history() {
    let original = fixture(16);
    for (p, expected) in [
        (
            json!({"left":true,"right":false,"top":false,"bottom":false}),
            [253, 0, 47, 40],
        ),
        (
            json!({"left":false,"right":true,"top":true,"bottom":false}),
            [0, 7, 271, 33],
        ),
    ] {
        let mut e = original.clone();
        let result = run(&mut e, "image.trim", p);
        let f = &result["frame"];
        assert_eq!(
            [
                f["x"].as_i64().unwrap(),
                f["y"].as_i64().unwrap(),
                f["width"].as_i64().unwrap(),
                f["height"].as_i64().unwrap()
            ],
            expected
        );
    }
    let mut e = original;
    let target = e.snapshot().active_layer;
    run(
        &mut e,
        "layer.set_position",
        json!({"layer":{"id":target},"x":-129,"y":25}),
    );
    run(
        &mut e,
        "image.crop",
        json!({"rect":{"x":-129,"y":25,"width":300,"height":40},"pixels":"preserve"}),
    );
    run(&mut e, "image.trim", json!({}));
    let f = e.document().unwrap().frame();
    assert_eq!((f.x, f.y, f.width, f.height), (124, 32, 18, 22));
    let before = e.document().unwrap().to_archive().unwrap();
    let revision = e.revision();
    let history = e.snapshot().history.len();
    let result = run(&mut e, "image.trim", json!({}));
    assert_eq!(result["changed"], false);
    assert_eq!(e.revision(), revision);
    assert_eq!(e.snapshot().history.len(), history);
    assert_eq!(e.document().unwrap().to_archive().unwrap(), before);
}
#[test]
fn visible_groups_masks_clipping_and_styles_define_trim_instead_of_storage_bounds() {
    let mut e = Editor::new();
    run(
        &mut e,
        "file.new",
        json!({"width":32,"height":24,"bit_depth":16}),
    );
    run(
        &mut e,
        "select.marquee",
        json!({"x":8,"y":7,"width":8,"height":6}),
    );
    run(
        &mut e,
        "layer.fill.solid",
        json!({"name":"masked fill","color":[50001,30003,10001,65535]}),
    );
    run(
        &mut e,
        "layer.mask.add",
        json!({"layer":{"name":"masked fill"},"source":"from_selection"}),
    );
    let group = run(
        &mut e,
        "layer.group",
        json!({"layers":[{"name":"masked fill"}],"name":"group"}),
    )["layer_id"]
        .clone();
    run(
        &mut e,
        "layer.set_styles",
        json!({"layer":{"id":group},"styles":{"drop_shadow":{"color":[0,0,0,65535],"blur":0,"offset_x":4,"offset_y":2}}}),
    );
    run(
        &mut e,
        "layer.select",
        json!({"layer":{"name":"Background"}}),
    );
    let hidden = run(
        &mut e,
        "layer.create",
        json!({"name":"hidden full","fill":[10001,30003,50001,65535]}),
    )["layer_id"]
        .clone();
    let hidden_group = run(
        &mut e,
        "layer.group",
        json!({"layers":[{"id":hidden}],"name":"hidden group"}),
    )["layer_id"]
        .clone();
    run(
        &mut e,
        "layer.set_visibility",
        json!({"layer":{"id":hidden_group},"visible":false}),
    );
    run(&mut e, "select.none", json!({}));
    let before = e.document().unwrap().to_archive().unwrap();
    let result = run(&mut e, "image.trim", json!({}));
    assert_eq!(
        result["frame"],
        json!({"name":"Artboard 1","x":8,"y":7,"width":12,"height":8})
    );
    assert_eq!(
        e.document().unwrap().to_archive().unwrap().tiles,
        before.tiles
    );
    assert!(matches!(
        e.document()
            .unwrap()
            .layers()
            .iter()
            .find(|l| l.info().name == "masked fill")
            .unwrap()
            .info()
            .kind,
        strata_core::LayerKind::Solid { .. }
    ));
    run(&mut e, "edit.undo", json!({}));
    assert_eq!(e.document().unwrap().to_archive().unwrap(), before);
}
#[test]
fn transparent_composite_bad_fields_no_sides_and_stale_work_are_atomic() {
    let mut e = Editor::new();
    run(
        &mut e,
        "file.new",
        json!({"width":8,"height":8,"bit_depth":16}),
    );
    let before = e.document().unwrap().to_archive().unwrap();
    assert!(
        e.execute(e.request(Source::User), "image.trim", json!({}), |_| Ok(()))
            .is_err()
    );
    assert_eq!(e.document().unwrap().to_archive().unwrap(), before);
    let mut e = fixture(16);
    let before = e.document().unwrap().to_archive().unwrap();
    for p in [
        json!({"top":false,"bottom":false,"left":false,"right":false}),
        json!({"alpha_threshold":0.01}),
    ] {
        assert!(
            e.execute(e.request(Source::User), "image.trim", p, |_| Ok(()))
                .is_err()
        );
        assert_eq!(e.document().unwrap().to_archive().unwrap(), before);
    }
    let request = e.request(Source::User);
    e.cancel_pending_work();
    assert!(e.prepare(request, "image.trim", json!({})).is_err());
    assert_eq!(e.document().unwrap().to_archive().unwrap(), before);
}
#[test]
fn actual_composite_work_cancels_and_budget_pressure_cannot_publish_partial_frame() {
    use strata_pixels::memory::{MemoryKind, MemoryLimits, MemoryPool};
    for pressure in [true, false] {
        let pool = MemoryPool::new(MemoryLimits {
            cpu_bytes: 32 * 1024 * 1024,
            gpu_bytes: 0,
            disk_bytes: 128 * 1024 * 1024,
        });
        let mut e = Editor::with_memory_pool(pool.clone());
        run(
            &mut e,
            "file.new",
            json!({"width":1024,"height":1024,"bit_depth":16,"background":[10001,30003,50001,32769]}),
        );
        let revision = e.revision();
        let frame = e.document().unwrap().frame().clone();
        if pressure {
            let _pressure = pool
                .reserve(MemoryKind::Transient, 30 * 1024 * 1024)
                .unwrap();
            assert!(
                e.execute(e.request(Source::User), "image.trim", json!({}), |_| Ok(()))
                    .is_err()
            );
        } else {
            let source = e.clone();
            let request = e.request(Source::User);
            let worker =
                std::thread::spawn(move || source.prepare(request, "image.trim", json!({})));
            let start = std::time::Instant::now();
            while pool.stats().transient_bytes < 3 * 1024 * 1024
                && start.elapsed() < std::time::Duration::from_secs(3)
            {
                std::thread::yield_now();
            }
            assert!(pool.stats().transient_bytes >= 3 * 1024 * 1024);
            e.cancel_pending_work();
            assert!(worker.join().unwrap().is_err());
        }
        assert_eq!(e.revision(), revision);
        assert!(e.snapshot().history.is_empty());
        assert_eq!(e.document().unwrap().frame(), &frame);
        assert_eq!(pool.stats().transient_bytes, 0);
    }
}
#[test]
fn noop_candidate_cannot_revert_a_newer_layer_selection() {
    let mut e = fixture(16);
    run(&mut e, "image.trim", json!({}));
    let prepared = e
        .prepare(e.request(Source::User), "image.trim", json!({}))
        .unwrap();
    let background = e.document().unwrap().layers()[0].id();
    run(&mut e, "layer.select", json!({"layer":{"id":background}}));
    let revision = e.revision();
    let result = e.commit(prepared).unwrap();
    assert_eq!(result["changed"], false);
    assert_eq!(e.snapshot().active_layer, Some(background));
    assert_eq!(e.revision(), revision);
}
