use serde_json::{Value, json};
use strata_core::{ClipboardImage, Document, Editor, Source};
use strata_pixels::{PixelBuffer, PixelFormat, PixelValue, memory::MemoryPool};
fn run(e: &mut Editor, id: &str, p: Value) -> Value {
    e.execute(e.request(Source::User), id, p, |_| Ok(()))
        .unwrap()
}
fn fixture(
    w: u32,
    h: u32,
    depth: u8,
    space: &str,
    mut sample: impl FnMut(u32, u32) -> [u16; 4],
) -> Editor {
    let mut e = Editor::new();
    run(
        &mut e,
        "file.new",
        json!({"width":w,"height":h,"bit_depth":depth,"working_space":space}),
    );
    let mut values = Vec::new();
    for y in 0..h {
        for x in 0..w {
            values.extend(sample(x, y));
        }
    }
    let buffer = if depth == 16 {
        PixelBuffer::Rgba16(values)
    } else {
        PixelBuffer::Rgba8(values.into_iter().map(|v| (v / 257) as u8).collect())
    };
    let image = ClipboardImage::new(
        buffer,
        w,
        h,
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
fn raw(e: &Editor, x: i64, y: i64) -> [u16; 4] {
    let layer = e
        .document()
        .unwrap()
        .layers()
        .iter()
        .find(|l| Some(l.id()) == e.snapshot().active_layer)
        .unwrap();
    match layer.pixels().pixel(x, y).unwrap() {
        PixelValue::Rgba16(v) => v,
        PixelValue::Rgba8(v) => v.map(|v| u16::from(v) * 257),
    }
}
fn p(e: &Editor) -> Value {
    json!({"layer":{"id":e.snapshot().active_layer},"sigma":0.8})
}
fn linear(e: &Editor, p: [u16; 4]) -> [f32; 4] {
    strata_color::working_to_linear_p3(
        &[p.map(|v| f32::from(v) / 65535.0)],
        e.document().unwrap().working_icc(),
    )
    .unwrap()[0]
}
fn reference(e: &Editor, w: usize, h: usize, sigma: f64) -> Vec<[f32; 4]> {
    let source = (0..h)
        .flat_map(|y| (0..w).map(move |x| linear(e, raw(e, x as i64, y as i64))))
        .collect::<Vec<_>>();
    let r = (3.0 * sigma).ceil() as i32;
    let mut weights = (-r..=r)
        .map(|v| (-f64::from(v * v) / (2.0 * sigma * sigma)).exp())
        .collect::<Vec<_>>();
    let total = weights.iter().sum::<f64>();
    for k in &mut weights {
        *k /= total;
    }
    let mut out = Vec::new();
    for y in 0..h {
        for x in 0..w {
            let mut sum = [0.0; 4];
            for dy in -r..=r {
                for dx in -r..=r {
                    let sx = (x as i32 + dx).clamp(0, w as i32 - 1) as usize;
                    let sy = (y as i32 + dy).clamp(0, h as i32 - 1) as usize;
                    let p = source[sy * w + sx].map(f64::from);
                    let weight = weights[(dy + r) as usize] * weights[(dx + r) as usize];
                    for c in 0..3 {
                        sum[c] += p[c] * p[3] * weight;
                    }
                    sum[3] += p[3] * weight;
                }
            }
            out.push(if sum[3] > 1e-12 {
                [
                    (sum[0] / sum[3]) as f32,
                    (sum[1] / sum[3]) as f32,
                    (sum[2] / sum[3]) as f32,
                    sum[3] as f32,
                ]
            } else {
                [0.0; 4]
            });
        }
    }
    out
}
#[test]
fn gaussian_matches_independent_direct_2d_premultiplied_reference_and_ignores_hidden_rgb() {
    for space in ["srgb", "display_p3"] {
        let mut e = fixture(7, 5, 16, space, |x, y| {
            let a = [0, 16384, 32768, 65535][((x + y) % 4) as usize];
            [1000 + x as u16 * 7000, 2000 + y as u16 * 9000, 50001, a]
        });
        let expected = reference(&e, 7, 5, 0.8);
        let expected = strata_color::from_linear_p3(
            &expected,
            PixelFormat::Rgba16,
            e.document().unwrap().working_icc(),
        )
        .unwrap();
        let PixelBuffer::Rgba16(expected) = expected else {
            panic!()
        };
        let params = p(&e);
        run(&mut e, "filter.gaussian_blur", params);
        for y in 0..5 {
            for x in 0..7 {
                let actual = raw(&e, x, y);
                let i = (y as usize * 7 + x as usize) * 4;
                for c in 0..4 {
                    assert!(
                        actual[c].abs_diff(expected[i + c]) <= 4,
                        "{space} {x},{y},{c}: {actual:?} != {:?}",
                        &expected[i..i + 4]
                    );
                }
            }
        }
    }
    let mut e = fixture(9, 9, 16, "srgb", |x, _| {
        if x < 4 {
            [65535, 0, 65535, 0]
        } else {
            [0, 0, 65535, 65535]
        }
    });
    let params = p(&e);
    run(&mut e, "filter.gaussian_blur", params);
    let edge = raw(&e, 3, 4);
    assert!(edge[0] < 8 && edge[1] < 8 && edge[2] > 65520 && edge[3] > 0);
}
#[test]
fn constant_colors_and_zero_amounts_preserve_exact_codes_at_both_depths() {
    for depth in [8, 16] {
        for space in ["srgb", "display_p3"] {
            for sigma in [0.1, 1.5, 8.0, 64.0] {
                let mut e = fixture(8, 5, depth, space, |_, _| [10001, 20002, 30003, 40004]);
                let before = e.document().unwrap().to_archive().unwrap();
                let mut params = p(&e);
                params["sigma"] = json!(sigma);
                run(&mut e, "filter.gaussian_blur", params);
                assert!(
                    e.document().unwrap().to_archive().unwrap().tiles == before.tiles,
                    "constant {depth} {space} {sigma}"
                );
                let mut params = p(&e);
                params["amount"] = json!(0);
                run(&mut e, "filter.unsharp_mask", params);
                let mut params = p(&e);
                params.as_object_mut().unwrap().remove("sigma");
                params["amount"] = json!(0);
                run(&mut e, "filter.add_noise", params);
                assert!(e.document().unwrap().to_archive().unwrap().tiles == before.tiles);
            }
        }
    }
}
#[test]
fn gaussian_halo_uses_unselected_input_and_soft_selection_blends_once() {
    let mut e = fixture(32, 12, 16, "srgb", |x, _| {
        if x < 16 {
            [10000, 20000, 30000, 32768]
        } else {
            [50000, 40000, 30000, 65535]
        }
    });
    let mut full = e.clone();
    let params = p(&full);
    run(&mut full, "filter.gaussian_blur", params);
    let original = raw(&e, 16, 6);
    let untouched = raw(&e, 8, 6);
    run(
        &mut e,
        "select.marquee",
        json!({"x":16,"y":0,"width":16,"height":12}),
    );
    run(&mut e, "select.feather", json!({"radius":2}));
    let weight = f64::from(
        e.document()
            .unwrap()
            .selection()
            .unwrap()
            .sample(16, 6)
            .unwrap(),
    ) / 65535.0;
    let a = linear(&e, original).map(f64::from);
    let b = linear(&full, raw(&full, 16, 6)).map(f64::from);
    let alpha = a[3] * (1.0 - weight) + b[3] * weight;
    let params = p(&e);
    run(&mut e, "filter.gaussian_blur", params);
    let actual = linear(&e, raw(&e, 16, 6)).map(f64::from);
    assert!((actual[3] - alpha).abs() < 0.0001);
    for c in 0..3 {
        let expected = (a[c] * a[3] * (1.0 - weight) + b[c] * b[3] * weight) / alpha;
        assert!((actual[c] - expected).abs() < 0.0001);
    }
    assert_eq!(raw(&e, 8, 6), untouched);
    assert_ne!(raw(&e, 16, 6), original);
}
#[test]
fn gaussian_crosses_tiles_is_offset_invariant_and_keeps_masks_history_and_project() {
    let mut e = fixture(520, 9, 16, "display_p3", |x, y| {
        let v = 8000 + (x % 16) as u16 * 2100 + y as u16 * 600;
        [v, v + 1, v + 2, 32768]
    });
    let mut shifted = e.clone();
    let layer = e.snapshot().active_layer;
    run(
        &mut shifted,
        "layer.set_position",
        json!({"layer":{"id":layer},"x":3,"y":0}),
    );
    run(
        &mut e,
        "layer.mask.add",
        json!({"layer":{"id":layer},"source":"white"}),
    );
    let archive = e.document().unwrap().to_archive().unwrap();
    let history = e.snapshot().history.len();
    let params = p(&e);
    run(&mut e, "filter.gaussian_blur", params);
    let params = p(&shifted);
    run(&mut shifted, "filter.gaussian_blur", params);
    for x in [128, 254, 255, 256, 257, 380] {
        for (a, b) in raw(&e, x, 4).into_iter().zip(raw(&shifted, x, 4)) {
            assert!(a.abs_diff(b) <= 2);
        }
    }
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
    assert_eq!(e.snapshot().history.len(), history + 1);
    let reopened = Document::from_archive(e.document().unwrap().to_archive().unwrap()).unwrap();
    assert!(reopened.export_pixels().unwrap() == e.document().unwrap().export_pixels().unwrap());
    run(&mut e, "edit.undo", json!({}));
    assert!(e.document().unwrap().to_archive().unwrap() == archive);
}
#[test]
fn usm_matches_step_reference_threshold_and_preserves_alpha() {
    let original = fixture(17, 5, 16, "srgb", |x, _| {
        if x < 8 {
            [15000, 20000, 25000, 32768]
        } else {
            [45000, 40000, 35000, 32768]
        }
    });
    let blurred = reference(&original, 17, 5, 0.8);
    let mut e = original.clone();
    let params = p(&e);
    run(&mut e, "filter.unsharp_mask", params);
    for x in [6usize, 7, 8, 9] {
        let old = linear(&original, raw(&original, x as i64, 2));
        let b = blurred[2 * 17 + x];
        let expected = [
            (2.0 * old[0] - b[0]).clamp(0.0, 1.0),
            (2.0 * old[1] - b[1]).clamp(0.0, 1.0),
            (2.0 * old[2] - b[2]).clamp(0.0, 1.0),
            old[3],
        ];
        // The independent convolution reference also includes the documented
        // working-ICC gamut clipping and final integer publication.
        let PixelBuffer::Rgba16(expected) = strata_color::from_linear_p3(
            &[expected],
            PixelFormat::Rgba16,
            e.document().unwrap().working_icc(),
        )
        .unwrap() else {
            panic!()
        };
        let actual = raw(&e, x as i64, 2);
        for c in 0..3 {
            assert!(
                actual[c].abs_diff(expected[c]) <= 4,
                "USM {x},{c}: {actual:?} != {expected:?}"
            );
        }
        assert_eq!(raw(&e, x as i64, 2)[3], 32768);
    }
    assert!(raw(&e, 7, 2)[0] < raw(&original, 7, 2)[0]);
    assert!(raw(&e, 8, 2)[0] > raw(&original, 8, 2)[0]);
    let mut e = original.clone();
    let mut params = p(&e);
    params["threshold"] = json!(1);
    run(&mut e, "filter.unsharp_mask", params);
    assert!(
        e.document().unwrap().to_archive().unwrap().tiles
            == original.document().unwrap().to_archive().unwrap().tiles
    );
}
#[test]
fn seeded_noise_is_deterministic_in_working_rgb_with_bounded_statistics_and_exact_alpha() {
    for depth in [8, 16] {
        for distribution in ["uniform", "gaussian"] {
            let original = fixture(96, 64, depth, "srgb", |_, _| [32768, 32768, 32768, 32768]);
            let mut e = original.clone();
            let params = json!({"layer":{"id":e.snapshot().active_layer},"amount":0.05,"distribution":distribution,"seed":42,"monochrome":true});
            let mut replay = original.clone();
            run(&mut e, "filter.add_noise", params.clone());
            run(&mut replay, "filter.add_noise", params.clone());
            assert!(
                e.document().unwrap().to_archive().unwrap().tiles
                    == replay.document().unwrap().to_archive().unwrap().tiles
            );
            let mut sum = 0.0;
            let mut square = 0.0;
            for y in 0..64 {
                for x in 0..96 {
                    let value = raw(&e, x, y);
                    assert_eq!(value[3], raw(&original, x, y)[3]);
                    assert!(value[0].abs_diff(value[1]) <= 2 && value[1].abs_diff(value[2]) <= 2);
                    let d = (f64::from(value[0]) - f64::from(raw(&original, x, y)[0])) / 65535.0;
                    sum += d;
                    square += d * d;
                    if distribution == "uniform" {
                        assert!(d.abs() < 0.052);
                    }
                }
            }
            let mean = sum / 6144.0;
            let variance = square / 6144.0 - mean * mean;
            assert!(mean.abs() < 0.003);
            let expected = if distribution == "uniform" {
                0.05 * 0.05 / 3.0
            } else {
                0.05 * 0.05
            };
            assert!(
                (variance - expected).abs() < expected * 0.12,
                "{distribution} {depth} {variance} != {expected}"
            );
            let mut next = original.clone();
            let mut params = params;
            params["seed"] = json!(43);
            run(&mut next, "filter.add_noise", params);
            assert!(
                next.document().unwrap().to_archive().unwrap().tiles
                    != e.document().unwrap().to_archive().unwrap().tiles
            );
        }
    }
    let mut e = fixture(8, 8, 16, "display_p3", |_, _| [65535, 0, 50001, 0]);
    let before = e.document().unwrap().to_archive().unwrap();
    let params = json!({"layer":{"id":e.snapshot().active_layer},"amount":0.2});
    run(&mut e, "filter.add_noise", params);
    assert!(e.document().unwrap().to_archive().unwrap().tiles == before.tiles);
}
#[test]
fn invalid_settings_locks_low_budget_and_cancellation_are_atomic() {
    let mut e = fixture(16, 12, 16, "srgb", |_, _| [20000, 30000, 40000, 65535]);
    for (command, extra) in [
        ("filter.gaussian_blur", json!({"sigma":0})),
        ("filter.gaussian_blur", json!({"sigma":65})),
        ("filter.unsharp_mask", json!({"amount":6})),
        ("filter.unsharp_mask", json!({"threshold":-1})),
        ("filter.add_noise", json!({"seed":4294967296u64})),
        ("filter.add_noise", json!({"amount":2})),
    ] {
        let mut params = json!({"layer":{"id":e.snapshot().active_layer}});
        params
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        let revision = e.revision();
        assert!(
            e.execute(e.request(Source::User), command, params, |_| Ok(()))
                .is_err()
        );
        assert_eq!(e.revision(), revision);
    }
    let layer = e.snapshot().active_layer;
    run(
        &mut e,
        "layer.set_locks",
        json!({"layer":{"id":layer},"locks":{"pixels":true}}),
    );
    let revision = e.revision();
    let params = p(&e);
    assert!(
        e.execute(
            e.request(Source::User),
            "filter.gaussian_blur",
            params,
            |_| Ok(())
        )
        .is_err()
    );
    assert_eq!(e.revision(), revision);
    use strata_pixels::memory::MemoryLimits;
    let mut limited = Editor::with_memory_pool(MemoryPool::new(MemoryLimits {
        cpu_bytes: 3 * 1024 * 1024,
        gpu_bytes: 0,
        disk_bytes: 16 * 1024 * 1024,
    }));
    run(
        &mut limited,
        "file.new",
        json!({"width":16,"height":12,"bit_depth":16,"background":[20000,30000,40000,65535]}),
    );
    let revision = limited.revision();
    let params = p(&limited);
    assert!(
        limited
            .execute(
                limited.request(Source::User),
                "filter.gaussian_blur",
                params,
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
    let request = e.request(Source::User);
    let source = e.clone();
    let worker = std::thread::spawn(move || {
        source.prepare(
            request,
            "filter.gaussian_blur",
            json!({"layer":{"name":"Background"},"sigma":8}),
        )
    });
    let start = std::time::Instant::now();
    while pool.stats().transient_bytes < 8 * 1024 * 1024
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

#[test]
fn preview_target_is_anchored_and_local_cancellation_does_not_revoke_other_work() {
    use strata_core::commands::WorkCancellation;
    let mut e = fixture(16, 12, 16, "srgb", |x, _| {
        if x < 8 {
            [10000, 20000, 30000, 65535]
        } else {
            [50000, 40000, 30000, 65535]
        }
    });
    let layer: strata_core::LayerRef =
        serde_json::from_value(json!({"id":e.snapshot().active_layer})).unwrap();
    let interaction = e.begin_filter(layer, "filter.gaussian_blur").unwrap();
    let work = WorkCancellation::default();
    let scoped = interaction.with_work_cancellation(&work);
    let unrelated = e.request(Source::User);
    let before = e.document().unwrap().to_archive().unwrap();
    let candidate = e
        .preview_filter(&scoped, "filter.gaussian_blur", json!({"sigma":0.8}))
        .unwrap();
    assert!(
        e.preview_filter(
            &scoped,
            "filter.gaussian_blur",
            json!({"layer":{"name":"Background"},"sigma":0.8})
        )
        .is_err()
    );
    work.cancel();
    assert!(e.commit(candidate).is_err());
    assert!(e.document().unwrap().to_archive().unwrap() == before);
    e.execute(unrelated, "document.inspect", json!({}), |_| Ok(()))
        .unwrap();
    let next = interaction.with_work_cancellation(&work);
    let candidate = e
        .preview_filter(&next, "filter.gaussian_blur", json!({"sigma":0.8}))
        .unwrap();
    e.commit(candidate).unwrap();
    assert_eq!(
        e.snapshot().history.last().unwrap().command,
        "filter.gaussian_blur"
    );
}

#[test]
fn negative_artboard_coordinates_and_transparency_lock_keep_sampling_and_hidden_codes() {
    let mut e = fixture(17, 9, 16, "display_p3", |x, y| {
        let v = 1000 + x as u16 * 2100 + y as u16 * 200;
        [v, v + 1, v + 2, 32768]
    });
    let mut archive = e.document().unwrap().to_archive().unwrap();
    archive.metadata["frame"]["x"] = json!(-129);
    archive.metadata["frame"]["y"] = json!(-7);
    for layer in archive.metadata["layers"].as_array_mut().unwrap() {
        layer["x"] = json!(-129);
        layer["y"] = json!(-7);
    }
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("negative.strata");
    strata_io::project::write_project(&path, &archive, false, false).unwrap();
    let mut negative = Editor::new();
    let token = negative.grant_read(&path, Source::User).unwrap();
    run(&mut negative, "file.open", json!({"resource":token}));
    let params = p(&e);
    run(&mut e, "filter.gaussian_blur", params);
    let params = p(&negative);
    run(&mut negative, "filter.gaussian_blur", params);
    for y in 0..9 {
        for x in 0..17 {
            assert_eq!(raw(&e, x, y), raw(&negative, x, y));
        }
    }
    let mut e = fixture(9, 9, 16, "srgb", |x, _| {
        if x < 4 {
            [65535, 0, 50001, 0]
        } else {
            [0, 0, 65535, 32768]
        }
    });
    let layer = e.snapshot().active_layer;
    let hidden = raw(&e, 3, 4);
    run(
        &mut e,
        "layer.set_locks",
        json!({"layer":{"id":layer},"locks":{"transparency":true}}),
    );
    let params = p(&e);
    run(&mut e, "filter.gaussian_blur", params);
    assert_eq!(raw(&e, 3, 4), hidden);
    assert_eq!(raw(&e, 4, 4)[3], 32768);
}
