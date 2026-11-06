use serde_json::{Value, json};
use strata_core::{ClipboardImage, Document, Editor, LayerKind, Source};
use strata_pixels::{
    PixelBuffer, PixelValue, TileCoord,
    memory::{MemoryKind, MemoryLimits, MemoryPool},
};
fn run(e: &mut Editor, id: &str, p: Value) -> Value {
    e.execute(e.request(Source::User), id, p, |_| Ok(()))
        .unwrap()
}
fn fixture(depth: u8) -> Editor {
    let mut e = Editor::new();
    run(
        &mut e,
        "file.new",
        json!({"width":257,"height":3,"bit_depth":depth,"working_space":"display_p3","resolution_dpi":300}),
    );
    let values = (0..257 * 3)
        .flat_map(|i| {
            if depth == 8 {
                [
                    (i % 256) as u16,
                    ((i * 13 + 11) % 256) as u16,
                    ((i * 31 + 5) % 256) as u16,
                    if i % 5 == 0 { 0 } else { (i % 256) as u16 },
                ]
            } else {
                let codes = [
                    0, 1, 128, 129, 256, 257, 32767, 32768, 32769, 50001, 65406, 65534, 65535,
                ];
                [
                    codes[i % codes.len()],
                    codes[(i + 3) % codes.len()],
                    codes[(i + 7) % codes.len()],
                    if i % 5 == 0 {
                        0
                    } else {
                        codes[(i + 9) % codes.len()]
                    },
                ]
            }
        })
        .collect::<Vec<_>>();
    let p = if depth == 8 {
        PixelBuffer::Rgba8(values.into_iter().map(|v| v as u8).collect())
    } else {
        PixelBuffer::Rgba16(values)
    };
    let image = ClipboardImage::new(
        p,
        257,
        3,
        0,
        0,
        e.document().unwrap().working_icc().to_vec(),
        &e.document().unwrap().memory_pool(),
    )
    .unwrap();
    let token = e.grant_clipboard_image(image, Source::User).unwrap();
    run(&mut e, "edit.paste_in_place", json!({"resource":token}));
    e
}
fn raw(e: &Editor, x: i64, y: i64) -> [u16; 4] {
    let l = &e.document().unwrap().layers()[1];
    match l.pixels().pixel(x, y).unwrap() {
        PixelValue::Rgba8(v) => v.map(u16::from),
        PixelValue::Rgba16(v) => v,
    }
}
#[test]
fn all_eight_bit_codes_expand_exactly_including_hidden_rgb_and_roundtrip_without_flattening() {
    let mut e = fixture(8);
    let id = e.snapshot().active_layer;
    run(
        &mut e,
        "layer.set_position",
        json!({"layer":{"id":id},"x":-19,"y":300}),
    );
    run(
        &mut e,
        "layer.set_visibility",
        json!({"layer":{"id":id},"visible":false}),
    );
    run(
        &mut e,
        "select.marquee",
        json!({"x":0,"y":0,"width":1,"height":1}),
    );
    let original = e.clone();
    let archive = e.document().unwrap().to_archive().unwrap();
    let cursor = e.snapshot().history_cursor;
    let r = run(&mut e, "image.convert_bit_depth", json!({"bit_depth":16}));
    assert_eq!(r["previous_bit_depth"], 8);
    assert_eq!(r["bit_depth"], 16);
    assert_eq!(r["changed"], true);
    assert_eq!(
        e.document().unwrap().working_icc(),
        original.document().unwrap().working_icc()
    );
    assert_eq!(
        e.document().unwrap().frame(),
        original.document().unwrap().frame()
    );
    assert_eq!(e.snapshot().layers, original.snapshot().layers);
    assert_eq!(e.snapshot().history_cursor, cursor + 1);
    for y in 0..3 {
        for x in 0..257 {
            assert_eq!(raw(&e, x, y), raw(&original, x, y).map(|v| v * 257));
        }
    }
    let saved = e.document().unwrap().to_archive().unwrap();
    assert_eq!(
        Document::from_archive(saved.clone())
            .unwrap()
            .to_archive()
            .unwrap(),
        saved
    );
    run(&mut e, "image.convert_bit_depth", json!({"bit_depth":8}));
    for y in 0..3 {
        for x in 0..257 {
            assert_eq!(raw(&e, x, y), raw(&original, x, y));
        }
    }
    run(&mut e, "edit.undo", json!({}));
    run(&mut e, "edit.undo", json!({}));
    assert_eq!(e.document().unwrap().to_archive().unwrap(), archive);
}
#[test]
fn sixteen_bit_quantization_matches_independent_normalized_reference_and_undo_restores_lost_precision()
 {
    let mut e = fixture(16);
    let original = e.clone();
    let archive = e.document().unwrap().to_archive().unwrap();
    run(&mut e, "image.convert_bit_depth", json!({"bit_depth":8}));
    for y in 0..3 {
        for x in 0..257 {
            let before = raw(&original, x, y);
            let expected = before.map(|v| (f64::from(v) / 65535. * 255.).round() as u16);
            let actual = raw(&e, x, y);
            assert_eq!(actual, expected, "{x},{y}");
            assert!((0..4).all(|c| before[c].abs_diff(actual[c] * 257) <= 128));
        }
    }
    assert_eq!(e.document().unwrap().bit_depth(), 8);
    run(&mut e, "edit.undo", json!({}));
    assert_eq!(e.document().unwrap().to_archive().unwrap(), archive);
}
#[test]
fn every_editable_layer_kind_and_all_style_colors_convert_while_references_and_float_settings_survive()
 {
    let mut e = Editor::new();
    run(
        &mut e,
        "file.new",
        json!({"width":16,"height":12,"bit_depth":16,"working_space":"display_p3","background":[10001,30003,50001,32769]}),
    );
    let color = json!([10001, 30003, 50001, 32769]);
    let end = json!([65535, 129, 128, 65535]);
    run(
        &mut e,
        "layer.fill.solid",
        json!({"name":"solid","color":color}),
    );
    run(
        &mut e,
        "layer.set_styles",
        json!({"layer":{"name":"solid"},"styles":{"drop_shadow":{"color":color,"offset_x":2,"offset_y":-1,"blur":1},"stroke":{"color":color,"size":1},"outer_glow":{"color":color,"size":2},"color_overlay":{"color":color,"opacity":0.25},"gradient_overlay":{"start":color,"end":end,"angle":27,"scale":0.7,"opacity":0.5}}}),
    );
    run(
        &mut e,
        "layer.fill.gradient",
        json!({"name":"gradient","start":color,"end":end,"angle":-27,"scale":0.7,"radial":true}),
    );
    run(
        &mut e,
        "layer.group",
        json!({"layers":[{"name":"solid"},{"name":"gradient"}],"name":"fills"}),
    );
    run(
        &mut e,
        "layer.add_adjustment",
        json!({"name":"exposure","type":"exposure","stops":0.5,"mask":"white"}),
    );
    run(&mut e, "layer.add_hue_saturation", json!({"name":"hue"}));
    run(
        &mut e,
        "layer.add_tonal_adjustment",
        json!({"name":"map","adjustment":{"type":"gradient_map","stops":[{"at":0,"color":[0.1,0.2,0.3]},{"at":1,"color":[0.8,0.7,0.6]}]}}),
    );
    run(
        &mut e,
        "layer.add_tonal_adjustment",
        json!({"name":"spatial","adjustment":{"type":"shadows_highlights","shadows":10,"highlights":15,"radius":2}}),
    );
    run(
        &mut e,
        "image.canvas_size",
        json!({"width":20,"height":18,"anchor":"center"}),
    );
    let before = e.document().unwrap().to_archive().unwrap();
    let expected: Vec<_> = e
        .document()
        .unwrap()
        .layers()
        .iter()
        .map(|l| {
            let mut v = serde_json::to_value(l.info()).unwrap();
            for path in [
                "/kind/color",
                "/kind/start",
                "/kind/end",
                "/styles/drop_shadow/color",
                "/styles/stroke/color",
                "/styles/outer_glow/color",
                "/styles/color_overlay/color",
                "/styles/gradient_overlay/start",
                "/styles/gradient_overlay/end",
            ] {
                if let Some(color) = v.pointer_mut(path) {
                    for c in color.as_array_mut().unwrap() {
                        *c = json!((c.as_u64().unwrap() as f64 / 65535. * 255.).round() as u16);
                    }
                }
            }
            v
        })
        .collect();
    run(&mut e, "image.convert_bit_depth", json!({"bit_depth":8}));
    let doc = e.document().unwrap();
    assert_eq!(
        doc.layers()
            .iter()
            .map(|l| serde_json::to_value(l.info()).unwrap())
            .collect::<Vec<_>>(),
        expected
    );
    assert!(
        doc.layers()
            .iter()
            .all(|l| l.pixels().format() == strata_pixels::PixelFormat::Rgba8)
    );
    assert!(
        doc.layers()
            .iter()
            .any(|l| matches!(l.info().kind, LayerKind::Group))
    );
    assert!(
        doc.layers()
            .iter()
            .any(|l| l.info().generated_size.is_some())
    );
    assert!(
        doc.layers()
            .iter()
            .any(|l| l.info().spatial_bounds.is_some())
    );
    let saved = doc.to_archive().unwrap();
    for key in [
        "working_icc",
        "source_icc",
        "profile_name",
        "profile_fallback",
        "frame",
        "resolution_dpi",
        "preset",
    ] {
        assert_eq!(saved.metadata[key], before.metadata[key], "{key}");
    }
    assert_eq!(
        Document::from_archive(saved.clone())
            .unwrap()
            .to_archive()
            .unwrap(),
        saved
    );
    run(&mut e, "edit.undo", json!({}));
    assert_eq!(e.document().unwrap().to_archive().unwrap(), before);
}
#[test]
fn soft_disabled_linked_group_masks_and_both_selection_planes_keep_exact_versions_and_samples() {
    let mut e = fixture(16);
    let id = e.snapshot().active_layer;
    run(
        &mut e,
        "select.marquee",
        json!({"x":2,"y":0,"width":250,"height":2}),
    );
    run(&mut e, "select.feather", json!({"radius":1.2}));
    run(
        &mut e,
        "layer.mask.add",
        json!({"layer":{"id":id},"source":"from_selection"}),
    );
    run(
        &mut e,
        "layer.mask.set_enabled",
        json!({"layer":{"id":id},"enabled":false}),
    );
    run(
        &mut e,
        "layer.set_position",
        json!({"layer":{"id":id},"x":-17,"y":300}),
    );
    let g = run(
        &mut e,
        "layer.group",
        json!({"layers":[{"id":id}],"name":"masked"}),
    )["layer_id"]
        .clone();
    run(
        &mut e,
        "layer.mask.add",
        json!({"layer":{"id":g},"source":"white"}),
    );
    run(&mut e, "select.none", json!({}));
    run(&mut e, "select.all", json!({}));
    let original = e.clone();
    run(&mut e, "image.convert_bit_depth", json!({"bit_depth":8}));
    for (before, after) in original
        .document()
        .unwrap()
        .layers()
        .iter()
        .zip(e.document().unwrap().layers())
    {
        assert_eq!(before.info().mask, after.info().mask);
        if let (Some(before), Some(after)) = (before.mask(), after.mask()) {
            assert_eq!(before.layout(), after.layout());
            assert_eq!(before.tiles().len(), after.tiles().len());
            for ((coord, tile), (after_coord, after_tile)) in before.tiles().zip(after.tiles()) {
                assert_eq!(coord, after_coord);
                assert_eq!(before.tile_version(*coord), after.tile_version(*coord));
                assert_eq!(*tile.unwrap(), *after_tile.unwrap());
            }
        }
    }
    for (before, after) in [
        (
            original.document().unwrap().selection().unwrap(),
            e.document().unwrap().selection().unwrap(),
        ),
        (
            original.document().unwrap().previous_selection().unwrap(),
            e.document().unwrap().previous_selection().unwrap(),
        ),
    ] {
        assert_eq!(before.layout(), after.layout());
        for y in 0..3 {
            for x in 0..257 {
                assert_eq!(before.sample(x, y).unwrap(), after.sample(x, y).unwrap());
            }
        }
        for (coord, _) in before.tiles() {
            assert_eq!(before.tile_version(*coord), after.tile_version(*coord));
        }
    }
}
#[test]
fn exact_numeric_schema_noop_concurrent_layer_selection_and_lock_policy_are_enforced() {
    let spec = strata_core::commands::catalog()
        .iter()
        .find(|s| s.id == "image.convert_bit_depth")
        .unwrap();
    let schema = serde_json::to_value(&spec.params).unwrap();
    assert_eq!(schema["properties"]["bit_depth"]["enum"], json!([8, 16]));
    assert_eq!(schema["additionalProperties"], false);
    assert!(!spec.batchable);
    assert!(spec.effects.contains(&"pixels.replace"));
    let mut e = fixture(16);
    let before = e.document().unwrap().to_archive().unwrap();
    for p in [
        json!({"bit_depth":9}),
        json!({"bit_depth":32}),
        json!({"bit_depth":"8"}),
        json!({"bit_depth":8.0}),
        json!({"bit_depth":8,"dither":true}),
        json!({}),
    ] {
        assert!(
            e.execute(
                e.request(Source::User),
                "image.convert_bit_depth",
                p,
                |_| Ok(())
            )
            .is_err()
        );
        assert_eq!(e.document().unwrap().to_archive().unwrap(), before);
    }
    let prepared = e
        .prepare(
            e.request(Source::User),
            "image.convert_bit_depth",
            json!({"bit_depth":16}),
        )
        .unwrap();
    run(
        &mut e,
        "layer.select",
        json!({"layer":{"name":"Background"}}),
    );
    let active = e.snapshot().active_layer;
    let revision = e.revision();
    assert_eq!(e.commit(prepared).unwrap()["changed"], false);
    assert_eq!(e.snapshot().active_layer, active);
    assert_eq!(e.revision(), revision);
    for lock in ["all", "pixels", "transparency"] {
        let mut target = e.clone();
        run(
            &mut target,
            "layer.set_locks",
            json!({"layer":{"name":"Background"},"locks":{lock:true}}),
        );
        let before = target.document().unwrap().to_archive().unwrap();
        let revision = target.revision();
        assert_eq!(
            run(
                &mut target,
                "image.convert_bit_depth",
                json!({"bit_depth":16})
            )["changed"],
            false
        );
        assert!(
            target
                .execute(
                    target.request(Source::User),
                    "image.convert_bit_depth",
                    json!({"bit_depth":8}),
                    |_| Ok(())
                )
                .is_err()
        );
        assert_eq!(target.revision(), revision);
        assert_eq!(target.document().unwrap().to_archive().unwrap(), before);
    }
    run(
        &mut e,
        "layer.set_locks",
        json!({"layer":{"name":"Background"},"locks":{"position":true}}),
    );
    run(&mut e, "image.convert_bit_depth", json!({"bit_depth":8}));
    assert_eq!(e.document().unwrap().bit_depth(), 8);
}
#[test]
fn target_frame_and_frozen_reference_byte_limits_reject_before_expensive_conversion() {
    for reference in ["frame", "generated", "spatial"] {
        let mut e = Editor::new();
        run(
            &mut e,
            "file.new",
            json!({"width":9000,"height":9000,"bit_depth":8}),
        );
        if reference == "generated" {
            run(
                &mut e,
                "layer.fill.solid",
                json!({"name":"large","color":[100,120,140,255]}),
            );
        }
        if reference == "spatial" {
            run(
                &mut e,
                "layer.add_tonal_adjustment",
                json!({"name":"large","adjustment":{"type":"shadows_highlights","shadows":1,"radius":1}}),
            );
        }
        if reference != "frame" {
            run(
                &mut e,
                "image.canvas_size",
                json!({"width":16,"height":16,"anchor":"top_left"}),
            );
        }
        let before = e.document().unwrap().to_archive().unwrap();
        let revision = e.revision();
        assert!(
            e.execute(
                e.request(Source::User),
                "image.convert_bit_depth",
                json!({"bit_depth":16}),
                |_| Ok(())
            )
            .is_err()
        );
        assert_eq!(e.revision(), revision);
        assert_eq!(e.document().unwrap().to_archive().unwrap(), before);
    }
}
#[test]
fn budget_pressure_and_cancellation_during_tile_conversion_keep_original_depth_and_exact_source() {
    for pressure in [true, false] {
        let pool = MemoryPool::new(MemoryLimits {
            cpu_bytes: 128 * 1024 * 1024,
            gpu_bytes: 0,
            disk_bytes: 512 * 1024 * 1024,
        });
        let mut e = Editor::with_memory_pool(pool.clone());
        run(
            &mut e,
            "file.new",
            json!({"width":2048,"height":2048,"bit_depth":16,"background":[10001,30003,50001,32769]}),
        );
        let version = e.document().unwrap().layers()[0]
            .pixels()
            .tile_version(TileCoord::new(0, 0).unwrap());
        let revision = e.revision();
        if pressure {
            pool.trim_sources().unwrap();
            let stats = pool.stats();
            let held = 128 * 1024 * 1024 - stats.cpu_bytes - 1024 * 1024;
            let _held = pool.reserve(MemoryKind::Transient, held).unwrap();
            assert!(
                e.execute(
                    e.request(Source::User),
                    "image.convert_bit_depth",
                    json!({"bit_depth":8}),
                    |_| Ok(())
                )
                .is_err()
            );
        } else {
            let source = e.clone();
            let request = e.request(Source::User);
            let worker = std::thread::spawn(move || {
                source.prepare(request, "image.convert_bit_depth", json!({"bit_depth":8}))
            });
            let start = std::time::Instant::now();
            while pool.stats().transient_bytes < 2 * 1024 * 1024
                && start.elapsed() < std::time::Duration::from_secs(5)
            {
                std::thread::yield_now();
            }
            assert!(pool.stats().transient_bytes >= 2 * 1024 * 1024);
            e.cancel_pending_work();
            assert!(worker.join().unwrap().is_err());
        }
        assert_eq!(e.document().unwrap().bit_depth(), 16);
        assert_eq!(e.revision(), revision);
        assert_eq!(
            e.document().unwrap().layers()[0]
                .pixels()
                .tile_version(TileCoord::new(0, 0).unwrap()),
            version
        );
        assert_eq!(
            e.document().unwrap().layers()[0]
                .pixels()
                .pixel(0, 0)
                .unwrap(),
            PixelValue::Rgba16([10001, 30003, 50001, 32769])
        );
        assert!(e.snapshot().history.is_empty());
        assert_eq!(pool.stats().transient_bytes, 0);
    }
}
