use serde_json::{Value, json};
use strata_core::{Document, Editor, Source, geometry::Homography};
use strata_pixels::memory::{MemoryLimits, MemoryPool};
fn run(e: &mut Editor, id: &str, p: Value) -> Value {
    e.execute(e.request(Source::User), id, p, |_| Ok(()))
        .unwrap()
}
fn fixture(depth: u8, size: [u32; 2]) -> Editor {
    let mut e = Editor::new();
    let color = if depth == 16 {
        [10001, 30003, 50001, 65535]
    } else {
        [39, 117, 194, 255]
    };
    run(
        &mut e,
        "file.new",
        json!({"width":size[0],"height":size[1],"bit_depth":depth,"working_space":"display_p3","background":color}),
    );
    e
}
fn reference_transform(before: &strata_core::Frame, angle: f64) -> Homography {
    let (c, s) = (angle.to_radians().cos(), angle.to_radians().sin());
    let (cx, cy) = (
        before.x as f64 + f64::from(before.width) / 2.,
        before.y as f64 + f64::from(before.height) / 2.,
    );
    let tx = cx - c * cx + s * cy;
    let ty = cy - s * cx - c * cy;
    let minx = [-0.5, f64::from(before.width) + 0.5]
        .into_iter()
        .flat_map(|x| {
            [-0.5, f64::from(before.height) + 0.5]
                .into_iter()
                .map(move |y| c * (before.x as f64 + x) - s * (before.y as f64 + y) + tx)
        })
        .fold(f64::INFINITY, f64::min)
        .floor();
    let miny = [-0.5, f64::from(before.width) + 0.5]
        .into_iter()
        .flat_map(|x| {
            [-0.5, f64::from(before.height) + 0.5]
                .into_iter()
                .map(move |y| s * (before.x as f64 + x) + c * (before.y as f64 + y) + ty)
        })
        .fold(f64::INFINITY, f64::min)
        .floor();
    Homography::affine([
        c,
        s,
        -s,
        c,
        tx + before.x as f64 - minx,
        ty + before.y as f64 - miny,
    ])
    .unwrap()
}
#[test]
fn positive_negative_and_diagonal_straighten_safe_crop_stays_opaque_with_one_exact_undo() {
    for depth in [8, 16] {
        for mode in ["nearest", "bilinear"] {
            for angle in [5., -17., 45.] {
                let mut e = fixture(depth, [40, 26]);
                let before = e.document().unwrap().to_archive().unwrap();
                let frame = e.document().unwrap().frame().clone();
                let cursor = e.snapshot().history_cursor;
                let r = run(
                    &mut e,
                    "image.straighten",
                    json!({"reference":{"type":"angle","angle_degrees":angle},"interpolation":mode}),
                );
                assert_eq!(r["changed"], true);
                assert_eq!(r["cropped"], true);
                assert_eq!(r["preserved_external_content"], true);
                assert_eq!(e.snapshot().history_cursor, cursor + 1);
                assert_eq!(
                    e.snapshot().history.last().unwrap().command,
                    "image.straighten"
                );
                let crop = e.document().unwrap().frame();
                let m = reference_transform(&frame, angle);
                let inset = if mode == "bilinear" { 0.5 } else { 0. };
                for [x, y] in [
                    [crop.x, crop.y],
                    [crop.x + i64::from(crop.width), crop.y],
                    [
                        crop.x + i64::from(crop.width),
                        crop.y + i64::from(crop.height),
                    ],
                    [crop.x, crop.y + i64::from(crop.height)],
                ] {
                    let [sx, sy] = m.unmap([x as f64, y as f64]).unwrap();
                    assert!(
                        sx >= inset - 1e-8
                            && sy >= inset - 1e-8
                            && sx <= 40. - inset + 1e-8
                            && sy <= 26. - inset + 1e-8,
                        "{mode} {angle} {sx},{sy}"
                    );
                }
                let exported = e.document().unwrap().export_pixels().unwrap();
                match exported {
                    strata_pixels::PixelBuffer::Rgba8(v) => {
                        assert!(v.chunks_exact(4).all(|p| p[3] == 255))
                    }
                    strata_pixels::PixelBuffer::Rgba16(v) => {
                        assert!(v.chunks_exact(4).all(|p| p[3] == 65535))
                    }
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
                assert_eq!(e.document().unwrap().to_archive().unwrap(), before);
            }
        }
    }
}
#[test]
fn horizontal_vertical_and_reversed_lines_match_independent_angle_and_expanded_rotation() {
    for (start, end, align, expected) in [
        (
            [2., 3.],
            [32., 8.],
            "horizontal",
            -(5f64 / 30.).atan().to_degrees(),
        ),
        (
            [32., 8.],
            [2., 3.],
            "horizontal",
            -(5f64 / 30.).atan().to_degrees(),
        ),
        (
            [3., 2.],
            [8., 32.],
            "vertical",
            (5f64 / 30.).atan().to_degrees(),
        ),
        (
            [8., 32.],
            [3., 2.],
            "vertical",
            (5f64 / 30.).atan().to_degrees(),
        ),
    ] {
        let mut e = fixture(16, [40, 26]);
        let mut reference = e.clone();
        let r = run(
            &mut e,
            "image.straighten",
            json!({"reference":{"type":"line","start":start,"end":end,"align":align},"crop":false,"interpolation":"nearest"}),
        );
        let angle = r["angle_degrees"].as_f64().unwrap();
        assert!((angle - expected).abs() < 1e-10);
        assert_eq!(r["cropped"], false);
        run(
            &mut reference,
            "image.rotate",
            json!({"angle_degrees":angle,"interpolation":"nearest"}),
        );
        assert_eq!(
            e.document().unwrap().frame(),
            reference.document().unwrap().frame()
        );
        assert_eq!(
            e.document().unwrap().export_pixels().unwrap(),
            reference.document().unwrap().export_pixels().unwrap()
        );
        assert_eq!(e.snapshot().layers, reference.snapshot().layers);
        let (c, s) = (angle.to_radians().cos(), angle.to_radians().sin());
        let dx = end[0] - start[0];
        let dy = end[1] - start[1];
        let residual = if align == "horizontal" {
            s * dx + c * dy
        } else {
            c * dx - s * dy
        };
        assert!(residual.abs() < 1e-10);
    }
}
#[test]
fn negative_frame_hidden_group_masks_and_selections_match_rotation_then_preserving_crop() {
    let mut e = fixture(16, [40, 26]);
    let id = e.snapshot().active_layer;
    run(
        &mut e,
        "select.marquee",
        json!({"x":3,"y":4,"width":19,"height":16}),
    );
    run(&mut e, "select.feather", json!({"radius":1.2}));
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
        "layer.set_position",
        json!({"layer":{"id":id},"x":-12,"y":7}),
    );
    run(
        &mut e,
        "layer.set_visibility",
        json!({"layer":{"id":id},"visible":false}),
    );
    let group = run(
        &mut e,
        "layer.group",
        json!({"layers":[{"id":id}],"name":"hidden source"}),
    )["layer_id"]
        .clone();
    run(
        &mut e,
        "layer.mask.add",
        json!({"layer":{"id":group},"source":"white"}),
    );
    run(
        &mut e,
        "image.crop",
        json!({"rect":{"x":-5,"y":3,"width":40,"height":26},"pixels":"preserve"}),
    );
    let original = e.document().unwrap().to_archive().unwrap();
    let mut comparison = e.clone();
    let r = run(
        &mut e,
        "image.straighten",
        json!({"reference":{"type":"angle","angle_degrees":-12}}),
    );
    let f = e.document().unwrap().frame().clone();
    run(
        &mut comparison,
        "image.rotate",
        json!({"angle_degrees":-12}),
    );
    run(
        &mut comparison,
        "image.crop",
        json!({"rect":{"x":f.x,"y":f.y,"width":f.width,"height":f.height},"pixels":"preserve"}),
    );
    assert_eq!(e.snapshot().layers, comparison.snapshot().layers);
    assert_eq!(
        e.document().unwrap().frame(),
        comparison.document().unwrap().frame()
    );
    assert_eq!(
        e.document().unwrap().export_pixels().unwrap(),
        comparison.document().unwrap().export_pixels().unwrap()
    );
    let doc = e.document().unwrap();
    assert_eq!(doc.selection().unwrap().layout().bounds.width, f.width);
    assert_eq!(
        doc.previous_selection().unwrap().layout().bounds.height,
        f.height
    );
    let layer = doc.layers().iter().find(|l| Some(l.id()) == id).unwrap();
    assert!(!layer.info().visible);
    assert!(!layer.info().mask.as_ref().unwrap().enabled);
    assert_eq!(r["preserved_layer_structure"], true);
    assert!(
        strata_core::geometry_pixels::content_bounds(
            layer.pixels(),
            [layer.info().x, layer.info().y],
            &|| false
        )
        .unwrap()
        .is_some_and(|b| b.x < f.x
            || b.y < f.y
            || b.x + i64::from(b.width) > f.x + i64::from(f.width)
            || b.y + i64::from(b.height) > f.y + i64::from(f.height))
    );
    run(&mut e, "edit.undo", json!({}));
    assert_eq!(e.document().unwrap().to_archive().unwrap(), original);
}
#[test]
fn zero_correction_noop_cannot_revert_new_selection_and_invalid_or_thin_crops_are_atomic() {
    let mut e = fixture(16, [40, 26]);
    let duplicate = run(
        &mut e,
        "layer.duplicate",
        json!({"layer":{"name":"Background"}}),
    )["layer_id"]
        .clone();
    let prepared = e
        .prepare(
            e.request(Source::User),
            "image.straighten",
            json!({"reference":{"type":"line","start":[0,0],"end":[10,0],"align":"horizontal"}}),
        )
        .unwrap();
    run(
        &mut e,
        "layer.select",
        json!({"layer":{"name":"Background"}}),
    );
    let active = e.snapshot().active_layer;
    let revision = e.revision();
    assert_eq!(e.commit(prepared).unwrap()["changed"], false);
    assert_eq!(e.revision(), revision);
    assert_eq!(e.snapshot().active_layer, active);
    assert_ne!(duplicate, serde_json::to_value(active).unwrap());
    let before = e.document().unwrap().to_archive().unwrap();
    for reference in [
        json!({"type":"angle","angle_degrees":46}),
        json!({"type":"angle","angle_degrees":-46}),
        json!({"type":"line","start":[1,1],"end":[1,1],"align":"horizontal"}),
        json!({"type":"line","start":[0,0],"end":[1,20],"align":"horizontal"}),
        json!({"type":"line","start":[0,0],"end":[20,1],"align":"diagonal"}),
        json!({"type":"line","start":[1e13,0],"end":[0,1],"align":"horizontal"}),
        json!({"type":"angle","angle_degrees":5,"delete":true}),
    ] {
        assert!(
            e.execute(
                e.request(Source::User),
                "image.straighten",
                json!({"reference":reference}),
                |_| Ok(())
            )
            .is_err()
        );
        assert_eq!(e.document().unwrap().to_archive().unwrap(), before);
    }
    let mut thin = fixture(16, [1, 20]);
    let before = thin.document().unwrap().to_archive().unwrap();
    assert!(
        thin.execute(
            thin.request(Source::User),
            "image.straighten",
            json!({"reference":{"type":"angle","angle_degrees":20}}),
            |_| Ok(())
        )
        .is_err()
    );
    assert_eq!(thin.document().unwrap().to_archive().unwrap(), before);
    run(
        &mut thin,
        "image.straighten",
        json!({"reference":{"type":"angle","angle_degrees":20},"crop":false}),
    );
}
#[test]
fn editable_types_can_straighten_and_failures_cannot_publish_rotation_or_crop() {
    for kind in ["fill", "style", "lock"] {
        let mut e = fixture(16, [40, 26]);
        let id = e.snapshot().active_layer;
        match kind {
            "fill" => {
                run(
                    &mut e,
                    "layer.fill.solid",
                    json!({"name":"fill","color":[10001,30003,50001,65535]}),
                );
            }
            "style" => {
                run(
                    &mut e,
                    "layer.set_styles",
                    json!({"layer":{"id":id},"styles":{"color_overlay":{"color":[10001,30003,50001,65535],"opacity":0.5}}}),
                );
            }
            _ => {
                run(
                    &mut e,
                    "layer.set_locks",
                    json!({"layer":{"id":id},"locks":{"position":true}}),
                );
            }
        }
        let before = e.document().unwrap().to_archive().unwrap();
        let revision = e.revision();
        assert_eq!(
            run(
                &mut e,
                "image.straighten",
                json!({"reference":{"type":"angle","angle_degrees":0}})
            )["changed"],
            false
        );
        let result = e.execute(
            e.request(Source::User),
            "image.straighten",
            json!({"reference":{"type":"angle","angle_degrees":5}}),
            |_| Ok(()),
        );
        if kind == "lock" {
            assert!(result.is_err());
            assert_eq!(e.revision(), revision);
        } else {
            assert_eq!(result.unwrap()["cropped"], true);
            run(&mut e, "edit.undo", json!({}));
        }
        assert_eq!(e.document().unwrap().to_archive().unwrap(), before);
    }
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
            json!({"width":512,"height":512,"bit_depth":16,"background":[10001,30003,50001,65535]}),
        );
        let before = e.document().unwrap().to_archive().unwrap();
        let revision = e.revision();
        if budget < 16 * 1024 * 1024 {
            assert!(
                e.execute(
                    e.request(Source::User),
                    "image.straighten",
                    json!({"reference":{"type":"angle","angle_degrees":17}}),
                    |_| Ok(())
                )
                .is_err()
            );
        } else {
            let source = e.clone();
            let request = e.request(Source::User);
            let thread = std::thread::spawn(move || {
                source.prepare(
                    request,
                    "image.straighten",
                    json!({"reference":{"type":"angle","angle_degrees":17}}),
                )
            });
            let start = std::time::Instant::now();
            while pool.stats().transient_bytes < 12 * 1024 * 1024
                && start.elapsed() < std::time::Duration::from_secs(3)
            {
                std::thread::yield_now();
            }
            assert!(pool.stats().transient_bytes >= 12 * 1024 * 1024);
            e.cancel_pending_work();
            assert!(thread.join().unwrap().is_err());
        }
        assert_eq!(e.revision(), revision);
        assert_eq!(e.document().unwrap().to_archive().unwrap(), before);
        assert!(e.snapshot().history.is_empty());
        assert_eq!(pool.stats().transient_bytes, 0);
    }
}
