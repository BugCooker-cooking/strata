use serde_json::{Value, json};
use strata_core::{ClipboardImage, Document, Editor, LayerKind, Source};
use strata_pixels::{PixelBuffer, PixelValue, TileCoord};
fn run(e: &mut Editor, id: &str, p: Value) -> Value {
    e.execute(e.request(Source::User), id, p, |_| Ok(()))
        .unwrap()
}
fn fixture(depth: u8) -> Editor {
    let mut e = Editor::new();
    run(
        &mut e,
        "file.new",
        json!({"width":520,"height":12,"bit_depth":depth,"working_space":"display_p3"}),
    );
    let data = (0..520 * 12)
        .flat_map(|i| {
            [
                10001 + (i % 31) as u16 * 1400,
                30003,
                50001,
                if i % 7 == 0 { 0 } else { 32769 },
            ]
        })
        .collect::<Vec<_>>();
    let pixels = if depth == 16 {
        PixelBuffer::Rgba16(data)
    } else {
        PixelBuffer::Rgba8(data.into_iter().map(|v| (v / 257) as u8).collect())
    };
    let image = ClipboardImage::new(
        pixels,
        520,
        12,
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
fn raw(e: &Editor, x: i64, y: i64) -> PixelValue {
    let l = e
        .document()
        .unwrap()
        .layers()
        .iter()
        .find(|l| matches!(l.info().kind, LayerKind::Pixel) && l.info().name != "Background")
        .unwrap();
    l.pixels().pixel(x - l.info().x, y - l.info().y).unwrap()
}
fn crop(rect: Value) -> Value {
    json!({"rect":rect,"pixels":"delete"})
}
#[test]
fn destructive_crop_deletes_raw_outside_codes_and_disabled_masks_without_touching_inside_or_layer_positions()
 {
    for depth in [8, 16] {
        let mut e = fixture(depth);
        let target = e.snapshot().active_layer;
        run(
            &mut e,
            "select.marquee",
            json!({"x":0,"y":1,"width":515,"height":10}),
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
        let before = e.clone();
        let archive = e.document().unwrap().to_archive().unwrap();
        let untouched = e
            .document()
            .unwrap()
            .layers()
            .last()
            .unwrap()
            .pixels()
            .tile_version(TileCoord::new(0, 0).unwrap());
        let result = run(
            &mut e,
            "image.crop",
            crop(json!({"x":-129,"y":26,"width":300,"height":9})),
        );
        assert_eq!(result["preserved_external_content"], false);
        let layer = e.document().unwrap().layers().last().unwrap();
        assert_eq!((layer.info().x, layer.info().y), (-129, 25));
        assert!(!layer.info().mask.as_ref().unwrap().enabled);
        assert_ne!(
            layer.pixels().tile_version(TileCoord::new(0, 0).unwrap()),
            untouched
        );
        assert_eq!(
            layer.pixels().tile_version(TileCoord::new(2, 0).unwrap()),
            None
        );
        for y in 25..37 {
            for x in -129..391 {
                let actual = raw(&e, x, y);
                if (-129..171).contains(&x) && (26..35).contains(&y) {
                    assert_eq!(actual, raw(&before, x, y));
                } else {
                    assert_eq!(
                        actual,
                        if depth == 16 {
                            PixelValue::Rgba16([0; 4])
                        } else {
                            PixelValue::Rgba8([0; 4])
                        }
                    );
                }
            }
        }
        let meta = layer.info().mask.as_ref().unwrap();
        let m = layer.mask().unwrap();
        assert_eq!(
            m.sample(
                400 + meta.anchor_x - layer.info().x,
                28 + meta.anchor_y - layer.info().y
            )
            .unwrap(),
            0
        );
        assert_eq!(
            m.sample(
                -125 + meta.anchor_x - layer.info().x,
                28 + meta.anchor_y - layer.info().y
            )
            .unwrap(),
            65535
        );
        let current = e.document().unwrap().to_archive().unwrap();
        assert_eq!(
            Document::from_archive(current.clone())
                .unwrap()
                .to_archive()
                .unwrap(),
            current
        );
        run(
            &mut e,
            "image.canvas_size",
            json!({"width":520,"height":12,"anchor":"top_left"}),
        );
        assert_eq!(
            raw(&e, 300, 28),
            if depth == 16 {
                PixelValue::Rgba16([0; 4])
            } else {
                PixelValue::Rgba8([0; 4])
            }
        );
        run(&mut e, "edit.undo", json!({}));
        run(&mut e, "edit.undo", json!({}));
        assert_eq!(e.document().unwrap().to_archive().unwrap(), archive);
    }
}
#[test]
fn generated_crop_remains_editable_and_never_regrows_after_expansion_mask_removal_or_profile_edits()
{
    let mut e = Editor::new();
    run(
        &mut e,
        "file.new",
        json!({"width":32,"height":24,"bit_depth":16}),
    );
    let fill=run(&mut e,"layer.fill.gradient",json!({"name":"gradient","start":[1000,3000,5000,65535],"end":[50000,45000,40000,65535],"angle":35,"scale":1}))["layer_id"].clone();
    run(
        &mut e,
        "layer.mask.add",
        json!({"layer":{"id":fill},"source":"white"}),
    );
    let before = e.document().unwrap().to_archive().unwrap();
    let reference = e.document().unwrap().export_pixels().unwrap();
    run(
        &mut e,
        "image.crop",
        crop(json!({"x":4,"y":3,"width":16,"height":12})),
    );
    let doc = e.document().unwrap();
    let layer = doc.layers().last().unwrap();
    assert!(matches!(layer.info().kind, LayerKind::Gradient { .. }));
    assert_eq!(layer.info().generated_size, Some([32, 24]));
    assert_eq!(
        doc.generated_canvas_bounds(layer).unwrap().unwrap(),
        strata_pixels::CoverageRect {
            x: 4,
            y: 3,
            width: 16,
            height: 12
        }
    );
    let PixelBuffer::Rgba16(cropped) = doc.export_pixels().unwrap() else {
        panic!()
    };
    let PixelBuffer::Rgba16(reference) = reference else {
        panic!()
    };
    for y in 0..12 {
        for x in 0..16 {
            assert_eq!(
                &cropped[(y * 16 + x) * 4..(y * 16 + x + 1) * 4],
                &reference[((y + 3) * 32 + x + 4) * 4..((y + 3) * 32 + x + 5) * 4]
            );
        }
    }
    let archive = doc.to_archive().unwrap();
    assert_eq!(archive.metadata["version"], 6);
    let mut old = archive.clone();
    old.metadata["version"] = json!(5);
    assert!(Document::from_archive(old).is_err());
    assert_eq!(
        Document::from_archive(archive.clone())
            .unwrap()
            .to_archive()
            .unwrap(),
        archive
    );
    run(&mut e, "layer.mask.remove", json!({"layer":{"id":fill}}));
    run(&mut e, "color.assign", json!({"space":"display_p3"}));
    run(
        &mut e,
        "image.canvas_size",
        json!({"width":32,"height":24,"anchor":"center"}),
    );
    let PixelBuffer::Rgba16(expanded) = e.document().unwrap().export_pixels().unwrap() else {
        panic!()
    };
    assert_eq!(&expanded[(2 * 32 + 2) * 4..(2 * 32 + 3) * 4], &[0; 4]);
    for _ in 0..4 {
        run(&mut e, "edit.undo", json!({}));
    }
    assert_eq!(e.document().unwrap().to_archive().unwrap(), before);
}
#[test]
fn cropped_generated_domain_follows_exact_orientation_and_rasterization_without_resurrection() {
    let mut e = Editor::new();
    run(
        &mut e,
        "file.new",
        json!({"width":32,"height":24,"bit_depth":16}),
    );
    run(
        &mut e,
        "layer.fill.solid",
        json!({"name":"fill","color":[10001,30003,50001,65535]}),
    );
    run(
        &mut e,
        "image.crop",
        crop(json!({"x":4,"y":3,"width":16,"height":12})),
    );
    run(&mut e, "image.orient", json!({"operation":"rotate_90_cw"}));
    let layer = e.document().unwrap().layers().last().unwrap();
    let bounds = e
        .document()
        .unwrap()
        .generated_canvas_bounds(layer)
        .unwrap()
        .unwrap();
    assert_eq!(
        bounds,
        strata_pixels::CoverageRect {
            x: 4,
            y: 3,
            width: 12,
            height: 16
        }
    );
    run(&mut e, "layer.rasterize", json!({"layer":{"name":"fill"}}));
    assert!(
        e.document()
            .unwrap()
            .layers()
            .last()
            .unwrap()
            .info()
            .generated_crop
            .is_none()
    );
    let before = e.document().unwrap().to_archive().unwrap();
    run(&mut e, "image.reveal_all", json!({}));
    let f = e.document().unwrap().frame();
    assert_eq!((f.x, f.y, f.width, f.height), (4, 3, 12, 16));
    run(&mut e, "edit.undo", json!({}));
    assert_eq!(e.document().unwrap().to_archive().unwrap(), before);
}
#[test]
fn zero_overlap_generated_fill_is_explicitly_empty_and_hidden_locked_sources_fail_atomically() {
    let mut e = Editor::new();
    run(
        &mut e,
        "file.new",
        json!({"width":16,"height":16,"bit_depth":16}),
    );
    let fill = run(
        &mut e,
        "layer.fill.solid",
        json!({"name":"outside","color":[10001,30003,50001,65535]}),
    )["layer_id"]
        .clone();
    run(
        &mut e,
        "layer.set_position",
        json!({"layer":{"id":fill},"x":50,"y":50}),
    );
    run(
        &mut e,
        "image.crop",
        crop(json!({"x":0,"y":0,"width":8,"height":8})),
    );
    let layer = e.document().unwrap().layers().last().unwrap();
    assert!(
        layer
            .info()
            .generated_crop
            .as_ref()
            .unwrap()
            .bounds
            .is_none()
    );
    run(&mut e, "image.reveal_all", json!({}));
    assert_eq!(e.document().unwrap().frame().width, 8);
    run(
        &mut e,
        "layer.set_visibility",
        json!({"layer":{"id":fill},"visible":false}),
    );
    run(
        &mut e,
        "layer.set_locks",
        json!({"layer":{"id":fill},"locks":{"pixels":true}}),
    );
    let before = e.document().unwrap().to_archive().unwrap();
    let revision = e.revision();
    assert!(
        e.execute(
            e.request(Source::User),
            "image.crop",
            crop(json!({"x":0,"y":0,"width":4,"height":4})),
            |_| Ok(())
        )
        .is_err()
    );
    assert_eq!(e.revision(), revision);
    assert_eq!(e.document().unwrap().to_archive().unwrap(), before);
}
#[test]
fn full_inside_tiles_keep_versions_and_position_only_lock_allows_crop() {
    let mut e = fixture(16);
    let target = e.snapshot().active_layer;
    run(
        &mut e,
        "layer.set_locks",
        json!({"layer":{"id":target},"locks":{"position":true}}),
    );
    let old = e
        .document()
        .unwrap()
        .layers()
        .last()
        .unwrap()
        .pixels()
        .tile_version(TileCoord::new(0, 0).unwrap());
    run(
        &mut e,
        "image.crop",
        crop(json!({"x":0,"y":0,"width":256,"height":256})),
    );
    let layer = e.document().unwrap().layers().last().unwrap();
    assert_eq!(
        layer.pixels().tile_version(TileCoord::new(0, 0).unwrap()),
        old
    );
    assert_eq!(layer.pixels().tile_count(), 1);
}
#[test]
fn destructive_crop_budget_and_inflight_cancellation_are_atomic_and_release_transients() {
    use strata_pixels::memory::{MemoryLimits, MemoryPool};
    for cpu_bytes in [16 * 1024 * 1024, 64 * 1024 * 1024] {
        let pool = MemoryPool::new(MemoryLimits {
            cpu_bytes,
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
        let params = crop(json!({"x":1,"y":1,"width":1022,"height":1022}));
        if cpu_bytes < 32 * 1024 * 1024 {
            let _pressure = pool
                .reserve(
                    strata_pixels::memory::MemoryKind::Transient,
                    cpu_bytes - 256 * 1024,
                )
                .unwrap();
            assert!(
                e.execute(e.request(Source::User), "image.crop", params, |_| Ok(()))
                    .is_err()
            );
        } else {
            let editor = e.clone();
            let request = e.request(Source::User);
            let worker = std::thread::spawn(move || editor.prepare(request, "image.crop", params));
            let start = std::time::Instant::now();
            while pool.stats().transient_bytes < 512 * 1024
                && start.elapsed() < std::time::Duration::from_secs(3)
            {
                std::thread::yield_now();
            }
            assert!(pool.stats().transient_bytes >= 512 * 1024);
            e.cancel_pending_work();
            assert!(worker.join().unwrap().is_err());
        }
        assert_eq!(e.revision(), revision);
        assert!(e.snapshot().history.is_empty());
        assert_eq!(pool.stats().transient_bytes, 0);
    }
}
