use serde_json::{Value, json};
use strata_core::{ClipboardImage, Editor, Source};
use strata_pixels::{PixelBuffer, PixelValue, memory::MemoryPool};
fn run(e: &mut Editor, id: &str, p: Value) -> Value {
    e.execute(e.request(Source::User), id, p, |_| Ok(()))
        .unwrap()
}
fn editor(depth: u8) -> Editor {
    let mut e = Editor::new();
    run(
        &mut e,
        "file.new",
        json!({"width":16,"height":16,"bit_depth":depth}),
    );
    e
}
fn rgba(e: &Editor, x: i64, y: i64) -> [u16; 4] {
    match e
        .document()
        .unwrap()
        .layers()
        .last()
        .unwrap()
        .pixels()
        .pixel(x, y)
        .unwrap()
    {
        PixelValue::Rgba8(v) => v.map(u16::from),
        PixelValue::Rgba16(v) => v,
    }
}
fn place(e: &mut Editor, pixels: PixelBuffer, width: u32, height: u32) {
    let image = ClipboardImage::new(
        pixels,
        width,
        height,
        0,
        0,
        e.document().unwrap().working_icc().to_vec(),
        &MemoryPool::global(),
    )
    .unwrap();
    let token = e.grant_clipboard_image(image, Source::User).unwrap();
    run(e, "edit.paste_in_place", json!({"resource":token}));
}
#[test]
fn point_sampling_preserves_raw_16bit_values_and_average_uses_alpha_weighted_linear_light() {
    let mut e = editor(16);
    place(
        &mut e,
        PixelBuffer::Rgba16(vec![
            32768, 32769, 50001, 0, 65535, 65535, 65535, 65535, 0, 0, 0, 65535,
        ]),
        3,
        1,
    );
    let before = e.snapshot().history.len();
    let revision = e.revision();
    let raw = run(
        &mut e,
        "paint.sample_color",
        json!({"x":0,"y":0,"sample":"current_layer"}),
    );
    assert_eq!(raw["color"][0], json!(32768.0 / 65535.0));
    assert_eq!(raw["color"][1], json!(32769.0 / 65535.0));
    assert_eq!(raw["color"][3], 0.0);
    let average = run(
        &mut e,
        "paint.sample_color",
        json!({"x":1,"y":0,"radius":1,"sample":"current_layer"}),
    );
    assert!((average["color"][0].as_f64().unwrap() - 0.73535698).abs() < 0.0001);
    assert!((average["color"][3].as_f64().unwrap() - 1.0 / 3.0).abs() < 1e-6);
    assert_eq!(e.revision(), revision);
    assert_eq!(e.snapshot().history.len(), before);
    let active = e.snapshot().active_layer;
    run(
        &mut e,
        "layer.set_visibility",
        json!({"layer":{"id":active},"visible":false}),
    );
    assert!(
        e.execute(
            e.request(Source::User),
            "paint.sample_color",
            json!({"x":1,"y":0,"sample":"current_layer"}),
            |_| Ok(())
        )
        .is_err()
    );
}
#[test]
fn fill_preserves_selected_boundary_and_source_codes_and_transparency_lock() {
    for depth in [8, 16] {
        let mut e = editor(depth);
        let max = if depth == 16 { 65535 } else { 255 };
        run(
            &mut e,
            "layer.create",
            json!({"name":"target","fill":[max/2,max/2+1,max*3/4,max/2]}),
        );
        let original = rgba(&e, 0, 0);
        run(
            &mut e,
            "select.marquee",
            json!({"x":8,"y":0,"width":8,"height":16}),
        );
        run(
            &mut e,
            "layer.set_locks",
            json!({"layer":{"name":"target"},"locks":{"transparency":true}}),
        );
        run(
            &mut e,
            "paint.fill",
            json!({"layer":{"name":"target"},"color":[1,0,0,1],"opacity":0.5}),
        );
        for y in 0..16 {
            for x in 0..8 {
                assert_eq!(rgba(&e, x, y), original);
            }
        }
        assert_ne!(&rgba(&e, 10, 10)[..3], &original[..3]);
        assert_eq!(rgba(&e, 10, 10)[3], original[3]);
        run(&mut e, "edit.undo", json!({}));
        assert_eq!(rgba(&e, 10, 10), original);
    }
}
#[test]
fn bucket_connected_and_global_sampling_preserve_current_selection() {
    let mut e = editor(16);
    let mut pixels = Vec::new();
    for _ in 0..16 {
        for x in 0..16 {
            pixels.extend(if (5..10).contains(&x) {
                [0, 0, 65535, 65535]
            } else {
                [65535, 0, 0, 65535]
            });
        }
    }
    place(&mut e, PixelBuffer::Rgba16(pixels), 16, 16);
    let id = e.snapshot().active_layer.unwrap();
    run(
        &mut e,
        "select.marquee",
        json!({"x":0,"y":4,"width":16,"height":8}),
    );
    let selection = e.document().unwrap().selection().unwrap().clone();
    let p = json!({"layer":{"id":id},"x":2,"y":8,"tolerance":0,"sample":"current_layer","color":[0,1,0,1]});
    run(&mut e, "paint.bucket", p.clone());
    assert!(rgba(&e, 2, 8)[1] > 65000);
    assert_eq!(rgba(&e, 12, 8), [65535, 0, 0, 65535]);
    assert_eq!(rgba(&e, 2, 2), [65535, 0, 0, 65535]);
    assert_eq!(
        e.document()
            .unwrap()
            .selection()
            .unwrap()
            .sample(2, 2)
            .unwrap(),
        selection.sample(2, 2).unwrap()
    );
    run(&mut e, "edit.undo", json!({}));
    let mut p = p;
    p["contiguous"] = json!(false);
    run(&mut e, "paint.bucket", p);
    assert!(rgba(&e, 12, 8)[1] > 65000);
    assert_eq!(rgba(&e, 7, 8), [0, 0, 65535, 65535]);
}
fn gradient(shape: &str) -> Value {
    json!({"layer":{"name":"Background"},"start":[8.5,8.5],"end":[12.5,8.5],"shape":shape,"stops":[{"at":0,"color":[0,0,0,1]},{"at":1,"color":[1,1,1,1]}]})
}
#[test]
fn all_five_gradient_shapes_match_fixed_pixel_center_geometry() {
    for shape in ["linear", "radial", "angle", "reflected", "diamond"] {
        let mut e = editor(16);
        run(&mut e, "paint.gradient", gradient(shape));
        assert_eq!(rgba(&e, 8, 8), [0, 0, 0, 65535]);
        let (x, y, t) = match shape {
            "linear" => (10, 8, 0.5),
            "radial" => (8, 10, 0.5),
            "angle" => (8, 12, 0.25),
            "reflected" => (6, 8, 0.5),
            _ => (9, 9, 0.5),
        };
        let expected = (t * 65535.0f64).round() as u16;
        assert!(rgba(&e, x, y)[0].abs_diff(expected) <= 4, "{shape}");
        let source = e.document().unwrap().to_archive().unwrap();
        let reopened = strata_core::Document::from_archive(source).unwrap();
        assert!(
            reopened.export_pixels().unwrap() == e.document().unwrap().export_pixels().unwrap()
        );
    }
}
#[test]
fn transparent_gradient_stops_do_not_leak_hidden_color_and_bad_parameters_are_atomic() {
    let mut e = editor(16);
    let mut p = gradient("linear");
    p["start"] = json!([0.5, 0.5]);
    p["end"] = json!([10.5, 0.5]);
    p["stops"] = json!([{"at":0,"color":[1,0,0,0]},{"at":1,"color":[0,0,1,1]}]);
    run(&mut e, "paint.gradient", p.clone());
    let middle = rgba(&e, 5, 0);
    assert!(middle[0] < 5 && middle[1] < 5 && middle[2] > 65000);
    assert!(middle[3].abs_diff(32768) <= 1);
    let revision = e.revision();
    for invalid in [
        json!({"end":[0.5,0.5]}),
        json!({"stops":[{"at":0,"color":[1,0,0,1]},{"at":0,"color":[0,1,0,1]}]}),
        json!({"opacity":2}),
    ] {
        let mut bad = p.clone();
        bad.as_object_mut()
            .unwrap()
            .extend(invalid.as_object().unwrap().clone());
        assert!(
            e.execute(e.request(Source::User), "paint.gradient", bad, |_| Ok(()))
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
            "paint.fill",
            json!({"layer":{"name":"Background"},"color":[1,0,0,1]}),
            |_| Ok(())
        )
        .is_err()
    );
}

