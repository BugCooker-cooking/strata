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

// Independent dense two-dimensional encoded-RGB oracle. It never uses the
// filter's separable buffers or ICC round trip to derive its low frequency.
fn high_pass_reference(e: &Editor, w: usize, h: usize, sigma: f64) -> Vec<[f32; 4]> {
    let input = (0..h)
        .flat_map(|y| {
            (0..w).map(move |x| raw(e, x as i64, y as i64).map(|v| f64::from(v) / 65535.0))
        })
        .collect::<Vec<_>>();
    let radius = (3.0 * sigma).ceil() as i32;
    let mut output = Vec::new();
    for y in 0..h {
        for x in 0..w {
            let center = input[y * w + x];
            if center[3] == 0.0 {
                output.push(center.map(|v| v as f32));
                continue;
            }
            let mut sum = [0.0; 4];
            for dy in -radius..=radius {
                for dx in -radius..=radius {
                    let sx = (x as i32 + dx).clamp(0, w as i32 - 1) as usize;
                    let sy = (y as i32 + dy).clamp(0, h as i32 - 1) as usize;
                    let sample = input[sy * w + sx];
                    let k = (-(f64::from(dx * dx + dy * dy)) / (2.0 * sigma * sigma)).exp();
                    for c in 0..3 {
                        sum[c] += k * sample[c] * sample[3];
                    }
                    sum[3] += k * sample[3];
                }
            }
            let rgb = std::array::from_fn::<_, 3, _>(|c| {
                (0.5 + center[c] - sum[c] / sum[3]).clamp(0.0, 1.0) as f32
            });
            output.push([rgb[0], rgb[1], rgb[2], center[3] as f32]);
        }
    }
    output
}

#[test]
fn high_pass_matches_independent_encoded_alpha_weighted_reference_in_both_depths_and_iccs() {
    for depth in [8, 16] {
        for space in ["srgb", "display_p3"] {
            let mut e = fixture(19, 7, depth, space, |x, y| {
                if x == 6 {
                    [65535, 0, 54321, 0]
                } else {
                    [
                        1000 + (x * 2711 + y * 571) as u16,
                        62000 - (x * 2477 + y * 277) as u16,
                        5000 + ((x % 3) * 15001 + y * 1793) as u16,
                        if x == 7 { 257 } else { 32769 },
                    ]
                }
            });
            let expected = high_pass_reference(&e, 19, 7, 0.8);
            let before = e.clone();
            let params = p(&e);
            assert_eq!(run(&mut e, "filter.high_pass", params)["touched_tiles"], 1);
            assert_eq!(
                e.snapshot().history.len(),
                before.snapshot().history.len() + 1
            );
            for y in 0..7 {
                for x in 0..19 {
                    let actual = raw(&e, x, y);
                    let original = raw(&before, x, y);
                    assert_eq!(actual[3], original[3]);
                    if original[3] == 0 {
                        assert_eq!(actual, original);
                        continue;
                    }
                    let target = expected[y as usize * 19 + x as usize];
                    for c in 0..3 {
                        let ideal = (f64::from(target[c]) * 65535.0).round() as u16;
                        let tolerance = if depth == 8 { 257 } else { 3 };
                        assert!(
                            actual[c].abs_diff(ideal) <= tolerance,
                            "{depth} {space} {x},{y},{c}: {} vs {ideal}",
                            actual[c]
                        );
                    }
                }
            }
            run(&mut e, "edit.undo", json!({}));
            assert!(
                e.document().unwrap().to_archive().unwrap()
                    == before.document().unwrap().to_archive().unwrap()
            );
        }
    }
}

#[test]
fn high_pass_flat_fields_become_working_gray_and_repeated_gray_is_a_true_noop() {
    for depth in [8, 16] {
        for space in ["srgb", "display_p3"] {
            let mut e = fixture(9, 5, depth, space, |_, _| [10001, 30003, 50001, 32769]);
            let params = p(&e);
            run(&mut e, "filter.high_pass", params.clone());
            let first = raw(&e, 4, 2);
            for channel in first.into_iter().take(3) {
                assert!(
                    channel.abs_diff(if depth == 8 { 32896 } else { 32768 })
                        <= if depth == 8 { 0 } else { 1 },
                    "{depth} {space}: {first:?}"
                );
            }
            let before = e.document().unwrap().to_archive().unwrap();
            let cursor = e.snapshot().history_cursor;
            let revision = e.revision();
            let dirty = e.dirty();
            let candidate = e
                .prepare(e.request(Source::User), "filter.high_pass", params)
                .unwrap();
            run(
                &mut e,
                "layer.select",
                json!({"layer":{"name":"Background"}}),
            );
            let selected = e.snapshot().active_layer;
            let selected_before = e.document().unwrap().to_archive().unwrap();
            assert_eq!(e.commit(candidate).unwrap()["touched_tiles"], 0);
            assert_eq!(e.snapshot().active_layer, selected);
            assert_eq!(e.revision(), revision);
            assert_eq!(e.snapshot().history_cursor, cursor);
            assert_eq!(e.dirty(), dirty);
            assert!(e.document().unwrap().to_archive().unwrap() == selected_before);
            assert!(e.document().unwrap().to_archive().unwrap().tiles == before.tiles);
        }
    }
}

