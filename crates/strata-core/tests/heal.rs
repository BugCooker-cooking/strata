use serde_json::{Value, json};
use strata_core::{ClipboardImage, Editor, Source};
use strata_pixels::{PixelBuffer, PixelValue, memory::MemoryPool};
fn run(e: &mut Editor, id: &str, p: Value) -> Value {
    e.execute(e.request(Source::User), id, p, |_| Ok(()))
        .unwrap()
}
fn editor(mut value: impl FnMut(u32, u32) -> [u16; 4]) -> Editor {
    let mut e = Editor::new();
    run(
        &mut e,
        "file.new",
        json!({"width":40,"height":20,"bit_depth":16}),
    );
    let mut pixels = Vec::new();
    for y in 0..20 {
        for x in 0..40 {
            pixels.extend(value(x, y));
        }
    }
    let image = ClipboardImage::new(
        PixelBuffer::Rgba16(pixels),
        40,
        20,
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
        .last()
        .unwrap()
        .pixels()
        .pixel(x, y)
        .unwrap()
    {
        PixelValue::Rgba16(v) => v,
        _ => panic!(),
    }
}
fn params(e: &Editor) -> Value {
    json!({"stroke":{"layer":{"id":e.snapshot().active_layer},"points":[{"x":29.5,"y":9.5}],"size":10,"hardness":1,"clone_source":{"source":[9.5,9.5]}}})
}
#[test]
fn constant_source_is_matched_to_target_boundary_instead_of_pasted_as_clone() {
    let mut e = editor(|x, _| {
        if x < 20 {
            [15000, 15000, 15000, 65535]
        } else {
            [45000, 45000, 45000, 65535]
        }
    });
    let history = e.snapshot().history.len();
    let before = e.document().unwrap().export_pixels().unwrap();
    let p = params(&e);
    let result = run(&mut e, "retouch.heal", p.clone());
    assert!(result["residual_rms"].as_f64().unwrap() <= 0.00005);
    assert!(result["domain_pixels"].as_u64().unwrap() > 0);
    for c in 0..3 {
        assert!(rgba(&e, 29, 9)[c].abs_diff(45000) <= 4);
    }
    assert_eq!(e.snapshot().history.len(), history + 1);
    run(&mut e, "edit.undo", json!({}));
    assert!(e.document().unwrap().export_pixels().unwrap() == before);
    run(&mut e, "retouch.clone", p["stroke"].clone());
    assert!(rgba(&e, 29, 9)[0].abs_diff(15000) <= 4);
}
#[test]
fn source_texture_is_preserved_while_low_frequency_target_brightness_and_boundary_are_matched() {
    let mut e = editor(|x, y| {
        let base = if x < 20 { 18000 } else { 44000 };
        let texture = if x < 20 && ((x + y).is_multiple_of(2)) {
            6000
        } else {
            0
        };
        let value = base + texture;
        [value, value, value, 65535]
    });
    let p = params(&e);
    let result = run(&mut e, "retouch.heal", p);
    assert!(result["residual_rms"].as_f64().unwrap() <= 0.00005);
    let a = rgba(&e, 28, 9)[0];
    let b = rgba(&e, 29, 9)[0];
    assert!(a.abs_diff(b) > 1000, "healing discarded source texture");
    assert!(
        a > 38000 && b > 38000,
        "healing copied source low frequency instead of target boundary"
    );
    for x in [0, 20, 39] {
        assert_eq!(rgba(&e, x, 9)[3], 65535);
    }
    assert_eq!(rgba(&e, 39, 9)[0], 44000);
    let archive = e.document().unwrap().to_archive().unwrap();
    let reopened = strata_core::Document::from_archive(archive).unwrap();
    assert!(reopened.export_pixels().unwrap() == e.document().unwrap().export_pixels().unwrap());
}
#[test]
fn selection_and_soft_opacity_apply_once_and_nonconvergence_is_atomic() {
    let mut e = editor(|x, y| {
        let value = if x < 20 {
            10000 + ((x * 7 + y * 11) % 5) as u16 * 5000
        } else {
            40000
        };
        [value, value, value, 65535]
    });
    run(
        &mut e,
        "select.marquee",
        json!({"x":29,"y":0,"width":11,"height":20}),
    );
    let before = rgba(&e, 28, 9);
    let mut p = params(&e);
    p["stroke"]["opacity"] = json!(0.5);
    run(&mut e, "retouch.heal", p.clone());
    assert_eq!(rgba(&e, 28, 9), before);
    let state = e.document().unwrap().state_id();
    let history = e.snapshot().history.len();
    p["max_iterations"] = json!(1);
    p["tolerance"] = json!(0.0000001);
    let error = e
        .execute(e.request(Source::User), "retouch.heal", p, |_| Ok(()))
        .unwrap_err();
    assert!(error.to_string().contains("did not converge"));
    assert_eq!(e.document().unwrap().state_id(), state);
    assert_eq!(e.snapshot().history.len(), history);
}
#[test]
fn invalid_limits_missing_source_and_transparent_domain_do_not_damage_target() {
    let mut e = editor(|_, _| [20000, 30000, 40000, 0]);
    let p = params(&e);
    let before = e.document().unwrap().export_pixels().unwrap();
    let result = run(&mut e, "retouch.heal", p.clone());
    assert_eq!(result["domain_pixels"], 0);
    assert!(e.document().unwrap().export_pixels().unwrap() == before);
    for change in [json!({"max_iterations":0}), json!({"tolerance":0.1})] {
        let mut value = p.clone();
        value
            .as_object_mut()
            .unwrap()
            .extend(change.as_object().unwrap().clone());
        let revision = e.revision();
        assert!(
            e.execute(e.request(Source::User), "retouch.heal", value, |_| Ok(()))
                .is_err()
        );
        assert_eq!(e.revision(), revision);
    }
    let mut bad = p;
    bad["stroke"]
        .as_object_mut()
        .unwrap()
        .remove("clone_source");
    assert!(
        e.execute(e.request(Source::User), "retouch.heal", bad, |_| Ok(()))
            .is_err()
    );
}

#[test]
fn soft_selection_opacity_and_pressure_are_applied_once_in_linear_light() {
    let mut e = editor(|x, y| {
        let value = if x == 9 && y == 9 {
            30000
        } else if x < 20 {
            15000
        } else {
            45000
        };
        [value, value, value, 65535]
    });
    run(
        &mut e,
        "select.marquee",
        json!({"x":29,"y":0,"width":11,"height":20}),
    );
    run(&mut e, "select.feather", json!({"radius":3}));
    let selected = f64::from(
        e.document()
            .unwrap()
            .selection()
            .unwrap()
            .sample(29, 9)
            .unwrap(),
    ) / 65535.0;
    assert!(selected > 0.0 && selected < 1.0);
    let original = rgba(&e, 19, 9);
    let mut p = params(&e);
    p["stroke"]["opacity"] = json!(0.4);
    p["stroke"]["pressure_opacity"] = json!(true);
    p["stroke"]["points"][0]["pressure"] = json!(0.5);
    let result = run(&mut e, "retouch.heal", p);
    assert!(result["residual_rms"].as_f64().unwrap() <= 0.00005);
    let values = [15000.0, 30000.0, 45000.0].map(|v| {
        [
            v as f32 / 65535.0,
            v as f32 / 65535.0,
            v as f32 / 65535.0,
            1.0,
        ]
    });
    let icc = e.document().unwrap().working_icc();
    let linear = strata_color::working_to_linear_p3(&values, icc).unwrap();
    let weight = selected * 0.4 * (32768.0 / 65535.0);
    let expected = std::array::from_fn(|c| {
        if c == 3 {
            1.0
        } else {
            linear[2][c] + (linear[1][c] - linear[0][c]) * weight as f32
        }
    });
    let encoded = strata_color::linear_p3_to_working(&[expected], icc).unwrap()[0];
    for (c, expected) in encoded.iter().take(3).enumerate() {
        assert!(
            rgba(&e, 29, 9)[c].abs_diff((expected * 65535.0).round() as u16) <= 8,
            "coverage was applied more than once"
        );
    }
    assert_eq!(rgba(&e, 19, 9), original);
}

#[test]
fn shifted_layer_healing_keeps_existing_mask_and_supports_both_depths_and_working_spaces() {
    for depth in [8, 16] {
        for profile in ["srgb", "display_p3"] {
            let mut e = Editor::new();
            run(
                &mut e,
                "file.new",
                json!({"width":40,"height":20,"bit_depth":depth,"working_space":profile}),
            );
            let mut pixels = Vec::new();
            for _ in 0..20 {
                for x in 0..40 {
                    let value: u16 = if x < 20 { 15420 } else { 44975 };
                    pixels.extend([value, value, value, 65535]);
                }
            }
            let buffer = if depth == 16 {
                PixelBuffer::Rgba16(pixels)
            } else {
                PixelBuffer::Rgba8(pixels.into_iter().map(|v| (v / 257) as u8).collect())
            };
            let image = ClipboardImage::new(
                buffer,
                40,
                20,
                0,
                0,
                e.document().unwrap().working_icc().to_vec(),
                &MemoryPool::global(),
            )
            .unwrap();
            let token = e.grant_clipboard_image(image, Source::User).unwrap();
            run(&mut e, "edit.paste_in_place", json!({"resource":token}));
            let layer = e.snapshot().active_layer.unwrap();
            run(
                &mut e,
                "layer.mask.add",
                json!({"layer":{"id":layer},"source":"white"}),
            );
            run(
                &mut e,
                "layer.set_position",
                json!({"layer":{"id":layer},"x":3,"y":1}),
            );
            let archive = e.document().unwrap().to_archive().unwrap();
            run(
                &mut e,
                "retouch.heal",
                json!({"stroke":{"layer":{"id":layer},"points":[{"x":32.5,"y":10.5}],"size":10,"hardness":1,"clone_source":{"source":[12.5,10.5]}}}),
            );
            let expected = if depth == 16 { 44975 } else { 175 };
            let value = e
                .document()
                .unwrap()
                .layers()
                .last()
                .unwrap()
                .pixels()
                .pixel(29, 9)
                .unwrap();
            let value = match value {
                PixelValue::Rgba8(v) => v.map(u16::from),
                PixelValue::Rgba16(v) => v,
            };
            assert!(value[0].abs_diff(expected) <= if depth == 16 { 5 } else { 1 });
            let after = e.document().unwrap().to_archive().unwrap();
            assert_eq!(after.metadata["layers"], archive.metadata["layers"]);
            let mask_tiles = |a: &strata_io::project::ProjectArchive| {
                a.tiles
                    .iter()
                    .filter(|t| t.layer.contains("mask"))
                    .cloned()
                    .collect::<Vec<_>>()
            };
            assert!(
                mask_tiles(&after) == mask_tiles(&archive),
                "healing replaced the existing linked mask"
            );
            run(&mut e, "edit.undo", json!({}));
            assert!(e.document().unwrap().to_archive().unwrap() == archive);
        }
    }
}

#[test]
fn healing_uses_shared_budget_and_cancels_solver_without_publishing() {
    use strata_pixels::memory::{MemoryLimits, MemoryPool};
    let mut limited = Editor::with_memory_pool(MemoryPool::new(MemoryLimits {
        cpu_bytes: 16 * 1024 * 1024,
        gpu_bytes: 0,
        disk_bytes: 32 * 1024 * 1024,
    }));
    run(
        &mut limited,
        "file.new",
        json!({"width":32,"height":20,"bit_depth":16,"background":[25000,35000,45000,65535]}),
    );
    let revision = limited.revision();
    let p = json!({"stroke":{"layer":{"name":"Background"},"points":[{"x":20.5,"y":10.5}],"size":10,"clone_source":{"source":[5.5,10.5]}}});
    assert!(
        limited
            .execute(limited.request(Source::User), "retouch.heal", p, |_| Ok(()))
            .is_err()
    );
    assert_eq!(limited.revision(), revision);
    let pool = MemoryPool::new(MemoryLimits {
        cpu_bytes: 256 * 1024 * 1024,
        gpu_bytes: 0,
        disk_bytes: 256 * 1024 * 1024,
    });
    let mut e = Editor::with_memory_pool(pool.clone());
    run(
        &mut e,
        "file.new",
        json!({"width":512,"height":512,"bit_depth":16,"background":[25000,35000,45000,65535]}),
    );
    let revision = e.revision();
    let snapshot = e.clone();
    let request = e.request(Source::User);
    let worker = std::thread::spawn(move || {
        snapshot.prepare(request,"retouch.heal",json!({"stroke":{"layer":{"name":"Background"},"points":[{"x":256.5,"y":256.5}],"size":180,"clone_source":{"source":[100.5,100.5]}}}))
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
    assert_eq!(e.snapshot().history.len(), 0);
}
