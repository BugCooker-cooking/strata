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
    let mut v = Vec::new();
    for y in 0..h {
        for x in 0..w {
            v.extend(sample(x, y));
        }
    }
    let p = if depth == 8 {
        PixelBuffer::Rgba8(v.into_iter().map(|v| v as u8).collect())
    } else {
        PixelBuffer::Rgba16(v)
    };
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
    let resource = e.grant_clipboard_image(image, Source::User).unwrap();
    run(&mut e, "edit.paste_in_place", json!({"resource":resource}));
    let id = e.snapshot().active_layer;
    run(
        &mut e,
        "layer.rename",
        json!({"layer":{"id":id},"name":"pixels"}),
    );
    e
}
fn raw(e: &Editor, x: i64, y: i64) -> [u16; 4] {
    let l = e
        .document()
        .unwrap()
        .layers()
        .iter()
        .find(|l| l.info().name == "pixels")
        .unwrap();
    match l.pixels().pixel(x, y).unwrap() {
        PixelValue::Rgba8(v) => v.map(u16::from),
        PixelValue::Rgba16(v) => v,
    }
}
fn params(method: &str, clip: f64) -> Value {
    json!({"layer":{"name":"pixels"},"settings":{"method":method,"clip_percent":clip}})
}
fn color_params(clip: f64, strength: f64) -> Value {
    json!({"layer":{"name":"pixels"},"settings":{"method":"color","clip_percent":clip,"neutralize_strength":strength}})
}
#[test]
fn contrast_and_tone_use_distinct_independent_stretches_at_both_depths_and_profiles() {
    for depth in [8, 16] {
        for space in ["srgb", "display_p3"] {
            for method in ["contrast", "tone"] {
                let points = if depth == 8 {
                    [
                        [10, 30, 50, 255],
                        [60, 100, 140, 128],
                        [210, 230, 250, 255],
                        [255, 0, 255, 0],
                    ]
                } else {
                    [
                        [1001, 3003, 5001, 65535],
                        [20003, 30003, 40001, 32769],
                        [50001, 55001, 60001, 65535],
                        [65535, 0, 65535, 0],
                    ]
                };
                let mut e = fixture(4, 1, depth, space, |x, _| points[x as usize]);
                let before = e.document().unwrap().to_archive().unwrap();
                let cursor = e.snapshot().history_cursor;
                let max: f64 = if depth == 8 { 255. } else { 65535. };
                let black = if method == "contrast" {
                    [points[0][0]; 3]
                } else {
                    std::array::from_fn(|c| points[0][c])
                };
                let white = if method == "contrast" {
                    [points[2][2]; 3]
                } else {
                    std::array::from_fn(|c| points[2][c])
                };
                let result = run(&mut e, "image.auto", params(method, 0.));
                assert_eq!(result["method"], method);
                assert_eq!(result["changed"], true);
                assert_eq!(result["sampled_pixels"], 3);
                assert_eq!(e.snapshot().history_cursor, cursor + 1);
                assert_eq!(e.snapshot().history.last().unwrap().command, "image.auto");
                for (x, original) in points.iter().copied().enumerate() {
                    let p = raw(&e, x as i64, 0);
                    assert_eq!(p[3], original[3]);
                    for c in 0..3 {
                        let expected = if original[3] == 0 {
                            original[c]
                        } else {
                            ((f64::from(original[c]) - f64::from(black[c]))
                                / f64::from(white[c] - black[c])
                                * max)
                                .clamp(0., max)
                                .round() as u16
                        };
                        assert_eq!(p[c], expected, "{depth} {space} {method} {x},{c}");
                    }
                }
                for c in 0..3 {
                    assert!(
                        (result["input_black"][c].as_f64().unwrap() - f64::from(black[c]) / max)
                            .abs()
                            < 1e-12
                    );
                    assert!(
                        (result["input_white"][c].as_f64().unwrap() - f64::from(white[c]) / max)
                            .abs()
                            < 1e-12
                    );
                }
                let saved = e.document().unwrap().to_archive().unwrap();
                assert_eq!(
                    Document::from_archive(saved.clone())
                        .unwrap()
                        .to_archive()
                        .unwrap(),
                    saved
                );
                run(&mut e, "edit.undo", json!({}));
                assert!(
                    e.document().unwrap().to_archive().unwrap() == before,
                    "document archive changed"
                );
            }
        }
    }
}
#[test]
fn clipped_percentiles_weight_tiny_alpha_instead_of_giving_outliers_full_votes() {
    let points = [
        [0, 0, 0, 1],
        [10000, 10000, 10000, 65535],
        [20000, 20000, 20000, 65535],
        [30000, 30000, 30000, 65535],
        [65535, 65535, 65535, 1],
        [65535, 0, 65535, 0],
    ];
    let mut e = fixture(6, 1, 16, "display_p3", |x, _| points[x as usize]);
    let r = run(&mut e, "image.auto", params("tone", 5.));
    assert_eq!(r["sampled_pixels"], 5);
    assert!((r["weighted_samples"].as_f64().unwrap() - (3. + 2. / 65535.)).abs() < 1e-12);
    for c in 0..3 {
        assert_eq!(r["input_black"][c].as_f64().unwrap(), 10000. / 65535.);
        assert_eq!(r["input_white"][c].as_f64().unwrap(), 30000. / 65535.);
    }
    assert_eq!(raw(&e, 1, 0), [0, 0, 0, 65535]);
    assert_eq!(raw(&e, 2, 0), [32768, 32768, 32768, 65535]);
    assert_eq!(raw(&e, 3, 0), [65535; 4]);
    assert_eq!(raw(&e, 5, 0), points[5]);
    assert_eq!(raw(&e, 0, 0)[3], 1);
    assert_eq!(raw(&e, 4, 0)[3], 1);
    let mut unweighted = fixture(6, 1, 16, "display_p3", |x, _| {
        let mut p = points[x as usize];
        if p[3] == 1 {
            p[3] = 65535;
        }
        p
    });
    let r = run(&mut unweighted, "image.auto", params("tone", 5.));
    assert_eq!(r["input_black"], json!([0., 0., 0.]));
    assert_eq!(r["input_white"], json!([1., 1., 1.]));
    assert_eq!(r["changed"], false);
}