#[test]
fn high_pass_soft_selection_blends_once_without_clipping_the_input_halo() {
    for depth in [8, 16] {
        for space in ["srgb", "display_p3"] {
            let mut e = fixture(32, 12, depth, space, |x, _| {
                if x < 16 {
                    [12000, 23000, 34000, 16448]
                } else {
                    [54000, 43000, 32000, 49344]
                }
            });
            let transformed = high_pass_reference(&e, 32, 12, 0.8);
            let before = e.clone();
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
            assert!(weight > 0.0 && weight < 1.0);
            let old = linear(&e, raw(&e, 16, 6));
            let new = strata_color::working_to_linear_p3(
                &[transformed[6 * 32 + 16]],
                e.document().unwrap().working_icc(),
            )
            .unwrap()[0];
            let params = p(&e);
            run(&mut e, "filter.high_pass", params);
            let actual = linear(&e, raw(&e, 16, 6));
            assert_eq!(raw(&e, 8, 6), raw(&before, 8, 6));
            assert_eq!(raw(&e, 16, 6)[3], raw(&before, 16, 6)[3]);
            for c in 0..3 {
                let expected = f64::from(old[c]) * (1.0 - weight) + f64::from(new[c]) * weight;
                assert!(
                    (f64::from(actual[c]) - expected).abs()
                        < if depth == 8 { 0.005 } else { 0.0001 }
                );
            }
        }
    }
}

#[test]
fn high_pass_negative_cross_tile_crop_preserves_offboard_source_masks_styles_and_project() {
    let mut e = fixture(520, 4, 16, "display_p3", |x, y| {
        [
            1000 + x as u16 * 100,
            2000 + y as u16 * 10000,
            60000 - x as u16 * 80,
            32769,
        ]
    });
    let id = e.snapshot().active_layer;
    run(
        &mut e,
        "select.marquee",
        json!({"x":250,"y":0,"width":40,"height":4}),
    );
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
    run(
        &mut e,
        "layer.set_styles",
        json!({"layer":{"id":id},"styles":{"color_overlay":{"color":[30003,20003,10001,65535],"opacity":0.25}}}),
    );
    run(&mut e, "select.none", json!({}));
    run(
        &mut e,
        "layer.set_position",
        json!({"layer":{"id":id},"x":-260,"y":3}),
    );
    run(
        &mut e,
        "image.crop",
        json!({"rect":{"x":-5,"y":3,"width":20,"height":4},"pixels":"preserve"}),
    );
    run(
        &mut e,
        "layer.set_locks",
        json!({"layer":{"id":id},"locks":{"transparency":true,"position":true}}),
    );
    let before = e.clone();
    let archive = e.document().unwrap().to_archive().unwrap();
    let oracle = fixture(20, 4, 16, "display_p3", |x, y| {
        raw(&e, i64::from(x) + 255, i64::from(y))
    });
    let expected = high_pass_reference(&oracle, 20, 4, 0.8);
    let params = p(&e);
    assert_eq!(run(&mut e, "filter.high_pass", params)["touched_tiles"], 2);
    assert_eq!(e.snapshot().layers, before.snapshot().layers);
    for y in 0..4 {
        for x in 0..520 {
            if !(255..275).contains(&x) {
                assert_eq!(raw(&e, x, y), raw(&before, x, y));
            } else {
                for (c, target) in expected[y as usize * 20 + (x - 255) as usize]
                    .into_iter()
                    .take(3)
                    .enumerate()
                {
                    let ideal = (f64::from(target) * 65535.0).round() as u16;
                    assert!(
                        raw(&e, x, y)[c].abs_diff(ideal) <= 3,
                        "cross-tile {x},{y},{c}"
                    );
                }
            }
            assert_eq!(raw(&e, x, y)[3], 32769);
        }
    }
    let a = before
        .document()
        .unwrap()
        .layers()
        .last()
        .unwrap()
        .mask()
        .unwrap();
    let b = e
        .document()
        .unwrap()
        .layers()
        .last()
        .unwrap()
        .mask()
        .unwrap();
    assert_eq!(a.layout(), b.layout());
    for (coord, _) in a.tiles() {
        assert_eq!(a.tile_version(*coord), b.tile_version(*coord));
    }
    let saved = e.document().unwrap().to_archive().unwrap();
    assert!(
        Document::from_archive(saved.clone())
            .unwrap()
            .to_archive()
            .unwrap()
            == saved
    );
    run(&mut e, "edit.undo", json!({}));
    assert!(e.document().unwrap().to_archive().unwrap() == archive);
}

