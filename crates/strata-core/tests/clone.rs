use serde_json::{Value, json};
use strata_core::{ClipboardImage, Editor, Source};
use strata_pixels::{PixelBuffer, PixelValue, memory::MemoryPool};
fn run(e: &mut Editor, id: &str, p: Value) -> Value {
    e.execute(e.request(Source::User), id, p, |_| Ok(()))
        .unwrap()
}
fn source(width: u32, depth: u8, mut sample: impl FnMut(u32, u32) -> [u16; 4]) -> Editor {
    let mut e = Editor::new();
    run(
        &mut e,
        "file.new",
        json!({"width":width,"height":16,"bit_depth":depth}),
    );
    let mut pixels = Vec::new();
    for y in 0..16 {
        for x in 0..width {
            pixels.extend(sample(x, y));
        }
    }
    let buffer = if depth == 16 {
        PixelBuffer::Rgba16(pixels)
    } else {
        PixelBuffer::Rgba8(pixels.into_iter().map(|v| (v / 257) as u8).collect())
    };
    let image = ClipboardImage::new(
        buffer,
        width,
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
fn rgba(e: &Editor, x: i64, y: i64) -> [u16; 4] {
    match e
        .document()
        .unwrap()
        .layers()
        .iter()
        .find(|l| Some(l.id()) == e.snapshot().active_layer)
        .unwrap()
        .pixels()
        .pixel(x, y)
        .unwrap()
    {
        PixelValue::Rgba8(v) => v.map(u16::from),
        PixelValue::Rgba16(v) => v,
    }
}
fn parameters(e: &Editor) -> Value {
    json!({"layer":{"id":e.snapshot().active_layer},"size":4,"hardness":1,"points":[{"x":12.5,"y":8.5}],"clone_source":{"source":[3.5,8.5]}})
}
#[test]
fn integer_clone_samples_immutable_original_across_overlapping_target_and_has_one_history() {
    for depth in [8, 16] {
        let mut e = source(32, depth, |x, _| {
            let v = 1000 + x as u16 * 1500;
            [v, v + 1, v + 2, 65535]
        });
        let before = (0..32).map(|x| rgba(&e, x, 8)).collect::<Vec<_>>();
        let history = e.snapshot().history.len();
        let mut p = parameters(&e);
        p["size"] = json!(2);
        p["spacing"] = json!(0.2);
        p["points"] = json!([{"x":10.5,"y":8.5},{"x":25.5,"y":8.5}]);
        p["clone_source"]["source"] = json!([0.5, 8.5]);
        run(&mut e, "retouch.clone", p);
        for x in 11..25 {
            for (c, expected) in before[(x - 10) as usize].iter().copied().enumerate() {
                assert!(
                    rgba(&e, x, 8)[c].abs_diff(expected) <= if depth == 16 { 4 } else { 1 },
                    "self-feedback depth {depth} at {x}"
                );
            }
        }
        assert_eq!(e.snapshot().history.len(), history + 1);
        run(&mut e, "edit.undo", json!({}));
        assert_eq!(rgba(&e, 20, 8), before[20]);
    }
}
#[test]
fn fractional_source_interpolates_premultiplied_alpha_without_hidden_color_bleed() {
    let mut e = source(16, 16, |x, _| {
        if x == 2 {
            [65535, 0, 65535, 0]
        } else if x == 3 {
            [0, 0, 65535, 65535]
        } else {
            [0, 0, 0, 0]
        }
    });
    let mut p = parameters(&e);
    p["clone_source"]["source"] = json!([3.0, 8.5]);
    run(&mut e, "retouch.clone", p);
    let actual = rgba(&e, 12, 8);
    assert!(actual[0] < 5 && actual[1] < 5 && actual[2] > 65000);
    assert!(actual[3].abs_diff(32768) <= 1);
}
#[test]
fn selection_and_transparency_protect_targets_and_source_layer_offset_is_canvas_relative() {
    let mut e = source(32, 16, |x, _| {
        if x < 8 {
            [65535, 0, 0, 65535]
        } else {
            [10000, 20000, 30000, 32768]
        }
    });
    let original = rgba(&e, 12, 8);
    run(
        &mut e,
        "select.marquee",
        json!({"x":12,"y":0,"width":2,"height":16}),
    );
    let active = e.snapshot().active_layer;
    run(
        &mut e,
        "layer.set_locks",
        json!({"layer":{"id":active},"locks":{"transparency":true}}),
    );
    let mut p = parameters(&e);
    p["size"] = json!(12);
    run(&mut e, "retouch.clone", p);
    assert_eq!(rgba(&e, 11, 8), original);
    assert_eq!(rgba(&e, 12, 8)[3], 32768);
    assert!(rgba(&e, 12, 8)[0] > 65000);
    run(&mut e, "select.none", json!({}));
    let target = e.snapshot().active_layer.unwrap();
    run(
        &mut e,
        "layer.set_position",
        json!({"layer":{"id":target},"x":3,"y":0}),
    );
    let mut p = parameters(&e);
    p["points"] = json!([{"x":20.5,"y":8.5}]);
    p["clone_source"]["source"] = json!([4.5, 8.5]);
    run(&mut e, "retouch.clone", p);
    assert!(rgba(&e, 17, 8)[0] > 65000);
}
#[test]
fn explicit_destination_keeps_alignment_across_separate_strokes_and_project_roundtrips() {
    let mut e = source(32, 16, |x, _| {
        let v = 1000 + x as u16 * 1500;
        [v, v, v, 65535]
    });
    let original = rgba(&e, 8, 8);
    let mut p = parameters(&e);
    p["clone_source"] = json!({"source":[3.5,8.5],"destination":[12.5,8.5]});
    p["points"] = json!([{"x":17.5,"y":8.5}]);
    run(&mut e, "retouch.clone", p);
    assert!(
        rgba(&e, 17, 8)
            .into_iter()
            .zip(original)
            .all(|(a, b)| a.abs_diff(b) <= 4)
    );
    let archive = e.document().unwrap().to_archive().unwrap();
    let reopened = strata_core::Document::from_archive(archive).unwrap();
    assert!(e.document().unwrap().export_pixels().unwrap() == reopened.export_pixels().unwrap());
    let revision = e.revision();
    let mut p = parameters(&e);
    p["clone_source"]["source"] = json!([-1, 0]);
    assert!(
        e.execute(e.request(Source::User), "retouch.clone", p, |_| Ok(()))
            .is_err()
    );
    assert_eq!(e.revision(), revision);
    let p = parameters(&e);
    assert!(
        e.execute(e.request(Source::User), "paint.stroke", p, |_| Ok(()))
            .is_err()
    );
}

#[test]
fn merged_sampling_includes_visible_layers_and_raw_mode_ignores_its_appearance() {
    for merged in [false, true] {
        let mut e = source(16, 16, |_, _| [40000, 10000, 20000, 65535]);
        let target = e.snapshot().active_layer.unwrap();
        run(
            &mut e,
            "layer.fill.solid",
            json!({"name":"visible overlay","color":[10000,50000,30000,65535]}),
        );
        run(&mut e, "layer.select", json!({"layer":{"id":target}}));
        let mut p = parameters(&e);
        p["clone_source"]["sample"] = json!(if merged { "merged" } else { "current_layer" });
        run(&mut e, "retouch.clone", p);
        let actual = rgba(&e, 12, 8);
        let expected = if merged {
            [10000, 50000, 30000, 65535]
        } else {
            [40000, 10000, 20000, 65535]
        };
        for c in 0..4 {
            assert!(actual[c].abs_diff(expected[c]) <= 4);
        }
    }
}

#[test]
fn clone_crosses_source_and_target_tiles_and_cancellation_keeps_original() {
    let mut e = source(520, 16, |x, _| {
        let v = 1000 + (x % 256) as u16 * 200;
        [v, v + 1, v + 2, 65535]
    });
    let before = rgba(&e, 256, 8);
    let mut p = parameters(&e);
    p["points"] = json!([{"x":256.5,"y":8.5}]);
    p["clone_source"]["source"] = json!([255.5, 8.5]);
    p["size"] = json!(12);
    run(&mut e, "retouch.clone", p);
    let changed = rgba(&e, 256, 8);
    assert!(changed[0].abs_diff(52000) <= 4);
    assert_ne!(changed, before);
    use strata_pixels::memory::{MemoryLimits, MemoryPool};
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
    let request = e.request(Source::User);
    let snapshot = e.clone();
    let worker = std::thread::spawn(move || {
        snapshot.prepare(request,"retouch.clone",json!({"layer":{"name":"Background"},"points":[{"x":1024.5,"y":1024.5}],"size":4096,"clone_source":{"source":[512.5,512.5]}}))
    });
    let start = std::time::Instant::now();
    while pool.stats().transient_bytes < 15 * 1024 * 1024
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
}