#[test]
fn exact_tail_percentages_remove_equal_rank_weight_from_both_ends() {
    for depth in [8, 16] {
        for method in ["contrast", "tone", "color"] {
            let mut e = fixture(20, 1, depth, "srgb", |x, _| {
                let v = if depth == 8 {
                    10 + x as u16 * 10
                } else {
                    1000 + x as u16 * 3000
                };
                [v, v, v, if depth == 8 { 255 } else { 65535 }]
            });
            let r = run(&mut e, "image.auto", params(method, 5.));
            let max: f64 = if depth == 8 { 255. } else { 65535. };
            let (lo, hi) = if depth == 8 {
                (20., 190.)
            } else {
                (4000., 55000.)
            };
            for c in 0..3 {
                assert_eq!(r["input_black"][c].as_f64().unwrap(), lo / max);
                assert_eq!(r["input_white"][c].as_f64().unwrap(), hi / max);
            }
            assert_eq!(raw(&e, 0, 0)[0], 0);
            assert_eq!(raw(&e, 1, 0)[0], 0);
            assert_eq!(raw(&e, 18, 0)[0], max as u16);
            assert_eq!(raw(&e, 19, 0)[0], max as u16);
        }
    }
}
#[test]
fn color_neutralizes_the_upper_weighted_median_with_controllable_gamma_and_tone_at_zero_strength() {
    let points = [
        [10u16, 20, 30, 255],
        [40, 100, 160, 255],
        [210, 220, 230, 255],
    ];
    for depth in [8, 16] {
        let source = fixture(3, 1, depth, "srgb", |x, _| {
            if depth == 8 {
                points[x as usize]
            } else {
                points[x as usize].map(|v| v * 257)
            }
        });
        let mut tone = source.clone();
        run(&mut tone, "image.auto", params("tone", 0.));
        let mut none = source.clone();
        let no = run(&mut none, "image.auto", color_params(0., 0.));
        assert_eq!(no["gamma"], json!([1., 1., 1.]));
        assert_eq!(
            none.document().unwrap().export_pixels().unwrap(),
            tone.document().unwrap().export_pixels().unwrap()
        );
        for strength in [0.5, 1.] {
            let mut e = source.clone();
            let r = run(&mut e, "image.auto", color_params(0., strength));
            let max: f64 = if depth == 8 { 255. } else { 65535. };
            let p = raw(&e, 1, 0);
            for (c, t) in [0.15f64, 0.4, 0.65].into_iter().enumerate() {
                let gamma = (t.ln() / 0.5f64.ln()).powf(strength);
                assert!((r["gamma"][c].as_f64().unwrap() - gamma).abs() < 1e-12);
                let expected = if strength == 1. {
                    (0.5 * max).round() as u16
                } else {
                    (t.powf(1. / gamma) * max).round() as u16
                };
                assert_eq!(p[c], expected);
            }
            if strength == 1. {
                assert_eq!(p[0], p[1]);
                assert_eq!(p[1], p[2]);
            }
        }
    }
}
#[test]
fn feathered_selection_statistics_and_single_linear_blend_match_an_independent_reference() {
    for depth in [8, 16] {
        for space in ["srgb", "display_p3"] {
            let max: f64 = if depth == 8 { 255. } else { 65535. };
            let mut e = fixture(11, 9, depth, space, |x, y| {
                let p = [
                    5000 + x as u16 * 4000,
                    8000 + y as u16 * 5000,
                    60000 - x as u16 * 3000,
                    if (x + y) % 5 == 0 { 0 } else { 32769 },
                ];
                if depth == 8 { p.map(|v| v / 257) } else { p }
            });
            run(
                &mut e,
                "select.marquee",
                json!({"x":3,"y":2,"width":5,"height":5}),
            );
            run(&mut e, "select.feather", json!({"radius":1.2}));
            let original = e.clone();
            let icc = original.document().unwrap().working_icc();
            let mut black = [u16::MAX; 3];
            let mut white = [0; 3];
            let selection = original.document().unwrap().selection().unwrap();
            let mut samples = 0;
            for y in 0..9 {
                for x in 0..11 {
                    let p = raw(&original, x, y);
                    if selection.sample(x, y).unwrap() > 0 && p[3] > 0 {
                        samples += 1;
                        for c in 0..3 {
                            black[c] = black[c].min(p[c]);
                            white[c] = white[c].max(p[c]);
                        }
                    }
                }
            }
            let result = run(&mut e, "image.auto", params("tone", 0.));
            assert_eq!(result["sampled_pixels"], samples);
            for y in 0..9 {
                for x in 0..11 {
                    let p = raw(&original, x, y);
                    let coverage = selection.sample(x, y).unwrap();
                    let actual = raw(&e, x, y);
                    assert_eq!(actual[3], p[3]);
                    if coverage == 0 || p[3] == 0 {
                        assert_eq!(actual, p);
                        continue;
                    }
                    let mapped: [f64; 3] = std::array::from_fn(|c| {
                        (f64::from(p[c]) - f64::from(black[c])) / f64::from(white[c] - black[c])
                    });
                    if coverage == 65535 {
                        for c in 0..3 {
                            assert_eq!(actual[c], (mapped[c] * max).round() as u16);
                        }
                        continue;
                    }
                    let old = p.map(|v| (f64::from(v) / max) as f32);
                    let new = [mapped[0] as f32, mapped[1] as f32, mapped[2] as f32, old[3]];
                    let back = strata_color::working_to_linear_p3(&[old], icc).unwrap()[0];
                    let front = strata_color::working_to_linear_p3(&[new], icc).unwrap()[0];
                    let w = f64::from(coverage) / 65535.;
                    let mix = std::array::from_fn(|c| {
                        if c == 3 {
                            old[3]
                        } else {
                            (f64::from(back[c]) * (1. - w) + f64::from(front[c]) * w) as f32
                        }
                    });
                    let expected =
                        strata_color::from_linear_p3(&[mix], e.document().unwrap().format(), icc)
                            .unwrap();
                    let codes = match expected {
                        PixelBuffer::Rgba8(v) => v.into_iter().map(u16::from).collect::<Vec<_>>(),
                        PixelBuffer::Rgba16(v) => v,
                    };
                    for c in 0..3 {
                        assert!(
                            actual[c].abs_diff(codes[c]) <= 4,
                            "{depth} {space} {x},{y},{c}: {actual:?} != {codes:?}"
                        );
                    }
                }
            }
            assert_eq!(
                e.document().unwrap().selection().unwrap().layout(),
                selection.layout()
            );
            for y in 0..9 {
                for x in 0..11 {
                    assert_eq!(
                        e.document()
                            .unwrap()
                            .selection()
                            .unwrap()
                            .sample(x, y)
                            .unwrap(),
                        selection.sample(x, y).unwrap()
                    );
                }
            }
        }
    }
}
#[test]
fn moved_cross_tile_layer_preserves_offboard_codes_masks_styles_tree_and_selection_planes() {
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
    let group = run(
        &mut e,
        "layer.group",
        json!({"layers":[{"id":id}],"name":"group"}),
    )["layer_id"]
        .clone();
    run(
        &mut e,
        "image.crop",
        json!({"rect":{"x":-5,"y":3,"width":20,"height":4},"pixels":"preserve"}),
    );
    run(&mut e, "select.all", json!({}));
    let original = e.clone();
    let before = e.document().unwrap().to_archive().unwrap();
    let r = run(&mut e, "image.auto", params("tone", 0.));
    assert_eq!(r["sampled_pixels"], 80);
    assert_eq!(r["touched_tiles"], 2);
    assert_eq!(e.snapshot().layers, original.snapshot().layers);
    assert_eq!(
        e.snapshot().active_layer,
        Some(serde_json::from_value(group).unwrap())
    );
    for y in 0..4 {
        for x in 0..520 {
            if !(255..275).contains(&x) {
                assert_eq!(raw(&e, x, y), raw(&original, x, y));
            } else {
                assert_eq!(raw(&e, x, y)[3], 32769);
            }
        }
    }
    let a = original
        .document()
        .unwrap()
        .layers()
        .iter()
        .find(|l| Some(l.id()) == id)
        .unwrap()
        .mask()
        .unwrap();
    let b = e
        .document()
        .unwrap()
        .layers()
        .iter()
        .find(|l| Some(l.id()) == id)
        .unwrap()
        .mask()
        .unwrap();
    assert_eq!(a.layout(), b.layout());
    for (coord, _) in a.tiles() {
        assert_eq!(a.tile_version(*coord), b.tile_version(*coord));
    }
    let saved = e.document().unwrap().to_archive().unwrap();
    assert_eq!(
        Document::from_archive(saved.clone())
            .unwrap()
            .to_archive()
            .unwrap(),
        saved
    );
    run(&mut e, "edit.undo", json!({}));
    assert!(
        e.document().unwrap().to_archive().unwrap() == before,
        "document archive changed"
    );
}
#[test]
fn flat_empty_transparent_and_already_full_range_results_are_history_free_and_keep_newer_layer_selection()
 {
    for method in ["tone", "contrast", "color"] {
        for points in [
            vec![[12000, 12000, 12000, 32769]; 4],
            vec![[10001, 30003, 50001, 0]; 4],
            vec![[0, 0, 0, 65535], [65535; 4], [0, 0, 0, 65535], [65535; 4]],
        ] {
            let mut e = fixture(4, 1, 16, "srgb", |x, _| points[x as usize]);
            let before = e.document().unwrap().to_archive().unwrap();
            let revision = e.revision();
            let cursor = e.snapshot().history_cursor;
            let prepared = e
                .prepare(e.request(Source::User), "image.auto", params(method, 0.))
                .unwrap();
            run(
                &mut e,
                "layer.select",
                json!({"layer":{"name":"Background"}}),
            );
            let selected = e.snapshot().active_layer;
            let selected_before = e.document().unwrap().to_archive().unwrap();
            let result = e.commit(prepared).unwrap();
            assert_eq!(result["changed"], false);
            assert_eq!(result["touched_tiles"], 0);
            assert_eq!(e.snapshot().active_layer, selected);
            assert_eq!(e.revision(), revision);
            assert_eq!(e.snapshot().history_cursor, cursor);
            assert!(
                e.document().unwrap().to_archive().unwrap() == selected_before,
                "unchanged auto candidate replaced newer layer selection"
            );
            assert!(
                e.document().unwrap().to_archive().unwrap().tiles == before.tiles,
                "unchanged candidate changed source codes"
            );
        }
        let mut e = fixture(4, 1, 16, "srgb", |x, _| {
            [1000 + x as u16 * 10000, 2000, 3000, 65535]
        });
        run(
            &mut e,
            "select.marquee",
            json!({"x":0,"y":0,"width":1,"height":1}),
        );

        run(
            &mut e,
            "select.marquee",
            json!({"x":0,"y":0,"width":1,"height":1,"mode":"subtract"}),
        );
        let before = e.document().unwrap().to_archive().unwrap();
        let rev = e.revision();
        assert_eq!(
            run(&mut e, "image.auto", params(method, 0.))["changed"],
            false
        );
        assert_eq!(e.revision(), rev);
        assert!(
            e.document().unwrap().to_archive().unwrap() == before,
            "document archive changed"
        );
    }
}
#[test]
fn strict_settings_nonpixel_ancestor_locks_and_failed_previews_never_publish_partial_edits() {
    let original = fixture(4, 2, 16, "srgb", |x, y| {
        [
            1000 + x as u16 * 10000,
            2000 + y as u16 * 20000,
            30003,
            32769,
        ]
    });
    for ancestor in [false, true] {
        let mut e = original.clone();
        let target = if ancestor {
            run(
                &mut e,
                "layer.group",
                json!({"layers":[{"name":"pixels"}],"name":"hidden"}),
            )["layer_id"]
                .clone()
        } else {
            json!(e.snapshot().active_layer)
        };
        run(
            &mut e,
            "layer.set_visibility",
            json!({"layer":{"id":target},"visible":false}),
        );
        let before = e.document().unwrap().to_archive().unwrap();
        assert!(
            e.execute(
                e.request(Source::User),
                "image.auto",
                params("tone", 0.),
                |_| Ok(())
            )
            .is_err()
        );
        assert!(
            e.document().unwrap().to_archive().unwrap() == before,
            "hidden source analysis modified the document"
        );
    }
    for settings in [
        json!({"method":"tone","clip_percent":-1}),
        json!({"method":"tone","clip_percent":5.01}),
        json!({"method":"tone","neutralize_strength":0.5}),
        json!({"method":"contrast","neutralize_strength":0.5}),
        json!({"method":"color","neutralize_strength":1.1}),
        json!({"method":"color","neutralize_strength":-0.1}),
        json!({"method":"guess"}),
        json!({"method":"tone","clip_percent":null}),
    ] {
        let mut e = original.clone();
        let before = e.document().unwrap().to_archive().unwrap();
        assert!(
            e.execute(
                e.request(Source::User),
                "image.auto",
                json!({"layer":{"name":"pixels"},"settings":settings}),
                |_| Ok(())
            )
            .is_err()
        );
        assert!(
            e.document().unwrap().to_archive().unwrap() == before,
            "document archive changed"
        );
    }
    for lock in ["all", "pixels"] {
        for ancestor in [false, true] {
            let mut e = original.clone();
            let target = if ancestor {
                run(
                    &mut e,
                    "layer.group",
                    json!({"layers":[{"name":"pixels"}],"name":"locked"}),
                )["layer_id"]
                    .clone()
            } else {
                json!(e.snapshot().active_layer)
            };
            run(
                &mut e,
                "layer.set_locks",
                json!({"layer":{"id":target},"locks":{lock:true}}),
            );
            let before = e.document().unwrap().to_archive().unwrap();
            assert!(
                e.execute(
                    e.request(Source::User),
                    "image.auto",
                    params("tone", 0.),
                    |_| Ok(())
                )
                .is_err()
            );
            assert!(
                e.document().unwrap().to_archive().unwrap() == before,
                "document archive changed"
            );
        }
    }
    for lock in ["position", "transparency"] {
        let mut e = original.clone();
        run(
            &mut e,
            "layer.set_locks",
            json!({"layer":{"name":"pixels"},"locks":{lock:true}}),
        );
        run(&mut e, "image.auto", params("tone", 0.));
        for y in 0..2 {
            for x in 0..4 {
                assert_eq!(raw(&e, x, y)[3], 32769);
            }
        }
    }
    let mut e = original.clone();
    run(
        &mut e,
        "layer.fill.gradient",
        json!({"name":"generated","start":[10001,30003,50001,65535],"end":[50001,10001,20003,65535]}),
    );
    let before = e.document().unwrap().to_archive().unwrap();
    assert!(
        e.execute(
            e.request(Source::User),
            "image.auto",
            json!({"layer":{"name":"generated"},"settings":{"method":"tone"}}),
            |_| Ok(())
        )
        .is_err()
    );
    assert!(
        e.document().unwrap().to_archive().unwrap() == before,
        "document archive changed"
    );
    let mut e = original;
    let before = e.document().unwrap().to_archive().unwrap();
    assert!(
        e.execute(
            e.request(Source::User),
            "image.auto",
            params("tone", 0.),
            |_| Err("display failed".into())
        )
        .is_err()
    );
    assert!(
        e.document().unwrap().to_archive().unwrap() == before,
        "document archive changed"
    );
}
#[test]
fn pixel_preview_is_original_context_bound_and_only_its_local_cancel_is_revoked() {
    let mut e = fixture(4, 2, 16, "srgb", |x, y| {
        [
            1000 + x as u16 * 10000,
            2000 + y as u16 * 20000,
            30003,
            32769,
        ]
    });
    let id = e.snapshot().active_layer.unwrap();
    let interaction = e
        .begin_filter(strata_core::LayerRef::Id(id), "image.auto")
        .unwrap();
    let before = e.document().unwrap().to_archive().unwrap();
    let p = e
        .preview_filter(
            &interaction,
            "image.auto",
            json!({"settings":{"method":"tone","clip_percent":0}}),
        )
        .unwrap();
    assert!(
        e.document().unwrap().to_archive().unwrap() == before,
        "document archive changed"
    );
    assert_ne!(
        p.document().unwrap().to_archive().unwrap().tiles,
        before.tiles
    );
    assert!(
        e.preview_filter(
            &interaction,
            "image.auto",
            json!({"layer":{"name":"Background"},"settings":{"method":"tone"}})
        )
        .is_err()
    );
    let work = strata_core::commands::WorkCancellation::default();
    let local = interaction.with_work_cancellation(&work);
    work.cancel();
    assert!(
        e.preview_filter(&local, "image.auto", json!({"settings":{"method":"tone"}}))
            .is_err()
    );
    assert!(
        e.preview_filter(
            &interaction,
            "image.auto",
            json!({"settings":{"method":"contrast"}})
        )
        .is_ok()
    );
    run(
        &mut e,
        "layer.rename",
        json!({"layer":{"name":"pixels"},"name":"changed"}),
    );
    assert!(e.commit(p).is_err());
    assert_eq!(e.snapshot().history.last().unwrap().command, "layer.rename");
}
#[test]
fn shared_budget_and_cancellation_during_output_keep_original_samples_and_release_scratch() {
    for budget in [4 * 1024 * 1024, 128 * 1024 * 1024] {
        let pool = MemoryPool::new(MemoryLimits {
            cpu_bytes: budget,
            gpu_bytes: 0,
            disk_bytes: 256 * 1024 * 1024,
        });
        let mut e = Editor::with_memory_pool(pool.clone());
        run(
            &mut e,
            "file.new",
            json!({"width":512,"height":512,"bit_depth":16,"background":[10001,30003,50001,32769]}),
        );
        let before = e.document().unwrap().to_archive().unwrap();
        let rev = e.revision();
        let cursor = e.snapshot().history_cursor;
        let p = json!({"layer":{"name":"Background"},"settings":{"method":"contrast","clip_percent":0}});
        if budget < 8 * 1024 * 1024 {
            assert!(
                e.execute(e.request(Source::User), "image.auto", p, |_| Ok(()))
                    .is_err()
            );
        } else {
            let source = e.clone();
            let req = e.request(Source::User);
            let worker = std::thread::spawn(move || source.prepare(req, "image.auto", p));
            let start = std::time::Instant::now();
            while pool.stats().transient_bytes < 8 * 1024 * 1024
                && start.elapsed() < std::time::Duration::from_secs(5)
            {
                std::thread::yield_now();
            }
            assert!(pool.stats().transient_bytes >= 8 * 1024 * 1024);
            e.cancel_pending_work();
            assert!(worker.join().unwrap().is_err());
        }
        assert_eq!(e.revision(), rev);
        assert_eq!(e.snapshot().history_cursor, cursor);
        assert!(
            e.document().unwrap().to_archive().unwrap() == before,
            "document archive changed"
        );
        assert_eq!(pool.stats().transient_bytes, 0);
    }
}
