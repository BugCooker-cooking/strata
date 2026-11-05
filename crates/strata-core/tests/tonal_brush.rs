use serde_json::{Value, json};
use strata_core::{ClipboardImage, Document, Editor, Source};
use strata_pixels::{PixelBuffer, PixelValue, memory::MemoryPool};
fn run(e: &mut Editor, id: &str, p: Value) -> Value {
    e.execute(e.request(Source::User), id, p, |_| Ok(()))
        .unwrap()
}
fn fixture(width: u32, depth: u8, space: &str, pixel: [u16; 4]) -> Editor {
    let mut e = Editor::new();
    run(
        &mut e,
        "file.new",
        json!({"width":width,"height":16,"bit_depth":depth,"working_space":space}),
    );
    let buffer = if depth == 16 {
        PixelBuffer::Rgba16(pixel.repeat((width * 16) as usize))
    } else {
        PixelBuffer::Rgba8(pixel.map(|v| (v / 257) as u8).repeat((width * 16) as usize))
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
fn pixel(e: &Editor, x: i64, y: i64) -> [u16; 4] {
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
        PixelValue::Rgba16(v) => v,
        PixelValue::Rgba8(v) => v.map(|v| u16::from(v) * 257),
    }
}
fn params(e: &Editor) -> Value {
    json!({"stroke":{"layer":{"id":e.snapshot().active_layer},"points":[{"x":12.5,"y":8.5}],"size":6,"hardness":1},"exposure":0.5,"range":"midtones","protect_tones":true})
}
fn linear(e: &Editor, p: [u16; 4]) -> [f32; 4] {
    strata_color::working_to_linear_p3(
        &[p.map(|v| f32::from(v) / 65535.0)],
        e.document().unwrap().working_icc(),
    )
    .unwrap()[0]
}
fn y(p: [f32; 4]) -> f64 {
    f64::from(p[0]) * 0.22897456 + f64::from(p[1]) * 0.69173852 + f64::from(p[2]) * 0.07928692
}

#[test]
fn dodge_and_burn_match_fixed_neutral_reference_and_keep_alpha_across_depths_and_icc() {
    for depth in [8, 16] {
        for space in ["srgb", "display_p3"] {
            for (command, expected) in [("retouch.dodge", 41139u16), ("retouch.burn", 24993u16)] {
                let mut e = fixture(32, depth, space, [32768, 32768, 32768, 32768]);
                let before = pixel(&e, 12, 8);
                let protected = pixel(&e, 0, 0);
                let p = params(&e);
                run(&mut e, command, p);
                let after = pixel(&e, 12, 8);
                assert_eq!(after[3], before[3]);
                assert_eq!(pixel(&e, 0, 0), protected);
                if depth == 16 {
                    assert!(
                        after[0].abs_diff(expected) <= 16,
                        "{command} {space}: {after:?} != {expected}"
                    );
                }
                assert!(if command == "retouch.dodge" {
                    after[0] > before[0]
                } else {
                    after[0] < before[0]
                });
            }
        }
    }
}
#[test]
fn tonal_ranges_isolate_shadows_midtones_highlights_and_protection_keeps_endpoints() {
    for (value, best) in [
        (5000, "shadows"),
        (30000, "midtones"),
        (58000, "highlights"),
    ] {
        let original = fixture(32, 16, "srgb", [value, value, value, 65535]);
        let mut changes = Vec::new();
        for range in ["shadows", "midtones", "highlights"] {
            let mut e = original.clone();
            let mut p = params(&e);
            p["range"] = json!(range);
            run(&mut e, "retouch.dodge", p);
            changes.push((
                range,
                y(linear(&e, pixel(&e, 12, 8))) - y(linear(&original, pixel(&original, 12, 8))),
            ));
        }
        let strongest = changes.iter().max_by(|a, b| a.1.total_cmp(&b.1)).unwrap();
        assert_eq!(strongest.0, best, "range weights {changes:?}");
    }
    for value in [0, 65535] {
        for command in ["retouch.dodge", "retouch.burn"] {
            let mut e = fixture(32, 16, "srgb", [value, value, value, 65535]);
            let before = pixel(&e, 12, 8);
            let mut p = params(&e);
            p["range"] = json!(if value == 0 { "shadows" } else { "highlights" });
            p["exposure"] = json!(1);
            run(&mut e, command, p);
            assert_eq!(pixel(&e, 12, 8), before);
        }
    }
    let mut e = fixture(32, 16, "srgb", [65535, 65535, 65535, 65535]);
    let mut p = params(&e);
    p["range"] = json!("highlights");
    p["protect_tones"] = json!(false);
    run(&mut e, "retouch.burn", p);
    assert!(pixel(&e, 12, 8)[0] < 60000);
}
#[test]
fn sponge_keeps_physical_luminance_and_alpha_and_supports_saturation_and_vibrance() {
    for space in ["srgb", "display_p3"] {
        let original = fixture(32, 16, space, [20000, 40000, 50000, 32769]);
        let before = pixel(&original, 12, 8);
        let l = y(linear(&original, before));
        let chroma = |v: [u16; 4]| v[0].max(v[1]).max(v[2]) - v[0].min(v[1]).min(v[2]);
        for mode in ["desaturate", "saturate"] {
            for vibrance in [false, true] {
                let mut e = original.clone();
                let stroke = params(&e)["stroke"].clone();
                run(
                    &mut e,
                    "retouch.sponge",
                    json!({"stroke":stroke,"mode":mode,"amount":1,"vibrance":vibrance}),
                );
                let after = pixel(&e, 12, 8);
                assert_eq!(after[3], before[3]);
                assert!(
                    (y(linear(&e, after)) - l).abs() < 0.0001,
                    "{space} {mode} {vibrance}: {:?} Y {} != {}",
                    after,
                    y(linear(&e, after)),
                    l
                );
                assert!(if mode == "saturate" {
                    chroma(after) > chroma(before)
                } else {
                    chroma(after) < chroma(before)
                });
                if mode == "desaturate" && !vibrance {
                    assert!(chroma(after) <= 8);
                }
            }
        }
    }
    let mut e = fixture(32, 16, "srgb", [50000, 12000, 34000, 0]);
    let before = e.document().unwrap().to_archive().unwrap();
    let stroke = params(&e)["stroke"].clone();
    run(
        &mut e,
        "retouch.sponge",
        json!({"stroke":stroke,"amount":1}),
    );
    assert!(e.document().unwrap().to_archive().unwrap().tiles == before.tiles);
}
#[test]
fn soft_selection_pressure_and_opacity_blend_one_local_effect_in_linear_light() {
    let mut e = fixture(32, 16, "srgb", [32768, 32769, 40001, 32768]);
    let mut full = e.clone();
    let p = params(&full);
    run(&mut full, "retouch.dodge", p);
    let before = pixel(&e, 12, 8);
    let protected = pixel(&e, 0, 0);
    let a = linear(&e, before);
    let b = linear(&full, pixel(&full, 12, 8));
    run(
        &mut e,
        "select.marquee",
        json!({"x":12,"y":0,"width":20,"height":16}),
    );
    run(&mut e, "select.feather", json!({"radius":3}));
    let coverage = f64::from(
        e.document()
            .unwrap()
            .selection()
            .unwrap()
            .sample(12, 8)
            .unwrap(),
    ) / 65535.0;
    let mut p = params(&e);
    p["stroke"]["opacity"] = json!(0.4);
    p["stroke"]["points"][0]["pressure"] = json!(0.5);
    p["stroke"]["pressure_size"] = json!(false);
    run(&mut e, "retouch.dodge", p);
    let after = pixel(&e, 12, 8);
    let actual = linear(&e, after);
    let weight = coverage * 0.4 * 32768.0 / 65535.0;
    for c in 0..3 {
        assert!(
            (f64::from(actual[c])
                - (f64::from(a[c]) + (f64::from(b[c]) - f64::from(a[c])) * weight))
                .abs()
                < 0.0001
        );
    }
    assert_eq!(after[3], before[3]);
    assert_eq!(pixel(&e, 0, 0), protected);
}
#[test]
fn offset_cross_tile_brush_preserves_masks_history_and_projects_and_rejects_locks() {
    let mut e = fixture(520, 16, "display_p3", [20000, 30000, 40000, 32768]);
    let layer = e.snapshot().active_layer.unwrap();
    run(
        &mut e,
        "layer.mask.add",
        json!({"layer":{"id":layer},"source":"white"}),
    );
    run(
        &mut e,
        "layer.set_position",
        json!({"layer":{"id":layer},"x":3,"y":0}),
    );
    let before = e.document().unwrap().to_archive().unwrap();
    let history = e.snapshot().history.len();
    let mut p = params(&e);
    p["stroke"]["points"] = json!([{"x":259.5,"y":8.5}]);
    run(&mut e, "retouch.burn", p.clone());
    assert!(pixel(&e, 255, 8)[0] < 20000 && pixel(&e, 256, 8)[0] < 20000);
    assert_eq!(pixel(&e, 256, 8)[3], 32768);
    assert_eq!(e.snapshot().history.len(), history + 1);
    let after = e.document().unwrap().to_archive().unwrap();
    let masks = |a: &strata_io::project::ProjectArchive| {
        a.tiles
            .iter()
            .filter(|t| t.layer.starts_with("mask:"))
            .cloned()
            .collect::<Vec<_>>()
    };
    assert!(masks(&after) == masks(&before));
    assert!(
        Document::from_archive(after)
            .unwrap()
            .export_pixels()
            .unwrap()
            == e.document().unwrap().export_pixels().unwrap()
    );
    run(&mut e, "edit.undo", json!({}));
    assert!(e.document().unwrap().to_archive().unwrap() == before);
    run(
        &mut e,
        "layer.group",
        json!({"layers":[{"id":layer}],"name":"locked parent"}),
    );
    run(
        &mut e,
        "layer.set_locks",
        json!({"layer":{"name":"locked parent"},"locks":{"pixels":true}}),
    );
    let revision = e.revision();
    assert!(
        e.execute(e.request(Source::User), "retouch.burn", p, |_| Ok(()))
            .is_err()
    );
    assert_eq!(e.revision(), revision);
}
#[test]
fn invalid_settings_low_budget_and_cancellation_never_publish_partial_tonal_strokes() {
    let mut e = fixture(32, 16, "srgb", [20000, 30000, 40000, 65535]);
    for change in [
        json!({"exposure":-0.1}),
        json!({"range":"invalid"}),
        json!({"unknown":true}),
    ] {
        let mut p = params(&e);
        p.as_object_mut()
            .unwrap()
            .extend(change.as_object().unwrap().clone());
        let revision = e.revision();
        assert!(
            e.execute(e.request(Source::User), "retouch.dodge", p, |_| Ok(()))
                .is_err()
        );
        assert_eq!(e.revision(), revision);
    }
    use strata_pixels::memory::MemoryLimits;
    let mut limited = Editor::with_memory_pool(MemoryPool::new(MemoryLimits {
        cpu_bytes: 3 * 1024 * 1024,
        gpu_bytes: 0,
        disk_bytes: 16 * 1024 * 1024,
    }));
    run(
        &mut limited,
        "file.new",
        json!({"width":32,"height":16,"bit_depth":16,"background":[20000,30000,40000,65535]}),
    );
    let revision = limited.revision();
    let p = params(&limited);
    assert!(
        limited
            .execute(
                limited.request(Source::User),
                "retouch.dodge",
                p,
                |_| Ok(())
            )
            .is_err()
    );
    assert_eq!(limited.revision(), revision);
    let pool = MemoryPool::new(MemoryLimits {
        cpu_bytes: 128 * 1024 * 1024,
        gpu_bytes: 0,
        disk_bytes: 128 * 1024 * 1024,
    });
    let mut e = Editor::with_memory_pool(pool.clone());
    run(
        &mut e,
        "file.new",
        json!({"width":512,"height":512,"bit_depth":16,"background":[20000,30000,40000,65535]}),
    );
    let revision = e.revision();
    let snapshot = e.clone();
    let request = e.request(Source::User);
    let worker = std::thread::spawn(move || {
        snapshot.prepare(request,"retouch.dodge",json!({"stroke":{"layer":{"name":"Background"},"points":[{"x":256.5,"y":256.5}],"size":600,"hardness":1}}))
    });
    let start = std::time::Instant::now();
    while pool.stats().transient_bytes < 6_500_000
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