#[test]
fn merged_sampler_uses_visible_adjustments_and_fill_cancellation_keeps_original() {
    let mut e = editor(16);
    run(
        &mut e,
        "paint.fill",
        json!({"layer":{"name":"Background"},"color":[0.2,0.3,0.4,1]}),
    );
    let before = run(&mut e, "paint.sample_color", json!({"x":3,"y":3}));
    run(
        &mut e,
        "layer.add_tonal_adjustment",
        json!({"name":"bright","adjustment":{"type":"brightness_contrast","brightness":20}}),
    );
    let after = run(&mut e, "paint.sample_color", json!({"x":3,"y":3}));
    assert!(
        (after["color"][0].as_f64().unwrap() - before["color"][0].as_f64().unwrap() - 0.2).abs()
            < 0.0001
    );
    let pool = MemoryPool::new(strata_pixels::memory::MemoryLimits {
        cpu_bytes: 128 * 1024 * 1024,
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
        snapshot.prepare(
            request,
            "paint.fill",
            json!({"layer":{"name":"Background"},"color":[1,0,0,1]}),
        )
    });
    let start = std::time::Instant::now();
    while pool.stats().transient_bytes < 6 * 1024 * 1024
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
    let mut limited =
        Editor::with_memory_pool(MemoryPool::new(strata_pixels::memory::MemoryLimits {
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
                "paint.gradient",
                gradient("linear"),
                |_| Ok(())
            )
            .is_err()
    );
    assert_eq!(limited.revision(), revision);
}
