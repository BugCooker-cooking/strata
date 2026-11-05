use serde_json::{Value, json};
use strata_core::{ClipboardImage, Document, Editor, Source};
use strata_pixels::{PixelBuffer, memory::MemoryPool};
fn run(e: &mut Editor, id: &str, p: Value) -> Value {
    e.execute(e.request(Source::User), id, p, |_| Ok(()))
        .unwrap()
}
fn clean(x: u32, y: u32, texture: bool) -> [u16; 4] {
    let v = 20000
        + if texture {
            ((x % 8) / 4) as u16 * 8000 + y as u16 * 40
        } else {
            0
        };
    [v, v, v, 65535]
}
fn fixture(depth: u8, texture: bool) -> Editor {
    let mut e = Editor::new();
    run(
        &mut e,
        "file.new",
        json!({"width":96,"height":48,"bit_depth":depth}),
    );
    let pixels = (0..48)
        .flat_map(|y| {
            (0..96).flat_map(move |x| {
                if (x as i32 - 48).pow(2) + (y as i32 - 24).pow(2) <= 4 {
                    [60000, 7000, 45000, 65535]
                } else {
                    clean(x, y, texture)
                }
            })
        })
        .collect::<Vec<_>>();
    let buffer = if depth == 16 {
        PixelBuffer::Rgba16(pixels)
    } else {
        PixelBuffer::Rgba8(pixels.into_iter().map(|v| (v / 257) as u8).collect())
    };
    let image = ClipboardImage::new(
        buffer,
        96,
        48,
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
fn params(e: &Editor) -> Value {
    json!({"stroke":{"layer":{"id":e.snapshot().active_layer},"points":[{"x":48.5,"y":24.5}],"size":12,"hardness":1},"search_radius":40})
}
fn pixel(e: &Editor, x: i64, y: i64) -> [u16; 4] {
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
        strata_pixels::PixelValue::Rgba16(v) => v,
        strata_pixels::PixelValue::Rgba8(v) => v.map(|v| u16::from(v) * 257),
    }
}

#[test]
fn automatic_flat_spot_removes_blemish_without_a_source_and_preserves_outside_exactly() {
    for depth in [8, 16] {
        let mut e = fixture(depth, false);
        let before = e.document().unwrap().to_archive().unwrap();
        let history = e.snapshot().history.len();
        let p = params(&e);
        let result = run(&mut e, "retouch.spot_heal", p);
        assert!(result["candidates"].as_u64().unwrap() <= 2048);
        assert!(result["source"].is_array());
        for c in 0..3 {
            assert!(
                pixel(&e, 48, 24)[c].abs_diff(if depth == 16 {
                    20000
                } else {
                    (20000 / 257) * 257
                }) <= if depth == 16 { 8 } else { 257 }
            );
        }
        let original = Document::from_archive(before.clone()).unwrap();
        for y in 0..48 {
            for x in 0..96 {
                if (x as f64 - 48.0).hypot(y as f64 - 24.0) > 7.0 {
                    assert_eq!(
                        e.document()
                            .unwrap()
                            .layers()
                            .last()
                            .unwrap()
                            .pixels()
                            .pixel(x, y)
                            .unwrap(),
                        original
                            .layers()
                            .last()
                            .unwrap()
                            .pixels()
                            .pixel(x, y)
                            .unwrap()
                    );
                }
            }
        }
        assert_eq!(e.snapshot().history.len(), history + 1);
        run(&mut e, "edit.undo", json!({}));
        assert!(e.document().unwrap().to_archive().unwrap() == before);
    }
}
#[test]
fn automatic_periodic_texture_matches_frozen_linear_reference_and_is_deterministic() {
    let mut e = fixture(16, true);
    let mut replay = e.clone();
    let p = params(&e);
    let result = run(&mut e, "retouch.spot_heal", p.clone());
    let repeated = run(&mut replay, "retouch.spot_heal", p);
    assert_eq!(result["source"], repeated["source"]);
    assert!(
        e.document().unwrap().export_pixels().unwrap()
            == replay.document().unwrap().export_pixels().unwrap()
    );
    let icc = e.document().unwrap().working_icc();
    for y in 22..=26 {
        for x in 46..=50 {
            let a = pixel(&e, x, y).map(|v| f32::from(v) / 65535.0);
            let b = clean(x as u32, y as u32, true).map(|v| f32::from(v) / 65535.0);
            let linear = strata_color::working_to_linear_p3(&[a, b], icc).unwrap();
            assert!(
                (0..3).all(|c| (linear[0][c] - linear[1][c]).abs() <= 0.003),
                "texture at {x},{y}: {:?} != {:?}",
                linear[0],
                linear[1]
            );
        }
    }
    let reopened = Document::from_archive(e.document().unwrap().to_archive().unwrap()).unwrap();
    assert!(reopened.export_pixels().unwrap() == e.document().unwrap().export_pixels().unwrap());
}
#[test]
fn selection_mask_locks_and_no_source_failures_are_atomic() {
    let mut e = fixture(16, true);
    let layer = e.snapshot().active_layer;
    run(
        &mut e,
        "layer.mask.add",
        json!({"layer":{"id":layer},"source":"white"}),
    );
    run(
        &mut e,
        "select.marquee",
        json!({"x":48,"y":0,"width":48,"height":48}),
    );
    let protected = pixel(&e, 47, 24);
    let p = params(&e);
    run(&mut e, "retouch.spot_heal", p);
    assert_eq!(pixel(&e, 47, 24), protected);
    assert!(
        e.document()
            .unwrap()
            .layers()
            .last()
            .unwrap()
            .info()
            .mask
            .is_some()
    );
    let mut invalid = params(&e);
    invalid["search_radius"] = json!(1);
    let archive = e.document().unwrap().to_archive().unwrap();
    let history = e.snapshot().history.len();
    assert!(
        e.execute(
            e.request(Source::User),
            "retouch.spot_heal",
            invalid,
            |_| Ok(())
        )
        .is_err()
    );
    assert!(e.document().unwrap().to_archive().unwrap() == archive);
    assert_eq!(e.snapshot().history.len(), history);
    run(
        &mut e,
        "layer.set_locks",
        json!({"layer":{"id":layer},"locks":{"pixels":true}}),
    );
    let revision = e.revision();
    let p = params(&e);
    assert!(
        e.execute(e.request(Source::User), "retouch.spot_heal", p, |_| Ok(()))
            .is_err()
    );
    assert_eq!(e.revision(), revision);
}

#[test]
fn invalid_parameters_transparency_and_merged_source_have_explicit_atomic_results() {
    let mut e = fixture(16, false);
    for extra in [
        json!({"search_radius":0}),
        json!({"search_radius":513}),
        json!({"max_iterations":0}),
        json!({"tolerance":0.1}),
    ] {
        let mut p = params(&e);
        p.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        let revision = e.revision();
        assert!(
            e.execute(e.request(Source::User), "retouch.spot_heal", p, |_| Ok(()))
                .is_err()
        );
        assert_eq!(e.revision(), revision);
    }
    let mut p = params(&e);
    p["stroke"]["clone_source"] = json!({"source":[10,20]});
    assert!(
        e.execute(e.request(Source::User), "retouch.spot_heal", p, |_| Ok(()))
            .is_err()
    );
    run(&mut e, "layer.create", json!({"name":"transparent target"}));
    let p = params(&e);
    let revision = e.revision();
    assert!(
        e.execute(e.request(Source::User), "retouch.spot_heal", p, |_| Ok(()))
            .is_err()
    );
    assert_eq!(e.revision(), revision);
    let mut p = params(&e);
    p["sample"] = json!("merged");
    run(&mut e, "retouch.spot_heal", p);
    assert!(pixel(&e, 48, 24)[3] > 65000);
    let before = e.document().unwrap().export_pixels().unwrap();
    let mut p = params(&e);
    p["stroke"]["opacity"] = json!(0);
    let r = run(&mut e, "retouch.spot_heal", p);
    assert!(r["source"].is_null());
    assert!(e.document().unwrap().export_pixels().unwrap() == before);
}

#[test]
fn search_obeys_shared_budget_and_cancellation_without_publishing() {
    use strata_pixels::memory::MemoryLimits;
    let pool = MemoryPool::new(MemoryLimits {
        cpu_bytes: 16 * 1024 * 1024,
        gpu_bytes: 0,
        disk_bytes: 64 * 1024 * 1024,
    });
    let mut e = Editor::with_memory_pool(pool);
    run(
        &mut e,
        "file.new",
        json!({"width":96,"height":48,"bit_depth":16,"background":[20000,30000,40000,65535]}),
    );
    let revision = e.revision();
    let p = params(&e);
    assert!(
        e.execute(e.request(Source::User), "retouch.spot_heal", p, |_| Ok(()))
            .is_err()
    );
    assert_eq!(e.revision(), revision);
    let pool = MemoryPool::new(MemoryLimits {
        cpu_bytes: 128 * 1024 * 1024,
        gpu_bytes: 0,
        disk_bytes: 128 * 1024 * 1024,
    });
    let mut e = Editor::with_memory_pool(pool.clone());
    run(
        &mut e,
        "file.new",
        json!({"width":256,"height":256,"bit_depth":16,"background":[20000,30000,40000,65535]}),
    );
    let revision = e.revision();
    let request = e.request(Source::User);
    let copy = e.clone();
    let worker = std::thread::spawn(move || {
        copy.prepare(request,"retouch.spot_heal",json!({"stroke":{"layer":{"name":"Background"},"points":[{"x":128.5,"y":128.5}],"size":60,"hardness":1},"search_radius":120}))
    });
    let start = std::time::Instant::now();
    while pool.stats().transient_bytes < 18 * 1024 * 1024
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
    assert_eq!(e.snapshot().history.len(), 0);
}