#[test]
fn high_pass_transparent_or_empty_selection_keeps_history_dirty_state_and_sources() {
    for transparent in [false, true] {
        let mut e = fixture(9, 5, 16, "srgb", |_, _| {
            [10001, 30003, 50001, if transparent { 0 } else { 32769 }]
        });
        if !transparent {
            run(
                &mut e,
                "select.marquee",
                json!({"x":0,"y":0,"width":9,"height":5}),
            );
            run(
                &mut e,
                "select.marquee",
                json!({"x":0,"y":0,"width":9,"height":5,"mode":"subtract"}),
            );
        }
        let before = e.document().unwrap().to_archive().unwrap();
        let cursor = e.snapshot().history_cursor;
        let revision = e.revision();
        let dirty = e.dirty();
        let params = p(&e);
        assert_eq!(run(&mut e, "filter.high_pass", params)["touched_tiles"], 0);
        assert_eq!(e.revision(), revision);
        assert_eq!(e.snapshot().history_cursor, cursor);
        assert_eq!(e.dirty(), dirty);
        assert!(e.document().unwrap().to_archive().unwrap() == before);
    }
}

#[test]
fn high_pass_rejects_invalid_locked_nonpixel_or_over_budget_work_without_changes() {
    use strata_pixels::memory::MemoryLimits;
    let mut e = fixture(16, 12, 16, "srgb", |_, _| [20000, 30000, 40000, 65535]);
    for extra in [
        json!({"sigma":0}),
        json!({"sigma":65}),
        json!({"sigma":"2"}),
        json!({"radius":2}),
        json!({"space":"display"}),
    ] {
        let mut params = p(&e);
        params
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        let before = e.snapshot();
        assert!(
            e.execute(e.request(Source::User), "filter.high_pass", params, |_| Ok(
                ()
            ))
            .is_err()
        );
        assert_eq!(e.revision(), before.revision);
        assert_eq!(e.snapshot().history_cursor, before.history_cursor);
    }
    let layer = e.snapshot().active_layer;
    let parent = run(
        &mut e,
        "layer.group",
        json!({"layers":[{"id":layer}],"name":"parent"}),
    )["layer_id"]
        .clone();
    for locks in [json!({"pixels":true}), json!({"all":true})] {
        run(
            &mut e,
            "layer.set_locks",
            json!({"layer":{"id":parent},"locks":locks}),
        );
        let before = e.document().unwrap().to_archive().unwrap();
        assert!(
            e.execute(
                e.request(Source::User),
                "filter.high_pass",
                json!({"layer":{"id":layer}}),
                |_| Ok(())
            )
            .is_err()
        );
        assert!(e.document().unwrap().to_archive().unwrap() == before);
        run(
            &mut e,
            "layer.set_locks",
            json!({"layer":{"id":parent},"locks":{}}),
        );
    }
    assert!(
        e.execute(
            e.request(Source::User),
            "filter.high_pass",
            json!({"layer":{"id":parent}}),
            |_| Ok(())
        )
        .is_err()
    );
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
    let before = limited.document().unwrap().to_archive().unwrap();
    let params = p(&limited);
    assert!(
        limited
            .execute(
                limited.request(Source::User),
                "filter.high_pass",
                params,
                |_| Ok(())
            )
            .is_err()
    );
    assert!(limited.document().unwrap().to_archive().unwrap() == before);
    let mut huge = Editor::new();
    run(
        &mut huge,
        "file.new",
        json!({"width":8001,"height":8000,"bit_depth":16}),
    );
    let revision = huge.revision();
    let params = p(&huge);
    assert!(matches!(
        huge.execute(
            huge.request(Source::User),
            "filter.high_pass",
            params,
            |_| Ok(())
        ),
        Err(strata_core::commands::CommandError::Document(
            strata_core::DocumentError::ResourceLimit
        ))
    ));
    assert_eq!(huge.revision(), revision);
}

#[test]
fn high_pass_cancels_during_convolution_and_display_failure_never_commits() {
    use strata_pixels::memory::MemoryLimits;
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
    let before = e.document().unwrap().to_archive().unwrap();
    let revision = e.revision();
    let request = e.request(Source::User);
    let source = e.clone();
    let worker = std::thread::spawn(move || {
        source.prepare(
            request,
            "filter.high_pass",
            json!({"layer":{"name":"Background"},"sigma":32}),
        )
    });
    let start = std::time::Instant::now();
    while pool.stats().transient_bytes < 12 * 1024 * 1024
        && start.elapsed() < std::time::Duration::from_secs(2)
    {
        std::thread::yield_now();
    }
    assert!(
        pool.stats().transient_bytes >= 12 * 1024 * 1024,
        "convolution scratch was not observed"
    );
    e.cancel_pending_work();
    assert!(matches!(
        worker.join().unwrap(),
        Err(strata_core::commands::CommandError::Expired)
    ));
    assert_eq!(pool.stats().transient_bytes, 0);
    assert_eq!(e.revision(), revision);
    assert!(e.document().unwrap().to_archive().unwrap() == before);
    let params = p(&e);
    assert!(
        e.execute(
            e.request(Source::User),
            "filter.high_pass",
            params,
            |_| Err("display allocation failed".into())
        )
        .is_err()
    );
    assert_eq!(e.revision(), revision);
    assert!(e.document().unwrap().to_archive().unwrap() == before);
}

