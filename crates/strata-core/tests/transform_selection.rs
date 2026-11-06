use serde_json::{Value, json};
use strata_core::{ClipboardImage, Document, Editor, Source};
use strata_pixels::{PixelBuffer, PixelValue, memory::MemoryPool};
fn run(e: &mut Editor, id: &str, p: Value) -> Value {
    e.execute(e.request(Source::User), id, p, |_| Ok(()))
        .unwrap()
}
fn fixture(depth: u8) -> Editor {
    let mut e = Editor::new();
    run(
        &mut e,
        "file.new",
        json!({"width":32,"height":16,"bit_depth":depth,"working_space":"display_p3"}),
    );
    let mut v = Vec::new();
    for y in 0..16 {
        for x in 0..32 {
            v.extend([
                10001 + x as u16 * 1400,
                20003 + y as u16 * 1100,
                50001,
                if x % 7 == 0 { 0 } else { 32769 },
            ]);
        }
    }
    let pixels = if depth == 16 {
        PixelBuffer::Rgba16(v)
    } else {
        PixelBuffer::Rgba8(v.into_iter().map(|v| (v / 257) as u8).collect())
    };
    let image = ClipboardImage::new(
        pixels,
        32,
        16,
        0,
        0,
        e.document().unwrap().working_icc().to_vec(),
        &e.document().unwrap().memory_pool(),
    )
    .unwrap();
    let resource = e.grant_clipboard_image(image, Source::User).unwrap();
    run(&mut e, "edit.paste_in_place", json!({"resource":resource}));
    e
}
fn pixel(e: &Editor, x: i64, y: i64) -> [u16; 4] {
    let l = e.document().unwrap().layers().last().unwrap();
    match l.pixels().pixel(x - l.info().x, y - l.info().y).unwrap() {
        PixelValue::Rgba8(v) => v.map(u16::from),
        PixelValue::Rgba16(v) => v,
    }
}
fn params(e: &Editor, dx: f64, dy: f64) -> Value {
    json!({"layer":{"id":e.snapshot().active_layer},"scope":"selection","geometry":{"type":"components","translation":[dx,dy]}})
}
#[test]
fn selected_integer_move_preserves_unselected_samples_mask_position_and_offboard_destination() {
    for depth in [8, 16] {
        let mut e = fixture(depth);
        let target = e.snapshot().active_layer;
        run(
            &mut e,
            "select.marquee",
            json!({"x":2,"y":3,"width":5,"height":6}),
        );
        run(
            &mut e,
            "layer.mask.add",
            json!({"layer":{"id":target},"source":"white"}),
        );
        let before = e.clone();
        let archive = e.document().unwrap().to_archive().unwrap();
        let oldinfo = e
            .document()
            .unwrap()
            .layers()
            .last()
            .unwrap()
            .info()
            .clone();
        let history = e.snapshot().history.len();
        let p = params(&e, 40., 2.);
        let answer = run(&mut e, "transform.apply", p);
        assert_eq!(answer["scope"], "selection");
        assert_eq!(answer["mask_transformed"], false);
        assert_eq!(answer["selection_transformed"], true);
        assert_eq!(e.snapshot().history.len(), history + 1);
        for y in 0..16 {
            for x in 0..32 {
                let actual = pixel(&e, x, y);
                let old = pixel(&before, x, y);
                if (2..7).contains(&x) && (3..9).contains(&y) && old[3] > 0 {
                    assert_eq!(&actual[..3], &old[..3]);
                    assert_eq!(actual[3], 0);
                } else {
                    assert_eq!(actual, old);
                }
            }
        }
        for y in 3..9 {
            for x in 2..7 {
                assert_eq!(pixel(&e, x + 40, y + 2), pixel(&before, x, y));
            }
        }
        assert_eq!(
            e.document().unwrap().layers().last().unwrap().info(),
            &oldinfo
        );
        assert!(
            e.document()
                .unwrap()
                .selection()
                .unwrap()
                .content_bounds()
                .unwrap()
                .is_none()
        );
        let current = e.document().unwrap().to_archive().unwrap();
        assert_eq!(
            Document::from_archive(current.clone())
                .unwrap()
                .to_archive()
                .unwrap(),
            current
        );
        run(&mut e, "edit.undo", json!({}));
        assert_eq!(e.document().unwrap().to_archive().unwrap(), archive);
    }
}
#[test]
fn identity_is_bit_exact_and_partial_coverage_uses_source_over_complement_once() {
    for depth in [8, 16] {
        let mut e = fixture(depth);
        run(
            &mut e,
            "select.marquee",
            json!({"x":3,"y":3,"width":5,"height":5}),
        );
        run(&mut e, "select.feather", json!({"radius":2}));
        let before = e.clone();
        let p = params(&e, 0., 0.);
        run(&mut e, "transform.apply", p);
        assert_eq!(
            e.document().unwrap().to_archive().unwrap().tiles,
            before.document().unwrap().to_archive().unwrap().tiles
        );
        run(&mut e, "edit.undo", json!({}));
        let selection = e.document().unwrap().selection().unwrap().clone();
        let p = params(&e, 40., 0.);
        run(&mut e, "transform.apply", p);
        let max = if depth == 16 { 65535. } else { 255. };
        for y in 0..16 {
            for x in 0..32 {
                let old = pixel(&before, x, y);
                let w = selection.sample(x, y).unwrap();
                if old[3] == 0 || w == 0 {
                    assert_eq!(pixel(&e, x, y), old);
                    continue;
                }
                let a = f64::from(old[3]) / max;
                let s = ((a * 65535. * f64::from(w) / 65535.).round()) / 65535.;
                let expected =
                    ((if s == 1. { 0. } else { (a - s) / (1. - s) }) * max).round() as u16;
                assert!(pixel(&e, x, y)[3].abs_diff(expected) <= 1);
                let moved = pixel(&e, x + 40, y);
                assert!(moved[3].abs_diff((s * max).round() as u16) <= 1);
                if moved[3] > 0 {
                    assert_eq!(&moved[..3], &old[..3]);
                }
            }
        }
    }
}
#[test]
fn moved_selection_and_overlapping_linear_composite_match_independent_reference() {
    let mut e = fixture(16);
    run(
        &mut e,
        "select.marquee",
        json!({"x":3,"y":3,"width":5,"height":5}),
    );
    let before = e.clone();
    let p = params(&e, 4., 0.);
    run(&mut e, "transform.apply", p);
    assert_eq!(
        e.document()
            .unwrap()
            .selection()
            .unwrap()
            .sample(8, 5)
            .unwrap(),
        65535
    );
    assert_eq!(
        e.document()
            .unwrap()
            .selection()
            .unwrap()
            .sample(4, 5)
            .unwrap(),
        0
    );
    let old = pixel(&before, 9, 5);
    let front = pixel(&before, 5, 5);
    let icc = e.document().unwrap().working_icc();
    let linear = strata_color::to_linear_p3(
        &PixelBuffer::Rgba16(old.into_iter().chain(front).collect()),
        2,
        1,
        icc,
    )
    .unwrap();
    let ba = f64::from(old[3]) / 65535.;
    let sa = f64::from(front[3]) / 65535.;
    let alpha = sa + ba * (1. - sa);
    let rgb: [f32; 4] = std::array::from_fn(|c| {
        if c == 3 {
            alpha as f32
        } else {
            ((f64::from(linear[1][c]) * sa + f64::from(linear[0][c]) * ba * (1. - sa)) / alpha)
                as f32
        }
    });
    let PixelBuffer::Rgba16(expected) =
        strata_color::from_linear_p3(&[rgb], strata_pixels::PixelFormat::Rgba16, icc).unwrap()
    else {
        panic!()
    };
    let actual = pixel(&e, 9, 5);
    assert!((0..4).all(|c| actual[c].abs_diff(expected[c]) <= 2));
}
#[test]
fn fractional_selected_transform_does_not_reclip_destination_and_failure_is_atomic() {
    let mut e = fixture(16);
    let before = e.document().unwrap().to_archive().unwrap();
    let p = params(&e, 4., 0.);
    assert!(
        e.execute(e.request(Source::User), "transform.apply", p, |_| Ok(()))
            .is_err()
    );
    assert_eq!(e.document().unwrap().to_archive().unwrap(), before);
    run(
        &mut e,
        "select.marquee",
        json!({"x":3,"y":3,"width":3,"height":4}),
    );
    let before = e.clone();
    let p = params(&e, 10.5, 0.25);
    run(&mut e, "transform.apply", p);
    assert_ne!(pixel(&e, 14, 5), pixel(&before, 14, 5));
    assert_eq!(
        e.document()
            .unwrap()
            .selection()
            .unwrap()
            .sample(14, 5)
            .unwrap(),
        65535
    );
    let target = e.snapshot().active_layer;
    run(
        &mut e,
        "layer.set_locks",
        json!({"layer":{"id":target},"locks":{"transparency":true}}),
    );
    let snapshot = e.document().unwrap().to_archive().unwrap();
    let p = params(&e, 2., 1.);
    assert!(
        e.execute(e.request(Source::User), "transform.apply", p, |_| Ok(()))
            .is_err()
    );
    assert_eq!(e.document().unwrap().to_archive().unwrap(), snapshot);
    let mut limited =
        Editor::with_memory_pool(MemoryPool::new(strata_pixels::memory::MemoryLimits {
            cpu_bytes: 1024 * 1024,
            gpu_bytes: 0,
            disk_bytes: 32 * 1024 * 1024,
        }));
    run(
        &mut limited,
        "file.new",
        json!({"width":16,"height":16,"bit_depth":16,"background":[10001,30003,50001,32769]}),
    );
    run(&mut limited, "select.all", json!({}));
    let revision = limited.revision();
    let p = params(&limited, 2., 1.);
    assert!(
        limited
            .execute(limited.request(Source::User), "transform.apply", p, |_| Ok(
                ()
            ))
            .is_err()
    );
    assert_eq!(limited.revision(), revision);
}
#[test]
fn fractional_lift_over_an_unselected_opaque_neighbor_matches_premultiplied_linear_reference() {
    let mut e = Editor::new();
    run(
        &mut e,
        "file.new",
        json!({"width":8,"height":4,"bit_depth":16,"working_space":"srgb"}),
    );
    let image = ClipboardImage::new(
        PixelBuffer::Rgba16(vec![65535, 0, 0, 65535, 0, 0, 65535, 65535]),
        2,
        1,
        0,
        0,
        e.document().unwrap().working_icc().to_vec(),
        &e.document().unwrap().memory_pool(),
    )
    .unwrap();
    let resource = e.grant_clipboard_image(image, Source::User).unwrap();
    run(&mut e, "edit.paste_in_place", json!({"resource":resource}));
    run(
        &mut e,
        "select.marquee",
        json!({"x":0,"y":0,"width":1,"height":1}),
    );
    let p = params(&e, 0.5, 0.);
    run(&mut e, "transform.apply", p);
    let left = pixel(&e, 0, 0);
    assert_eq!(&left[..3], &[65535, 0, 0]);
    assert!(left[3].abs_diff(32768) <= 1);
    let icc = e.document().unwrap().working_icc();
    let reference = strata_color::to_linear_p3(
        &PixelBuffer::Rgba16(vec![65535, 0, 0, 65535, 0, 0, 65535, 65535]),
        2,
        1,
        icc,
    )
    .unwrap();
    let value = std::array::from_fn(|c| {
        if c == 3 {
            1.
        } else {
            (reference[0][c] + reference[1][c]) / 2.
        }
    });
    let PixelBuffer::Rgba16(expected) =
        strata_color::from_linear_p3(&[value], strata_pixels::PixelFormat::Rgba16, icc).unwrap()
    else {
        panic!()
    };
    let right = pixel(&e, 1, 0);
    assert!(
        (0..4).all(|c| right[c].abs_diff(expected[c]) <= 2),
        "{right:?} != {expected:?}"
    );
    assert!(
        e.document()
            .unwrap()
            .selection()
            .unwrap()
            .sample(1, 0)
            .unwrap()
            .abs_diff(32768)
            <= 1
    );
}
#[test]
fn negative_frame_and_moved_layer_keep_links_and_select_all_matches_projective_visible_codes() {
    let mut e = fixture(16);
    let target = e.snapshot().active_layer;
    run(
        &mut e,
        "layer.set_position",
        json!({"layer":{"id":target},"x":-129,"y":25}),
    );
    run(
        &mut e,
        "image.crop",
        json!({"rect":{"x":-129,"y":25,"width":32,"height":16},"pixels":"preserve"}),
    );
    run(&mut e, "select.all", json!({}));
    for geometry in [
        json!({"type":"components","rotation_degrees":90,"pivot":[-113,33]}),
        json!({"type":"corners","source":[[-129,25],[-97,25],[-97,41],[-129,41]],"destination":[[-127,27],[-93,24],[-95,43],[-130,42]]}),
    ] {
        let mut whole = e.clone();
        let mut selected = e.clone();
        let p = json!({"layer":{"id":target},"scope":"whole_layer","geometry":geometry});
        run(&mut whole, "transform.apply", p);
        let p = json!({"layer":{"id":target},"scope":"selection","geometry":geometry});
        run(&mut selected, "transform.apply", p);
        for y in 10..55 {
            for x in -140..-80 {
                let a = pixel(&whole, x, y);
                let b = pixel(&selected, x, y);
                assert_eq!(a[3], b[3], "alpha {x},{y}");
                if a[3] > 0 {
                    assert!(
                        (0..3).all(|c| a[c].abs_diff(b[c]) <= 1),
                        "color {x},{y}: {a:?} != {b:?}"
                    );
                }
            }
        }
        let info = selected.document().unwrap().layers().last().unwrap().info();
        assert_eq!((info.x, info.y), (-129, 25));
    }
}
#[test]
fn cancellation_during_selected_source_work_keeps_the_original_and_releases_transients() {
    let pool = MemoryPool::new(strata_pixels::memory::MemoryLimits {
        cpu_bytes: 128 * 1024 * 1024,
        gpu_bytes: 0,
        disk_bytes: 128 * 1024 * 1024,
    });
    let mut e = Editor::with_memory_pool(pool.clone());
    run(
        &mut e,
        "file.new",
        json!({"width":512,"height":512,"bit_depth":16,"background":[10001,30003,50001,32769]}),
    );
    run(&mut e, "select.all", json!({}));
    let revision = e.revision();
    let source = e.clone();
    let request = e.request(Source::User);
    let worker = std::thread::spawn(move || {
        source.prepare(request,"transform.apply",json!({"layer":{"name":"Background"},"scope":"selection","geometry":{"type":"components","translation":[0.5,0.25]}}))
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
    assert_eq!(e.revision(), revision);
    assert_eq!(pool.stats().transient_bytes, 0);
}
#[test]
fn ancestor_transparency_lock_rejects_selected_and_whole_layer_transforms() {
    let mut e = fixture(16);
    run(&mut e, "select.all", json!({}));
    let target = e.snapshot().active_layer;
    let group = run(
        &mut e,
        "layer.group",
        json!({"layers":[{"id":target}],"name":"locked parent"}),
    )["layer_id"]
        .clone();
    run(
        &mut e,
        "layer.set_locks",
        json!({"layer":{"id":group},"locks":{"transparency":true}}),
    );
    let before = e.document().unwrap().to_archive().unwrap();
    for scope in ["selection", "whole_layer"] {
        let p = json!({"layer":{"id":target},"scope":scope,"geometry":{"type":"components","translation":[3,2]}});
        assert!(
            e.execute(e.request(Source::User), "transform.apply", p, |_| Ok(()))
                .is_err()
        );
        assert_eq!(e.document().unwrap().to_archive().unwrap(), before);
    }
}
#[test]
fn projective_horizon_outside_selected_support_does_not_reject_valid_selected_content() {
    let mut e = fixture(16);
    run(
        &mut e,
        "select.marquee",
        json!({"x":2,"y":3,"width":4,"height":5}),
    );
    let before = e.document().unwrap().to_archive().unwrap();
    let target = e.snapshot().active_layer;
    let answer = run(
        &mut e,
        "transform.apply",
        json!({"layer":{"id":target},"scope":"selection","geometry":{"type":"matrix","values":[1,0,0,0,1,0,0.1,0,-1]}}),
    );
    assert_eq!(answer["selection_transformed"], true);
    assert!(
        e.document()
            .unwrap()
            .selection()
            .unwrap()
            .content_bounds()
            .unwrap()
            .is_none()
    );
    run(&mut e, "edit.undo", json!({}));
    assert_eq!(e.document().unwrap().to_archive().unwrap(), before);
}
