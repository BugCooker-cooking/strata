use serde_json::{Value, json};
use strata_core::{ClipboardImage, Document, Editor, Source};
use strata_pixels::{
    PixelBuffer, PixelValue,
    memory::{MemoryLimits, MemoryPool},
};
fn run(e: &mut Editor, id: &str, p: Value) -> Value {
    e.execute(e.request(Source::User), id, p, |_| Ok(()))
        .unwrap()
}
fn fixture(w: u32, h: u32, depth: u8) -> Editor {
    let mut e = Editor::new();
    run(
        &mut e,
        "file.new",
        json!({"width":w,"height":h,"bit_depth":depth,"working_space":"display_p3"}),
    );
    let data = (0..w * h)
        .flat_map(|i| {
            [
                10001 + (i % w % 19) as u16 * 1900,
                20003 + (i / w % 13) as u16 * 1700,
                50001,
                if i % 5 == 0 { 0 } else { 32769 },
            ]
        })
        .collect::<Vec<_>>();
    let pixels = if depth == 16 {
        PixelBuffer::Rgba16(data)
    } else {
        PixelBuffer::Rgba8(data.into_iter().map(|v| (v / 257) as u8).collect())
    };
    paste(&mut e, pixels, w, h);
    e
}
fn paste(e: &mut Editor, p: PixelBuffer, w: u32, h: u32) {
    let image = ClipboardImage::new(
        p,
        w,
        h,
        0,
        0,
        e.document().unwrap().working_icc().to_vec(),
        &e.document().unwrap().memory_pool(),
    )
    .unwrap();
    let r = e.grant_clipboard_image(image, Source::User).unwrap();
    run(e, "edit.paste_in_place", json!({"resource":r}));
}
fn pixel(e: &Editor, x: i64, y: i64) -> [u16; 4] {
    let l = e.document().unwrap().layers().last().unwrap();
    match l.pixels().pixel(x - l.info().x, y - l.info().y).unwrap() {
        PixelValue::Rgba8(v) => v.map(u16::from),
        PixelValue::Rgba16(v) => v,
    }
}
#[test]
fn nearest_resizes_across_tiles_and_large_offsets_without_phase_or_code_loss() {
    for depth in [8, 16] {
        for offset in [[0, 0], [-13, -7], [999999999000i64, -999999999000i64]] {
            let mut e = fixture(257, 3, depth);
            let id = e.snapshot().active_layer;
            run(
                &mut e,
                "layer.set_position",
                json!({"layer":{"id":id},"x":offset[0],"y":offset[1]}),
            );
            run(
                &mut e,
                "image.crop",
                json!({"rect":{"x":offset[0],"y":offset[1],"width":257,"height":3},"pixels":"preserve"}),
            );
            let original = e.clone();
            let archive = e.document().unwrap().to_archive().unwrap();
            let cursor = e.snapshot().history_cursor;
            run(
                &mut e,
                "image.resize",
                json!({"width":385,"height":5,"interpolation":"nearest"}),
            );
            let frame = e.document().unwrap().frame();
            assert_eq!(
                (frame.x, frame.y, frame.width, frame.height),
                (offset[0], offset[1], 385, 5)
            );
            for y in 0..5 {
                for x in 0..385 {
                    let sx = ((f64::from(x) + 0.5) * 257. / 385.).floor() as i64;
                    let sy = ((f64::from(y) + 0.5) * 3. / 5.).floor() as i64;
                    assert_eq!(
                        pixel(&e, offset[0] + i64::from(x), offset[1] + i64::from(y)),
                        pixel(&original, offset[0] + sx, offset[1] + sy),
                        "{depth} {offset:?} {x},{y}"
                    );
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
            assert_eq!(e.snapshot().history_cursor, cursor + 1);
            run(&mut e, "edit.undo", json!({}));
            assert_eq!(e.document().unwrap().to_archive().unwrap(), archive);
        }
    }
}
fn reference(e: &Editor, size: [u32; 2], mode: &str) -> PixelBuffer {
    let frame = e.document().unwrap().frame();
    let (w, h) = (frame.width, frame.height);
    let icc = e.document().unwrap().working_icc();
    let format = e.document().unwrap().format();
    let mut source = Vec::new();
    for y in 0..h {
        for x in 0..w {
            let v = pixel(e, i64::from(x), i64::from(y));
            let p = if format == strata_pixels::PixelFormat::Rgba16 {
                PixelBuffer::Rgba16(v.to_vec())
            } else {
                PixelBuffer::Rgba8(v.map(|v| v as u8).to_vec())
            };
            source.push(strata_color::to_linear_p3(&p, 1, 1, icc).unwrap()[0].map(f64::from));
        }
    }
    let weights = |d: u32, input: u32, output: u32| -> Vec<(usize, f64)> {
        let scale = f64::from(input) / f64::from(output);
        if mode == "area" && input > output {
            let start = f64::from(d) * scale;
            let end = f64::from(d + 1) * scale;
            (0..input)
                .filter_map(|s| {
                    let v = (end.min(f64::from(s + 1)) - start.max(f64::from(s))).max(0.) / scale;
                    (v > 0.).then_some((s as usize, v))
                })
                .collect()
        } else {
            let center = ((f64::from(d) + 0.5) * scale - 0.5).clamp(0., f64::from(input - 1));
            let first = center.floor() as usize;
            let f = center - first as f64;
            vec![(first, 1. - f), ((first + 1).min(input as usize - 1), f)]
        }
    };
    let mut values = Vec::new();
    for y in 0..size[1] {
        for x in 0..size[0] {
            let mut p = [0.; 4];
            for (sy, wy) in weights(y, h, size[1]) {
                for (sx, wx) in weights(x, w, size[0]) {
                    let s = source[sy * w as usize + sx];
                    for c in 0..3 {
                        p[c] += s[c] * s[3] * wx * wy;
                    }
                    p[3] += s[3] * wx * wy;
                }
            }
            if p[3] > 0. {
                for c in 0..3 {
                    p[c] /= p[3];
                }
            }
            values.push(p.map(|v| v as f32));
        }
    }
    strata_color::from_linear_p3(&values, format, icc).unwrap()
}
#[test]
fn bilinear_and_area_match_independent_premultiplied_linear_reference_in_both_depths() {
    for depth in [8, 16] {
        for size in [[4, 3], [13, 11], [4, 11]] {
            for mode in ["bilinear", "area"] {
                let mut e = fixture(9, 7, depth);
                let expected = reference(&e, size, mode);
                run(
                    &mut e,
                    "image.resize",
                    json!({"width":size[0],"height":size[1],"interpolation":mode}),
                );
                for y in 0..size[1] {
                    for x in 0..size[0] {
                        let i = (y * size[0] + x) as usize * 4;
                        let v: [u16; 4] = match &expected {
                            PixelBuffer::Rgba8(v) => std::array::from_fn(|c| u16::from(v[i + c])),
                            PixelBuffer::Rgba16(v) => std::array::from_fn(|c| v[i + c]),
                        };
                        let actual = pixel(&e, i64::from(x), i64::from(y));
                        // Integer-center samples preserve even alpha-zero hidden RGB.
                        if v[3] > 0 {
                            assert!(
                                (0..4).all(|c| actual[c].abs_diff(v[c]) <= 4),
                                "{depth} {mode} {size:?} {x},{y}: {actual:?} != {v:?}"
                            );
                        } else {
                            assert_eq!(actual[3], 0);
                        }
                    }
                }
            }
        }
    }
}
#[test]
fn area_reduction_suppresses_aliasing_and_enlargement_does_not_fade_opaque_edges() {
    let mut e = Editor::new();
    run(
        &mut e,
        "file.new",
        json!({"width":6,"height":1,"bit_depth":16}),
    );
    paste(
        &mut e,
        PixelBuffer::Rgba16(
            (0..6)
                .flat_map(|x| {
                    if x % 2 == 0 {
                        [0, 0, 0, 65535]
                    } else {
                        [65535; 4]
                    }
                })
                .collect(),
        ),
        6,
        1,
    );
    let mut linear = e.clone();
    run(
        &mut linear,
        "image.resize",
        json!({"width":2,"height":1,"interpolation":"bilinear"}),
    );
    run(&mut e, "image.resize", json!({"width":2,"height":1}));
    assert!(pixel(&linear, 0, 0)[0] > 65000);
    assert!(pixel(&linear, 1, 0)[0] < 10);
    let a = pixel(&e, 0, 0)[0];
    let b = pixel(&e, 1, 0)[0];
    assert!((35000..45000).contains(&a));
    assert!((50000..57000).contains(&b));
    for mode in ["nearest", "bilinear", "area"] {
        let mut opaque = Editor::new();
        run(
            &mut opaque,
            "file.new",
            json!({"width":3,"height":2,"bit_depth":16,"background":[10001,30003,50001,65535]}),
        );
        run(
            &mut opaque,
            "image.resize",
            json!({"width":13,"height":9,"interpolation":mode}),
        );
        for (x, y) in [(0, 0), (12, 0), (0, 8), (12, 8)] {
            let p = pixel(&opaque, x, y);
            assert_eq!(p[3], 65535);
            assert!((0..3).all(|c| p[c].abs_diff([10001, 30003, 50001][c]) <= 3));
        }
    }
}
#[test]
fn hidden_group_offboard_sources_disabled_masks_and_both_selections_keep_their_geometry() {
    let mut e = fixture(16, 12, 16);
    let id = e.snapshot().active_layer;
    run(
        &mut e,
        "select.marquee",
        json!({"x":2,"y":2,"width":9,"height":7}),
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
    run(&mut e, "select.none", json!({}));
    run(&mut e, "select.all", json!({}));
    run(
        &mut e,
        "layer.set_position",
        json!({"layer":{"id":id},"x":-7,"y":-3}),
    );
    run(
        &mut e,
        "layer.set_visibility",
        json!({"layer":{"id":id},"visible":false}),
    );
    let group = run(
        &mut e,
        "layer.group",
        json!({"layers":[{"id":id}],"name":"hidden source"}),
    )["layer_id"]
        .clone();
    run(
        &mut e,
        "layer.mask.add",
        json!({"layer":{"id":group},"source":"white"}),
    );
    let before = e.document().unwrap().to_archive().unwrap();
    let original = e.clone();
    run(
        &mut e,
        "image.resize",
        json!({"width":32,"height":24,"interpolation":"nearest"}),
    );
    let doc = e.document().unwrap();
    assert_eq!(
        doc.layers().iter().map(|l| l.id()).collect::<Vec<_>>(),
        original
            .document()
            .unwrap()
            .layers()
            .iter()
            .map(|l| l.id())
            .collect::<Vec<_>>()
    );
    let old = original
        .document()
        .unwrap()
        .layers()
        .iter()
        .find(|l| Some(l.id()) == id)
        .unwrap();
    let now = doc.layers().iter().find(|l| Some(l.id()) == id).unwrap();
    assert_eq!((now.info().x, now.info().y), (-14, -6));
    assert!(!now.info().visible);
    assert!(!now.info().mask.as_ref().unwrap().enabled);
    let meta = old.info().mask.as_ref().unwrap();
    let dx = old.info().x - meta.anchor_x;
    let dy = old.info().y - meta.anchor_y;
    for y in -6..24 {
        for x in -14..32 {
            assert_eq!(
                now.mask().unwrap().sample(x, y).unwrap(),
                old.mask()
                    .unwrap()
                    .sample(x.div_euclid(2) - dx, y.div_euclid(2) - dy)
                    .unwrap()
            );
        }
    }
    assert_eq!(doc.selection().unwrap().layout().bounds.width, 32);
    assert_eq!(doc.previous_selection().unwrap().layout().bounds.height, 24);
    for y in 0..24 {
        for x in 0..32 {
            assert_eq!(doc.selection().unwrap().sample(x, y).unwrap(), 65535);
            assert_eq!(
                doc.previous_selection().unwrap().sample(x, y).unwrap(),
                original
                    .document()
                    .unwrap()
                    .previous_selection()
                    .unwrap()
                    .sample(x / 2, y / 2)
                    .unwrap()
            );
        }
    }
    let saved = doc.to_archive().unwrap();
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
fn equal_size_is_noop_and_invalid_locked_or_unsupported_edits_are_atomic() {
    let original = fixture(9, 7, 16);
    for kind in ["solid", "style", "adjustment", "lock", "ancestor"] {
        let mut e = original.clone();
        let id = e.snapshot().active_layer;
        match kind {
            "solid" => {
                run(
                    &mut e,
                    "layer.fill.solid",
                    json!({"name":"fill","color":[10001,30003,50001,65535]}),
                );
            }
            "adjustment" => {
                run(
                    &mut e,
                    "layer.add_tonal_adjustment",
                    json!({"name":"tone","adjustment":{"type":"brightness_contrast"}}),
                );
            }
            "style" => {
                run(
                    &mut e,
                    "layer.set_styles",
                    json!({"layer":{"id":id},"styles":{"color_overlay":{"color":[10001,30003,50001,65535],"opacity":0.5}}}),
                );
            }
            "ancestor" => {
                let g = run(
                    &mut e,
                    "layer.group",
                    json!({"layers":[{"id":id}],"name":"locked"}),
                )["layer_id"]
                    .clone();
                run(
                    &mut e,
                    "layer.set_locks",
                    json!({"layer":{"id":g},"locks":{"all":true}}),
                );
            }
            _ => {
                run(
                    &mut e,
                    "layer.set_locks",
                    json!({"layer":{"id":id},"locks":{"position":true}}),
                );
            }
        }
        let before = e.document().unwrap().to_archive().unwrap();
        let revision = e.revision();
        let cursor = e.snapshot().history_cursor;
        assert_eq!(
            run(&mut e, "image.resize", json!({"width":9,"height":7}))["changed"],
            false
        );
        assert!(
            e.execute(
                e.request(Source::User),
                "image.resize",
                json!({"width":13,"height":11}),
                |_| Ok(())
            )
            .is_err()
        );
        assert_eq!(e.revision(), revision);
        assert_eq!(e.snapshot().history_cursor, cursor);
        assert_eq!(e.document().unwrap().to_archive().unwrap(), before);
    }
    let mut e = original;
    let before = e.document().unwrap().to_archive().unwrap();
    for p in [
        json!({"width":0,"height":1}),
        json!({"width":100001,"height":1}),
        json!({"width":100000,"height":100000}),
        json!({"width":3.5,"height":4}),
        json!({"width":4,"height":4,"interpolation":"bicubic"}),
        json!({"width":4,"height":4,"keep_layers":false}),
    ] {
        assert!(
            e.execute(e.request(Source::User), "image.resize", p, |_| Ok(()))
                .is_err()
        );
        assert_eq!(e.document().unwrap().to_archive().unwrap(), before);
    }
}

#[test]
fn soft_u16_mask_and_current_selection_use_independent_area_coverage_reference() {
    let mut e = fixture(9, 7, 16);
    let id = e.snapshot().active_layer;
    run(
        &mut e,
        "select.marquee",
        json!({"x":1,"y":1,"width":6,"height":5}),
    );
    run(&mut e, "select.feather", json!({"radius":1.2}));
    run(
        &mut e,
        "layer.mask.add",
        json!({"layer":{"id":id},"source":"from_selection"}),
    );
    let original = e.clone();
    run(
        &mut e,
        "image.resize",
        json!({"width":4,"height":3,"interpolation":"area"}),
    );
    let layer = e
        .document()
        .unwrap()
        .layers()
        .iter()
        .find(|l| Some(l.id()) == id)
        .unwrap();
    let source = original.document().unwrap().selection().unwrap();
    for y in 0..3 {
        for x in 0..4 {
            let (left, right, top, bottom) = (
                f64::from(x) * 9. / 4.,
                f64::from(x + 1) * 9. / 4.,
                f64::from(y) * 7. / 3.,
                f64::from(y + 1) * 7. / 3.,
            );
            let mut sum = 0.;
            for sy in 0..7 {
                for sx in 0..9 {
                    let wx = (right.min(f64::from(sx + 1)) - left.max(f64::from(sx))).max(0.)
                        / (9. / 4.);
                    let wy = (bottom.min(f64::from(sy + 1)) - top.max(f64::from(sy))).max(0.)
                        / (7. / 3.);
                    sum += f64::from(source.sample(sx.into(), sy.into()).unwrap()) * wx * wy;
                }
            }
            let expected = sum.round() as u16;
            assert!(
                layer
                    .mask()
                    .unwrap()
                    .sample(x.into(), y.into())
                    .unwrap()
                    .abs_diff(expected)
                    <= 1
            );
            assert!(
                e.document()
                    .unwrap()
                    .selection()
                    .unwrap()
                    .sample(x.into(), y.into())
                    .unwrap()
                    .abs_diff(expected)
                    <= 1
            );
        }
    }
}
#[test]
fn low_budget_and_cancellation_during_real_resampling_preserve_original() {
    for budget in [8 * 1024 * 1024, 128 * 1024 * 1024] {
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
        let original = e.document().unwrap().to_archive().unwrap();
        let revision = e.revision();
        if budget < 16 * 1024 * 1024 {
            assert!(
                e.execute(
                    e.request(Source::User),
                    "image.resize",
                    json!({"width":800,"height":600}),
                    |_| Ok(())
                )
                .is_err()
            );
        } else {
            let worker = e.clone();
            let request = e.request(Source::User);
            let thread = std::thread::spawn(move || {
                worker.prepare(request, "image.resize", json!({"width":800,"height":600}))
            });
            let start = std::time::Instant::now();
            while pool.stats().transient_bytes < 16 * 1024 * 1024
                && start.elapsed() < std::time::Duration::from_secs(3)
            {
                std::thread::yield_now();
            }
            assert!(pool.stats().transient_bytes >= 16 * 1024 * 1024);
            e.cancel_pending_work();
            assert!(thread.join().unwrap().is_err());
        }
        assert_eq!(e.revision(), revision);
        assert_eq!(e.document().unwrap().to_archive().unwrap(), original);
        assert!(e.snapshot().history.is_empty());
        assert_eq!(pool.stats().transient_bytes, 0);
    }
}

#[test]
fn unchanged_sparse_large_canvas_is_not_subject_to_resampling_work_limits() {
    let mut e = Editor::new();
    run(
        &mut e,
        "file.new",
        json!({"width":1,"height":1,"bit_depth":8}),
    );
    run(
        &mut e,
        "image.canvas_size",
        json!({"width":9000,"height":9000,"anchor":"top_left"}),
    );
    let revision = e.revision();
    let before = e.document().unwrap().to_archive().unwrap();
    let cursor = e.snapshot().history_cursor;
    assert_eq!(
        run(&mut e, "image.resize", json!({"width":9000,"height":9000}))["changed"],
        false
    );
    assert_eq!(e.revision(), revision);
    assert_eq!(e.snapshot().history_cursor, cursor);
    assert_eq!(e.document().unwrap().to_archive().unwrap(), before);
}

#[test]
fn interior_transparent_layer_edges_resample_coverage_without_growing_the_object() {
    let mut source = Editor::new();
    run(
        &mut source,
        "file.new",
        json!({"width":4,"height":4,"bit_depth":16}),
    );
    paste(
        &mut source,
        PixelBuffer::Rgba16([10001, 30003, 50001, 65535].repeat(4)),
        2,
        2,
    );
    let id = source.snapshot().active_layer;
    run(
        &mut source,
        "layer.set_position",
        json!({"layer":{"id":id},"x":1,"y":1}),
    );
    for mode in ["bilinear", "area"] {
        let mut up = source.clone();
        run(
            &mut up,
            "image.resize",
            json!({"width":8,"height":8,"interpolation":mode}),
        );
        assert_eq!(pixel(&up, 0, 0)[3], 0);
        assert!(pixel(&up, 1, 1)[3].abs_diff(4096) <= 1);
        assert!(pixel(&up, 2, 2)[3].abs_diff(36863) <= 1);
        assert_eq!(pixel(&up, 3, 3)[3], 65535);
        let mut down = source.clone();
        run(
            &mut down,
            "image.resize",
            json!({"width":2,"height":2,"interpolation":mode}),
        );
        for y in 0..2 {
            for x in 0..2 {
                assert!(pixel(&down, x, y)[3].abs_diff(16384) <= 1);
            }
        }
    }
    let mut nearest = source;
    run(
        &mut nearest,
        "image.resize",
        json!({"width":2,"height":2,"interpolation":"nearest"}),
    );
    assert_eq!(pixel(&nearest, 0, 0)[3], 65535);
    for (x, y) in [(1, 0), (0, 1), (1, 1)] {
        assert_eq!(pixel(&nearest, x, y)[3], 0);
    }
}