#[test]
fn linear_high_pass_matches_independent_linear_gaussian_residual_at_both_depths_and_profiles() {
    for depth in [8, 16] {
        for space in ["srgb", "display_p3"] {
            let mut e = fixture(13, 7, depth, space, |x, y| {
                if x == 4 {
                    [65535, 0, 50001, 0]
                } else {
                    [
                        10000 + x as u16 * 2300,
                        20000 + y as u16 * 1700,
                        40000 - x as u16 * 900,
                        32769,
                    ]
                }
            });
            let before = e.clone();
            let low = reference(&e, 13, 7, 0.8);
            let mut expected = Vec::new();
            for y in 0..7 {
                for x in 0..13 {
                    let old = linear(&e, raw(&e, x, y));
                    let blurred = low[y as usize * 13 + x as usize];
                    expected.push([
                        (0.5 + old[0] - blurred[0]).clamp(0.0, 1.0),
                        (0.5 + old[1] - blurred[1]).clamp(0.0, 1.0),
                        (0.5 + old[2] - blurred[2]).clamp(0.0, 1.0),
                        old[3],
                    ]);
                }
            }
            let expected = strata_color::from_linear_p3(
                &expected,
                e.document().unwrap().format(),
                e.document().unwrap().working_icc(),
            )
            .unwrap();
            let mut params = p(&e);
            params["space"] = json!("linear_p3");
            assert!(
                run(&mut e, "filter.high_pass", params)["touched_tiles"]
                    .as_u64()
                    .unwrap()
                    > 0
            );
            for y in 0..7 {
                for x in 0..13 {
                    let original = raw(&before, x, y);
                    let actual = raw(&e, x, y);
                    assert_eq!(actual[3], original[3]);
                    if original[3] == 0 {
                        assert_eq!(actual, original);
                        continue;
                    }
                    let i = y as usize * 13 + x as usize;
                    for c in 0..3 {
                        let code = match &expected {
                            PixelBuffer::Rgba8(v) => u16::from(v[i * 4 + c]) * 257,
                            PixelBuffer::Rgba16(v) => v[i * 4 + c],
                        };
                        assert!(
                            actual[c].abs_diff(code) <= if depth == 8 { 257 } else { 3 },
                            "linear high pass {depth} {space} {x},{y},{c}"
                        );
                    }
                }
            }
            run(&mut e, "edit.undo", json!({}));
            assert!(
                e.document().unwrap().to_archive().unwrap()
                    == before.document().unwrap().to_archive().unwrap()
            );
        }
    }
}

#[test]
fn linear_high_pass_flat_fields_are_neutral_in_actual_blend_modes_and_repeat_without_history() {
    for depth in [8, 16] {
        for space in ["srgb", "display_p3"] {
            let mut e = fixture(9, 5, depth, space, |_, _| [10001, 30003, 50001, 32769]);
            let mut params = p(&e);
            params["space"] = json!("linear_p3");
            run(&mut e, "filter.high_pass", params.clone());
            let front = linear(&e, raw(&e, 4, 2)).map(f64::from);
            for mode in [
                strata_core::BlendMode::Overlay,
                strata_core::BlendMode::SoftLight,
                strata_core::BlendMode::LinearLight,
            ] {
                let back = [0.15, 0.5, 0.85, 1.0];
                let shown = strata_core::composite::blend(back, front, mode, 1.0);
                for c in 0..3 {
                    assert!(
                        (shown[c] - back[c]).abs() < if depth == 8 { 0.008 } else { 0.0001 },
                        "flat-field blend {depth} {space} {mode:?}: {shown:?}"
                    );
                }
                assert_eq!(shown[3], 1.0);
            }
            let before = e.document().unwrap().to_archive().unwrap();
            let cursor = e.snapshot().history_cursor;
            let revision = e.revision();
            assert_eq!(run(&mut e, "filter.high_pass", params)["touched_tiles"], 0);
            assert_eq!(e.revision(), revision);
            assert_eq!(e.snapshot().history_cursor, cursor);
            assert!(e.document().unwrap().to_archive().unwrap() == before);
        }
    }
}
