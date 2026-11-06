use serde_json::{Value, json};
use strata_core::{ClipboardImage, Document, Editor, LayerKind, Source};
use strata_pixels::{PixelBuffer, PixelValue};
fn run(e: &mut Editor, id: &str, p: Value) -> Value {
    e.execute(e.request(Source::User), id, p, |_| Ok(()))
        .unwrap()
}
fn map(op: &str, x: i64, y: i64, w: i64, h: i64) -> [i64; 2] {
    match op {
        "rotate_90_cw" => [h - 1 - y, x],
        "rotate_90_ccw" => [y, w - 1 - x],
        "rotate_180" => [w - 1 - x, h - 1 - y],
        "flip_horizontal" => [w - 1 - x, y],
        "flip_vertical" => [x, h - 1 - y],
        _ => panic!(),
    }
}
const OPS: [&str; 5] = [
    "rotate_90_cw",
    "rotate_90_ccw",
    "rotate_180",
    "flip_horizontal",
    "flip_vertical",
];
fn raw(layer: &strata_core::Layer, x: i64, y: i64) -> [u16; 4] {
    match layer
        .pixels()
        .pixel(x - layer.info().x, y - layer.info().y)
        .unwrap()
    {
        PixelValue::Rgba8(v) => v.map(u16::from),
        PixelValue::Rgba16(v) => v,
    }
}
fn mask(layer: &strata_core::Layer, x: i64, y: i64) -> u16 {
    let i = layer.info().mask.as_ref().unwrap();
    layer
        .mask()
        .unwrap()
        .sample(
            x + i.anchor_x - layer.info().x,
            y + i.anchor_y - layer.info().y,
        )
        .unwrap()
}
fn fixture() -> Editor {
    let mut e = Editor::new();
    run(
        &mut e,
        "file.new",
        json!({"width":32,"height":24,"bit_depth":16,"working_space":"display_p3"}),
    );
    e
}
#[test]
fn orientations_preserve_every_off_artboard_hidden_sample_mask_selection_and_exact_undo() {
    for depth in [8, 16] {
        let mut e = Editor::new();
        run(
            &mut e,
            "file.new",
            json!({"width":32,"height":24,"bit_depth":depth,"working_space":"display_p3"}),
        );
        let values = (0..17 * 9)
            .flat_map(|i| {
                [
                    10001 + (i % 17) as u16 * 1900,
                    20003,
                    50001,
                    if i % 3 == 0 { 0 } else { 32769 },
                ]
            })
            .collect::<Vec<_>>();
        let codes = if depth == 16 {
            PixelBuffer::Rgba16(values)
        } else {
            PixelBuffer::Rgba8(values.into_iter().map(|v| (v / 257) as u8).collect())
        };
        let image = ClipboardImage::new(
            codes,
            17,
            9,
            0,
            0,
            e.document().unwrap().working_icc().to_vec(),
            &e.document().unwrap().memory_pool(),
        )
        .unwrap();
        let token = e.grant_clipboard_image(image, Source::User).unwrap();
        run(&mut e, "edit.paste_in_place", json!({"resource":token}));
        let target = e.snapshot().active_layer;
        run(&mut e, "select.all", json!({}));
        run(
            &mut e,
            "select.marquee",
            json!({"x":2,"y":3,"width":4,"height":5}),
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
        run(
            &mut e,
            "layer.set_visibility",
            json!({"layer":{"id":target},"visible":false}),
        );
        for op in OPS {
            let mut edited = e.clone();
            let before = edited.document().unwrap().to_archive().unwrap();
            let history = edited.snapshot().history.len();
            let original = e.document().unwrap().layers().last().unwrap();
            let result = run(&mut edited, "image.orient", json!({"operation":op}));
            assert_eq!(result["preserved_layer_structure"], true);
            assert_eq!(edited.snapshot().history.len(), history + 1);
            let doc = edited.document().unwrap();
            let layer = doc.layers().last().unwrap();
            assert!(!layer.info().visible);
            assert!(!layer.info().mask.as_ref().unwrap().enabled);
            for y in 0..9 {
                for x in 0..17 {
                    let [dx, dy] = map(op, x - 129, y + 25, 32, 24);
                    assert_eq!(
                        raw(layer, dx, dy),
                        raw(original, x - 129, y + 25),
                        "{op} at {x},{y}"
                    );
                    assert_eq!(mask(layer, dx, dy), mask(original, x - 129, y + 25));
                }
            }
            let [sx, sy] = map(op, 3, 4, 32, 24);
            assert_eq!(doc.selection().unwrap().sample(sx, sy).unwrap(), 65535);
            let f = doc.frame();
            assert_eq!((f.x, f.y), (0, 0));
            assert_eq!(
                (f.width, f.height),
                if op.contains("90") {
                    (24, 32)
                } else {
                    (32, 24)
                }
            );
            assert_eq!(doc.selection().unwrap().layout().bounds.width, f.width);
            let archive = doc.to_archive().unwrap();
            assert_eq!(
                Document::from_archive(archive.clone())
                    .unwrap()
                    .to_archive()
                    .unwrap(),
                archive
            );
            run(&mut edited, "edit.undo", json!({}));
            assert_eq!(edited.document().unwrap().to_archive().unwrap(), before);
        }
    }
}
#[test]
fn generated_layers_group_styles_clipping_and_spatial_tone_remain_editable_and_covariant() {
    let mut e = fixture();
    run(
        &mut e,
        "paint.gradient",
        json!({"layer":{"name":"Background"},"start":[0.5,0.5],"end":[31.5,23.5],"stops":[{"at":0,"color":[0.1,0.2,0.3,1]},{"at":1,"color":[0.8,0.6,0.4,1]}]}),
    );
    run(
        &mut e,
        "layer.set_styles",
        json!({"layer":{"name":"Background"},"styles":{"gradient_overlay":{"start":[1000,2000,3000,65535],"end":[60000,62000,63000,65535],"angle":23,"scale":1,"opacity":0.3}}}),
    );
    let fill=run(&mut e,"layer.fill.gradient",json!({"name":"fill","start":[1000,3000,5000,20000],"end":[50000,45000,40000,20000],"angle":35,"scale":1}))["layer_id"].clone();
    run(
        &mut e,
        "layer.set_clipping",
        json!({"layer":{"id":fill},"clipped":true}),
    );
    run(
        &mut e,
        "layer.add_tonal_adjustment",
        json!({"name":"local tone","adjustment":{"type":"shadows_highlights","shadows":65,"highlights":20,"radius":8}}),
    );
    let tone = e.snapshot().active_layer;
    let group = run(
        &mut e,
        "layer.group",
        json!({"layers":[{"name":"Background"},{"id":fill},{"id":tone}],"name":"all"}),
    )["layer_id"]
        .clone();
    run(
        &mut e,
        "layer.set_styles",
        json!({"layer":{"id":group},"styles":{"drop_shadow":{"color":[0,0,0,30000],"blur":2,"offset_x":5,"offset_y":-3},"gradient_overlay":{"start":[1000,2000,3000,65535],"end":[60000,62000,63000,65535],"angle":11,"scale":1,"opacity":0.1}}}),
    );
    let reference = e.document().unwrap().export_pixels().unwrap();
    let PixelBuffer::Rgba16(ref codes) = reference else {
        panic!()
    };
    for op in OPS {
        let mut edited = e.clone();
        run(&mut edited, "image.orient", json!({"operation":op}));
        let doc = edited.document().unwrap();
        assert_eq!(doc.layers().len(), e.document().unwrap().layers().len());
        assert!(matches!(
            doc.layers()
                .iter()
                .find(|l| l.info().name == "fill")
                .unwrap()
                .info()
                .kind,
            LayerKind::Gradient { .. }
        ));
        assert!(
            doc.layers()
                .iter()
                .find(|l| l.info().name == "local tone")
                .unwrap()
                .info()
                .spatial_bounds
                .is_some()
        );
        let PixelBuffer::Rgba16(output) = doc.export_pixels().unwrap() else {
            panic!()
        };
        let width = doc.frame().width as usize;
        for (x, y) in [(1, 1), (5, 8), (16, 12), (30, 22)] {
            let [dx, dy] = map(op, x, y, 32, 24);
            for c in 0..4 {
                let a = output[(dy as usize * width + dx as usize) * 4 + c];
                let b = codes[(y as usize * 32 + x as usize) * 4 + c];
                assert!(a.abs_diff(b) <= 8, "{op} {x},{y},{c}: {a} != {b}");
            }
        }
        let reopened = Document::from_archive(doc.to_archive().unwrap()).unwrap();
        assert_eq!(
            reopened.export_pixels().unwrap(),
            PixelBuffer::Rgba16(output)
        );
    }
}
#[test]
fn every_lock_and_unknown_operation_reject_without_mutation() {
    for lock in ["pixels", "position", "transparency", "all"] {
        let mut e = fixture();
        let mut locks = serde_json::Map::new();
        locks.insert(lock.into(), json!(true));
        run(
            &mut e,
            "layer.set_locks",
            json!({"layer":{"name":"Background"},"locks":locks}),
        );
        let before = e.document().unwrap().to_archive().unwrap();
        let revision = e.revision();
        assert!(
            e.execute(
                e.request(Source::User),
                "image.orient",
                json!({"operation":"rotate_90_cw"}),
                |_| Ok(())
            )
            .is_err()
        );
        assert_eq!(e.revision(), revision);
        assert_eq!(e.document().unwrap().to_archive().unwrap(), before);
    }
    let mut e = fixture();
    assert!(
        e.execute(
            e.request(Source::User),
            "image.orient",
            json!({"operation":"rotate_13"}),
            |_| Ok(())
        )
        .is_err()
    );
    let prepared = e
        .prepare(
            e.request(Source::User),
            "image.orient",
            json!({"operation":"flip_horizontal"}),
        )
        .unwrap();
    let before = e.document().unwrap().to_archive().unwrap();
    e.cancel_pending_work();
    assert!(e.commit(prepared).is_err());
    assert_eq!(e.document().unwrap().to_archive().unwrap(), before);
}
#[test]
fn a_nonzero_cropped_canvas_keeps_its_origin_and_frozen_style_reference_under_orientation() {
    let mut e = fixture();
    run(
        &mut e,
        "paint.gradient",
        json!({"layer":{"name":"Background"},"start":[0.5,0.5],"end":[31.5,23.5],"stops":[{"at":0,"color":[0.1,0.2,0.3,0.5]},{"at":1,"color":[0.8,0.6,0.4,0.9]}]}),
    );
    run(
        &mut e,
        "layer.set_styles",
        json!({"layer":{"name":"Background"},"styles":{"gradient_overlay":{"start":[1000,2000,3000,65535],"end":[60000,62000,63000,65535],"angle":27,"scale":1,"opacity":0.3}}}),
    );
    run(
        &mut e,
        "image.crop",
        json!({"rect":{"x":-3,"y":5,"width":21,"height":13},"pixels":"preserve"}),
    );
    let PixelBuffer::Rgba16(reference) = e.document().unwrap().export_pixels().unwrap() else {
        panic!()
    };
    for op in OPS {
        let mut edited = e.clone();
        run(&mut edited, "image.orient", json!({"operation":op}));
        let doc = edited.document().unwrap();
        assert_eq!((doc.frame().x, doc.frame().y), (-3, 5));
        let PixelBuffer::Rgba16(output) = doc.export_pixels().unwrap() else {
            panic!()
        };
        let width = doc.frame().width as usize;
        for y in 0..13 {
            for x in 0..21 {
                let [dx, dy] = map(op, x, y, 21, 13);
                for c in 0..4 {
                    let a = output[(dy as usize * width + dx as usize) * 4 + c];
                    let b = reference[(y as usize * 21 + x as usize) * 4 + c];
                    assert!(a.abs_diff(b) <= 8, "{op} {x},{y},{c}: {a} != {b}");
                }
            }
        }
    }
}
#[test]
fn shared_budget_failure_and_inflight_cancel_never_publish_part_of_an_image() {
    use strata_pixels::memory::{MemoryLimits, MemoryPool};
    for budget in [4 * 1024 * 1024, 128 * 1024 * 1024] {
        let pool = MemoryPool::new(MemoryLimits {
            cpu_bytes: budget,
            gpu_bytes: 0,
            disk_bytes: 128 * 1024 * 1024,
        });
        let mut e = Editor::with_memory_pool(pool.clone());
        run(
            &mut e,
            "file.new",
            json!({"width":512,"height":512,"bit_depth":16,"background":[10001,30003,50001,32769]}),
        );
        let revision = e.revision();
        if budget < 8 * 1024 * 1024 {
            assert!(
                e.execute(
                    e.request(Source::User),
                    "image.orient",
                    json!({"operation":"rotate_180"}),
                    |_| Ok(())
                )
                .is_err()
            );
        } else {
            let source = e.clone();
            let request = e.request(Source::User);
            let worker = std::thread::spawn(move || {
                source.prepare(request, "image.orient", json!({"operation":"rotate_180"}))
            });
            let start = std::time::Instant::now();
            while pool.stats().transient_bytes < 12 * 1024 * 1024
                && start.elapsed() < std::time::Duration::from_secs(3)
            {
                std::thread::yield_now();
            }
            assert!(pool.stats().transient_bytes >= 12 * 1024 * 1024);
            e.cancel_pending_work();
            assert!(worker.join().unwrap().is_err());
        }
        assert_eq!(e.revision(), revision);
        assert!(e.snapshot().history.is_empty());
        assert_eq!(
            raw(&e.document().unwrap().layers()[0], 17, 29),
            [10001, 30003, 50001, 32769]
        );
        assert_eq!(pool.stats().transient_bytes, 0);
    }
}
