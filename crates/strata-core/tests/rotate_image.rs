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
fn fixture(depth: u8) -> Editor {
    let mut e = Editor::new();
    run(
        &mut e,
        "file.new",
        json!({"width":16,"height":12,"bit_depth":depth,"working_space":"display_p3"}),
    );
    let data = (0..16 * 12)
        .flat_map(|i| {
            [
                10001 + (i % 16) as u16 * 1900,
                20003 + (i / 16) as u16 * 1700,
                50001,
                if i % 5 == 0 { 0 } else { 32769 },
            ]
        })
        .collect::<Vec<_>>();
    let p = if depth == 16 {
        PixelBuffer::Rgba16(data)
    } else {
        PixelBuffer::Rgba8(data.into_iter().map(|v| (v / 257) as u8).collect())
    };
    let image = ClipboardImage::new(
        p,
        16,
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
fn pixel(e: &Editor, x: i64, y: i64) -> [u16; 4] {
    let layer = e.document().unwrap().layers().last().unwrap();
    match layer
        .pixels()
        .pixel(x - layer.info().x, y - layer.info().y)
        .unwrap()
    {
        PixelValue::Rgba16(v) => v,
        PixelValue::Rgba8(v) => v.map(u16::from),
    }
}
fn reference(angle: f64) -> ([f64; 6], [u32; 2]) {
    let theta = angle.to_radians();
    let (c, s) = (theta.cos(), theta.sin());
    let tx = 8. - c * 8. + s * 6.;
    let ty = 6. - s * 8. - c * 6.;
    let points = [[-0.5, -0.5], [16.5, -0.5], [16.5, 12.5], [-0.5, 12.5]]
        .map(|[x, y]| [c * x - s * y + tx, s * x + c * y + ty]);
    let l = points
        .iter()
        .map(|p| p[0])
        .fold(f64::INFINITY, f64::min)
        .floor();
    let t = points
        .iter()
        .map(|p| p[1])
        .fold(f64::INFINITY, f64::min)
        .floor();
    let r = points
        .iter()
        .map(|p| p[0])
        .fold(f64::NEG_INFINITY, f64::max)
        .ceil();
    let b = points
        .iter()
        .map(|p| p[1])
        .fold(f64::NEG_INFINITY, f64::max)
        .ceil();
    (
        [c, s, -s, c, tx - l, ty - t],
        [(r - l) as u32, (b - t) as u32],
    )
}
#[test]
fn nearest_arbitrary_rotation_matches_independent_inverse_centers_and_exact_source_codes() {
    for depth in [8, 16] {
        for angle in [27., -27., 45., 137.5] {
            let mut e = fixture(depth);
            let original = e.clone();
            let before = e.document().unwrap().to_archive().unwrap();
            let history = e.snapshot().history.len();
            let result = run(
                &mut e,
                "image.rotate",
                json!({"angle_degrees":angle,"interpolation":"nearest"}),
            );
            let (m, size) = reference(angle);
            let frame = e.document().unwrap().frame();
            assert_eq!(
                (frame.x, frame.y, frame.width, frame.height),
                (0, 0, size[0], size[1])
            );
            assert_eq!(result["changed"], true);
            assert_eq!(e.snapshot().history.len(), history + 1);
            for y in 0..size[1] {
                for x in 0..size[0] {
                    let dx = f64::from(x) + 0.5 - m[4];
                    let dy = f64::from(y) + 0.5 - m[5];
                    let index = |v: f64| {
                        if (v - v.round()).abs() <= 1e-9 {
                            v.round() as i64
                        } else {
                            v.floor() as i64
                        }
                    };
                    let sx = index(m[0] * dx + m[1] * dy);
                    let sy = index(m[2] * dx + m[3] * dy);
                    let expected = if (0..16).contains(&sx) && (0..12).contains(&sy) {
                        pixel(&original, sx, sy)
                    } else {
                        [0; 4]
                    };
                    assert_eq!(
                        pixel(&e, i64::from(x), i64::from(y)),
                        expected,
                        "depth={depth} angle={angle} {x},{y}"
                    );
                }
            }
            let archive = e.document().unwrap().to_archive().unwrap();
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
}
#[test]
fn bilinear_arbitrary_rotation_uses_premultiplied_linear_light_and_same_canvas_envelope() {
    let mut e = fixture(16);
    let original = e.clone();
    run(&mut e, "image.rotate", json!({"angle_degrees":27}));
    let (m, size) = reference(27.);
    assert_eq!(
        (
            e.document().unwrap().frame().width,
            e.document().unwrap().frame().height
        ),
        (size[0], size[1])
    );
    let icc = original.document().unwrap().working_icc();
    for (x, y) in [(8i64, 7i64), (10, 9), (13, 8), (15, 12)] {
        let dx = x as f64 + 0.5 - m[4];
        let dy = y as f64 + 0.5 - m[5];
        let sx = m[0] * dx + m[1] * dy - 0.5;
        let sy = m[2] * dx + m[3] * dy - 0.5;
        let x0 = sx.floor() as i64;
        let y0 = sy.floor() as i64;
        let fx = sx - x0 as f64;
        let fy = sy - y0 as f64;
        let mut expected = [0.; 4];
        for (px, wx) in [(x0, 1. - fx), (x0 + 1, fx)] {
            for (py, wy) in [(y0, 1. - fy), (y0 + 1, fy)] {
                if !(0..16).contains(&px) || !(0..12).contains(&py) {
                    continue;
                }
                let p = pixel(&original, px, py);
                let linear =
                    strata_color::to_linear_p3(&PixelBuffer::Rgba16(p.to_vec()), 1, 1, icc)
                        .unwrap()[0]
                        .map(f64::from);
                for c in 0..3 {
                    expected[c] += linear[c] * linear[3] * wx * wy;
                }
                expected[3] += linear[3] * wx * wy;
            }
        }
        if expected[3] > 0. {
            for c in 0..3 {
                expected[c] /= expected[3];
            }
        }
        let PixelBuffer::Rgba16(codes) = strata_color::from_linear_p3(
            &[expected.map(|v| v as f32)],
            strata_pixels::PixelFormat::Rgba16,
            icc,
        )
        .unwrap() else {
            panic!()
        };
        let actual = pixel(&e, x, y);
        assert!(
            (0..4).all(|c| actual[c].abs_diff(codes[c]) <= 4),
            "{x},{y}: {actual:?} != {codes:?}"
        );
    }
}
#[test]
fn quarter_turns_delegate_all_current_editable_types_and_full_revolutions_are_noops() {
    let mut e = fixture(16);
    run(
        &mut e,
        "layer.fill.gradient",
        json!({"name":"fill","start":[1000,3000,5000,65535],"end":[50000,45000,40000,65535]}),
    );
    let a = e.clone();
    for (angle, operation) in [
        (90, "rotate_90_cw"),
        (-90, "rotate_90_ccw"),
        (180, "rotate_180"),
    ] {
        let mut expected = a.clone();
        let mut actual = a.clone();
        run(
            &mut expected,
            "image.orient",
            json!({"operation":operation}),
        );
        run(&mut actual, "image.rotate", json!({"angle_degrees":angle}));
        assert_eq!(
            expected.document().unwrap().export_pixels().unwrap(),
            actual.document().unwrap().export_pixels().unwrap()
        );
        assert_eq!(expected.snapshot().layers, actual.snapshot().layers);
        assert_eq!(
            expected.document().unwrap().frame(),
            actual.document().unwrap().frame()
        );
    }
    let before = e.document().unwrap().to_archive().unwrap();
    let revision = e.revision();
    let history = e.snapshot().history.len();
    for angle in [0, 360, -720] {
        assert_eq!(
            run(&mut e, "image.rotate", json!({"angle_degrees":angle}))["changed"],
            false
        );
        assert_eq!(e.revision(), revision);
        assert_eq!(e.snapshot().history.len(), history);
        assert_eq!(e.document().unwrap().to_archive().unwrap(), before);
    }
}
#[test]
fn group_masks_hidden_offboard_sources_and_selections_rotate_without_layer_flattening() {
    let mut e = fixture(16);
    let target = e.snapshot().active_layer;
    run(
        &mut e,
        "select.marquee",
        json!({"x":2,"y":3,"width":8,"height":6}),
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
        json!({"layer":{"id":target},"x":-12,"y":2}),
    );
    run(
        &mut e,
        "layer.set_visibility",
        json!({"layer":{"id":target},"visible":false}),
    );
    let group = run(
        &mut e,
        "layer.group",
        json!({"layers":[{"id":target}],"name":"group"}),
    )["layer_id"]
        .clone();
    run(
        &mut e,
        "layer.mask.add",
        json!({"layer":{"id":group},"source":"white"}),
    );
    let before = e.document().unwrap().to_archive().unwrap();
    let ids = e
        .document()
        .unwrap()
        .layers()
        .iter()
        .map(|l| l.id())
        .collect::<Vec<_>>();
    run(&mut e, "image.rotate", json!({"angle_degrees":27}));
    let doc = e.document().unwrap();
    assert_eq!(doc.layers().iter().map(|l| l.id()).collect::<Vec<_>>(), ids);
    let pixel_layer = doc
        .layers()
        .iter()
        .find(|l| Some(l.id()) == target)
        .unwrap();
    assert!(!pixel_layer.info().visible);
    assert!(!pixel_layer.info().mask.as_ref().unwrap().enabled);
    let bounds = strata_core::geometry_pixels::content_bounds(
        pixel_layer.pixels(),
        [pixel_layer.info().x, pixel_layer.info().y],
        &|| false,
    )
    .unwrap()
    .unwrap();
    assert!(bounds.x < doc.frame().x);
    assert_eq!(
        doc.selection().unwrap().layout().bounds.width,
        doc.frame().width
    );
    assert_eq!(
        doc.selection().unwrap().layout().bounds.height,
        doc.frame().height
    );
    assert_eq!(
        doc.layers()
            .iter()
            .find(|l| l.info().name == "group")
            .unwrap()
            .mask()
            .unwrap()
            .layout()
            .default_coverage,
        0
    );
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
#[test]
fn arbitrary_rotation_accepts_editable_types_and_styles_and_rejects_locks() {
    let mut e = fixture(16);
    for extra in ["fill", "adjustment", "style", "lock"] {
        let mut a = e.clone();
        match extra {
            "fill" => {
                run(
                    &mut a,
                    "layer.fill.solid",
                    json!({"name":"fill","color":[10001,30003,50001,65535]}),
                );
            }
            "adjustment" => {
                run(
                    &mut a,
                    "layer.add_tonal_adjustment",
                    json!({"name":"tone","adjustment":{"type":"brightness_contrast"}}),
                );
            }
            "style" => {
                let target = a.snapshot().active_layer;
                run(
                    &mut a,
                    "layer.set_styles",
                    json!({"layer":{"id":target},"styles":{"color_overlay":{"color":[10001,30003,50001,65535],"opacity":0.5}}}),
                );
            }
            _ => {
                let target = a.snapshot().active_layer;
                run(
                    &mut a,
                    "layer.set_locks",
                    json!({"layer":{"id":target},"locks":{"position":true}}),
                );
            }
        }
        let before = a.document().unwrap().to_archive().unwrap();
        let revision = a.revision();
        let result = a.execute(
            a.request(Source::User),
            "image.rotate",
            json!({"angle_degrees":27}),
            |_| Ok(()),
        );
        if extra == "lock" {
            assert!(result.is_err());
            assert_eq!(a.revision(), revision);
        } else {
            assert_eq!(result.unwrap()["changed"], true);
            run(&mut a, "edit.undo", json!({}));
        }
        assert_eq!(a.document().unwrap().to_archive().unwrap(), before);
    }
    for p in [
        json!({"angle_degrees":360001}),
        json!({"angle_degrees":15,"expand":false}),
        json!({"angle_degrees":15,"interpolation":"guess"}),
    ] {
        assert!(
            e.execute(e.request(Source::User), "image.rotate", p, |_| Ok(()))
                .is_err()
        );
    }
    let target = e.snapshot().active_layer;
    run(
        &mut e,
        "layer.set_position",
        json!({"layer":{"id":target},"x":999999999980i64,"y":0}),
    );
    run(
        &mut e,
        "image.crop",
        json!({"rect":{"x":999999999980i64,"y":0,"width":16,"height":12},"pixels":"preserve"}),
    );
    let before = e.document().unwrap().to_archive().unwrap();
    assert!(
        e.execute(
            e.request(Source::User),
            "image.rotate",
            json!({"angle_degrees":27}),
            |_| Ok(())
        )
        .is_err()
    );
    assert_eq!(e.document().unwrap().to_archive().unwrap(), before);
}
#[test]
fn rotate_low_budget_and_actual_cancellation_keep_original_frame_and_samples() {
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
        let frame = e.document().unwrap().frame().clone();
        let revision = e.revision();
        if budget < 16 * 1024 * 1024 {
            assert!(
                e.execute(
                    e.request(Source::User),
                    "image.rotate",
                    json!({"angle_degrees":27}),
                    |_| Ok(())
                )
                .is_err()
            );
        } else {
            let source = e.clone();
            let request = e.request(Source::User);
            let worker = std::thread::spawn(move || {
                source.prepare(request, "image.rotate", json!({"angle_degrees":27}))
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
        assert_eq!(e.document().unwrap().frame(), &frame);
        assert!(e.snapshot().history.is_empty());
        assert_eq!(pool.stats().transient_bytes, 0);
    }
}
