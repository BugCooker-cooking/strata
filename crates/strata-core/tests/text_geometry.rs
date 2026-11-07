use serde_json::{Value, json};
use std::collections::BTreeMap;
use strata_core::{Document, Editor, LayerKind, Source};
use strata_pixels::PixelBuffer;

type Samples = BTreeMap<(i64, i64), [u16; 4]>;
fn run(e: &mut Editor, command: &str, params: Value) -> Value {
    e.execute(e.request(Source::User), command, params, |_| Ok(()))
        .unwrap()
}
fn fixture(depth: u8) -> Editor {
    let mut e = Editor::new();
    run(
        &mut e,
        "file.new",
        json!({"width":64,"height":48,"bit_depth":depth,"working_space":"display_p3"}),
    );
    run(
        &mut e,
        "type.create",
        json!({"name":"Type","spec":{"text":"中Ab","font":"sans","font_size":16,"layout":{"type":"point"}},"color":[0.2,0.4,0.8,1],"x":-4,"y":22}),
    );
    e
}
fn samples(e: &Editor) -> Samples {
    let layer = e.document().unwrap().layers().last().unwrap();
    let mut result = Samples::new();
    for (coord, tile) in layer.pixels().tiles() {
        let tile = tile.unwrap();
        let [ox, oy] = coord.origin();
        for i in 0..256 * 256 {
            let p = match &*tile {
                PixelBuffer::Rgba16(v) => [v[i * 4], v[i * 4 + 1], v[i * 4 + 2], v[i * 4 + 3]],
                PixelBuffer::Rgba8(v) => std::array::from_fn(|c| u16::from(v[i * 4 + c])),
            };
            if p != [0; 4] {
                result.insert(
                    (
                        ox + (i % 256) as i64 + layer.info().x,
                        oy + (i / 256) as i64 + layer.info().y,
                    ),
                    p,
                );
            }
        }
    }
    result
}
fn archive(e: &Editor) -> strata_io::project::ProjectArchive {
    e.document().unwrap().to_archive().unwrap()
}
fn transform(e: &mut Editor, geometry: Value, interpolation: &str) -> Value {
    run(
        e,
        "transform.apply",
        json!({"layer":{"name":"Type"},"scope":"whole_layer","geometry":geometry,"interpolation":interpolation}),
    )
}
fn definition(e: &mut Editor) -> Value {
    run(e, "type.inspect", json!({"layer":{"name":"Type"}}))["definition"].clone()
}
fn update(e: &mut Editor, text: &str, color: Value) -> Value {
    let mut spec = definition(e)["spec"].clone();
    spec["text"] = json!(text);
    run(
        e,
        "type.update",
        json!({"layer":{"name":"Type"},"spec":spec,"color":color}),
    )
}
#[test]
fn text_quarter_turn_and_flip_are_independent_exact_lattice_maps_at_both_depths() {
    for depth in [8, 16] {
        let mut e = fixture(depth);
        let before = archive(&e);
        let source = samples(&e);
        transform(
            &mut e,
            json!({"type":"affine","values":[0,1,-1,0,64,0]}),
            "nearest",
        );
        let expected: Samples = source
            .into_iter()
            .map(|((x, y), p)| ((63 - y, x), p))
            .collect();
        assert_eq!(samples(&e), expected);
        let retained = definition(&mut e);
        assert_eq!(retained["spec"]["text"], "中Ab");
        assert_eq!(retained["operations"].as_array().unwrap().len(), 1);
        let transformed = archive(&e);
        update(&mut e, "中Ab", json!([0.8, 0.1, 0.3, 1]));
        update(&mut e, "中Ab", json!([0.2, 0.4, 0.8, 1]));
        assert_eq!(
            samples(&e),
            expected,
            "re-render must keep geometry and original coverage"
        );
        run(&mut e, "edit.undo", json!({}));
        run(&mut e, "edit.undo", json!({}));
        assert!(archive(&e) == transformed);
        run(
            &mut e,
            "image.orient",
            json!({"operation":"flip_horizontal"}),
        );
        let flipped: Samples = expected
            .into_iter()
            .map(|((x, y), p)| ((63 - x, y), p))
            .collect();
        assert_eq!(samples(&e), flipped);
        run(&mut e, "edit.undo", json!({}));
        run(&mut e, "edit.undo", json!({}));
        assert!(archive(&e) == before);
    }
}
#[test]
fn projective_text_coverage_matches_an_independent_inverse_and_bilinear_reference() {
    let mut e = fixture(16);
    let source = samples(&e);
    transform(
        &mut e,
        json!({"type":"matrix","values":[1,0,0,0,1,0,0.004,0,1]}),
        "bilinear",
    );
    let actual = samples(&e);
    let mut nonzero = 0;
    for y in -5..45 {
        for x in -10..50 {
            let dx = x as f64 + 0.5;
            let dy = y as f64 + 0.5;
            let sx = dx / (1. - 0.004 * dx) - 0.5;
            let sy = dy / (1. - 0.004 * dx) - 0.5;
            let ix = sx.floor() as i64;
            let iy = sy.floor() as i64;
            let fx = sx - ix as f64;
            let fy = sy - iy as f64;
            let alpha = [
                ((ix, iy), (1. - fx) * (1. - fy)),
                ((ix + 1, iy), fx * (1. - fy)),
                ((ix, iy + 1), (1. - fx) * fy),
                ((ix + 1, iy + 1), fx * fy),
            ]
            .into_iter()
            .map(|(p, w)| f64::from(source.get(&p).copied().unwrap_or([0; 4])[3]) * w)
            .sum::<f64>()
            .round() as u16;
            let p = actual.get(&(x, y)).copied().unwrap_or([0; 4]);
            assert!(p[3].abs_diff(alpha) <= 1, "{x},{y}: {} != {alpha}", p[3]);
            if p[3] > 0 {
                assert_eq!(&p[..3], &[13107, 26214, 52428]);
                nonzero += 1;
            }
        }
    }
    assert!(nonzero > 100);
}
#[test]
fn nearest_and_area_image_resize_use_actual_different_coverage_and_keep_source() {
    for interpolation in ["nearest", "area", "bilinear"] {
        let mut e = fixture(16);
        let before = archive(&e);
        let source = samples(&e);
        run(
            &mut e,
            "image.resize",
            json!({"width":32,"height":24,"interpolation":interpolation}),
        );
        let actual = samples(&e);
        let mut visible = 0;
        for y in -5..25 {
            for x in -8..35 {
                let alpha = if interpolation == "nearest" {
                    source
                        .get(&(2 * x + 1, 2 * y + 1))
                        .copied()
                        .unwrap_or([0; 4])[3]
                } else {
                    ([
                        (2 * x, 2 * y),
                        (2 * x + 1, 2 * y),
                        (2 * x, 2 * y + 1),
                        (2 * x + 1, 2 * y + 1),
                    ]
                    .into_iter()
                    .map(|p| u32::from(source.get(&p).copied().unwrap_or([0; 4])[3]))
                    .sum::<u32>() as f64
                        / 4.)
                        .round() as u16
                };
                let p = actual.get(&(x, y)).copied().unwrap_or([0; 4]);
                assert!(
                    p[3].abs_diff(alpha) <= 1,
                    "{interpolation} {x},{y}:{} !={alpha}",
                    p[3]
                );
                visible += usize::from(p[3] > 0);
            }
        }
        assert!(visible > 30);
        assert_eq!(definition(&mut e)["spec"]["text"], "中Ab");
        let old = samples(&e);
        update(&mut e, "中Ab", json!([0.9, 0.1, 0.2, 1]));
        update(&mut e, "中Ab", json!([0.2, 0.4, 0.8, 1]));
        assert_eq!(samples(&e), old);
        for _ in 0..3 {
            run(&mut e, "edit.undo", json!({}));
        }
        assert!(archive(&e) == before);
    }
}
#[test]
fn crop_barrier_survives_later_orientation_edit_and_real_project_reopen() {
    let mut e = fixture(16);
    update(&mut e, "中文中文中文", json!([0.2, 0.4, 0.8, 1]));
    run(
        &mut e,
        "image.rotate",
        json!({"angle_degrees":12,"interpolation":"bilinear"}),
    );
    run(
        &mut e,
        "image.crop",
        json!({"rect":{"x":8,"y":8,"width":32,"height":24},"pixels":"delete"}),
    );
    run(&mut e, "image.orient", json!({"operation":"rotate_90_cw"}));
    update(&mut e, "再编辑中文Long source", json!([0.8, 0.2, 0.1, 1]));
    let frame = e.snapshot().frame.unwrap();
    let pixels = samples(&e);
    assert!(pixels.len() > 50);
    assert!(
        pixels.keys().all(|&(x, y)| x >= frame.x
            && y >= frame.y
            && x < frame.x + i64::from(frame.width)
            && y < frame.y + i64::from(frame.height)),
        "deleted support regrew after source edit"
    );
    let current = archive(&e);
    assert_eq!(current.metadata["version"], 8);
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("retained.strata");
    let token = e.grant_write(&file, false, Source::User).unwrap();
    run(&mut e, "file.save_as", json!({"resource":token}));
    let mut reopened = Editor::new();
    let token = reopened.grant_read(&file, Source::User).unwrap();
    run(&mut reopened, "file.open", json!({"resource":token}));
    assert_eq!(archive(&reopened).metadata, current.metadata);
    assert!(archive(&reopened).tiles == current.tiles);
    assert_eq!(
        definition(&mut reopened)["spec"]["text"],
        "再编辑中文Long source"
    );
    update(&mut reopened, "新内容", json!([0.1, 0.6, 0.2, 1]));
    run(&mut reopened, "edit.undo", json!({}));
    assert_eq!(archive(&reopened).metadata, current.metadata);
    assert!(archive(&reopened).tiles == current.tiles);
}
#[test]
fn text_transform_identity_translation_selection_rejection_and_locks_are_atomic() {
    let mut e = fixture(16);
    let before = archive(&e);
    let revision = e.revision();
    let result = transform(&mut e, json!({"type":"components"}), "bilinear");
    assert_eq!(result["changed"], false);
    assert_eq!(e.revision(), revision);
    assert!(archive(&e) == before);
    let source = samples(&e);
    transform(
        &mut e,
        json!({"type":"components","translation":[-300,270]}),
        "bilinear",
    );
    assert_eq!(
        samples(&e),
        source
            .into_iter()
            .map(|((x, y), p)| ((x - 300, y + 270), p))
            .collect()
    );
    assert!(definition(&mut e).get("operations").is_none());
    run(&mut e, "edit.undo", json!({}));
    run(&mut e, "select.all", json!({}));
    let selected = archive(&e);
    assert!(e.execute(e.request(Source::User),"transform.apply",json!({"layer":{"name":"Type"},"scope":"selection","geometry":{"type":"components","translation":[2,3]}}),|_|Ok(())).is_err());
    assert!(archive(&e) == selected);
    run(
        &mut e,
        "layer.set_locks",
        json!({"layer":{"name":"Type"},"locks":{"position":true}}),
    );
    let locked = archive(&e);
    assert!(e.execute(e.request(Source::User),"transform.apply",json!({"layer":{"name":"Type"},"scope":"whole_layer","geometry":{"type":"components","rotation_degrees":12}}),|_|Ok(())).is_err());
    assert!(archive(&e) == locked);
}
#[test]
fn retained_geometry_manifest_and_new_source_horizon_fail_without_losing_truth() {
    let mut e = fixture(16);
    let plain = archive(&e);
    let mut legacy = plain.clone();
    legacy.metadata["version"] = json!(7);
    assert!(Document::from_archive(legacy).is_ok());
    transform(
        &mut e,
        json!({"type":"matrix","values":[1,0,0,0,1,0,-0.02,0,1]}),
        "nearest",
    );
    let before = archive(&e);
    let mut invalid = before.clone();
    invalid.metadata["version"] = json!(7);
    assert!(Document::from_archive(invalid).is_err());
    invalid = before.clone();
    invalid.metadata["layers"][1]["kind"]["text"]["operations"][0]["matrix"] =
        json!([0, 0, 0, 0, 0, 0, 0, 0, 0]);
    assert!(Document::from_archive(invalid).is_err());
    let mut spec = definition(&mut e)["spec"].clone();
    spec["text"] = json!("AAAAAAAAAAAAAAAAAAAA");
    assert!(
        e.execute(
            e.request(Source::User),
            "type.update",
            json!({"layer":{"name":"Type"},"spec":spec,"color":[0.2,0.4,0.8,1]}),
            |_| Ok(())
        )
        .is_err()
    );
    assert!(archive(&e) == before);
    assert!(matches!(
        e.document().unwrap().layers().last().unwrap().info().kind,
        LayerKind::Text { .. }
    ));
    run(&mut e, "edit.undo", json!({}));
    assert!(archive(&e) == plain);
}

#[test]
fn text_styles_and_disabled_soft_mask_follow_exact_orientation_and_survive_reedit() {
    let mut e = fixture(16);
    run(
        &mut e,
        "select.marquee",
        json!({"x":1,"y":6,"width":30,"height":20}),
    );
    run(&mut e, "select.feather", json!({"radius":1}));
    run(
        &mut e,
        "layer.mask.add",
        json!({"layer":{"name":"Type"},"source":"from_selection"}),
    );
    run(
        &mut e,
        "layer.mask.set_enabled",
        json!({"layer":{"name":"Type"},"enabled":false}),
    );
    run(
        &mut e,
        "layer.set_styles",
        json!({"layer":{"name":"Type"},"styles":{"gradient_overlay":{"start":[1000,3000,5000,65535],"end":[50001,40003,30005,65535],"angle":23,"scale":1,"opacity":0.3},"drop_shadow":{"color":[0,0,0,32769],"offset_x":2,"offset_y":3,"blur":1},"stroke":{"color":[50001,20003,10005,40001],"size":1,"position":"outside"}}}),
    );
    let old = e.clone();
    let before = archive(&e);
    let PixelBuffer::Rgba16(source) = e.document().unwrap().export_pixels().unwrap() else {
        panic!()
    };
    run(&mut e, "image.orient", json!({"operation":"rotate_90_cw"}));
    let PixelBuffer::Rgba16(actual) = e.document().unwrap().export_pixels().unwrap() else {
        panic!()
    };
    let mut checked = 0;
    for y in 0..48usize {
        for x in 0..64usize {
            for c in 0..4 {
                let a = actual[((x * 48) + (47 - y)) * 4 + c];
                let b = source[(y * 64 + x) * 4 + c];
                assert!(a.abs_diff(b) <= 8, "style rotation {x},{y},{c}: {a}!={b}");
                checked += usize::from(a > 0);
            }
        }
    }
    assert!(checked > 300);
    let mask_sample = |editor: &Editor, x, y| {
        let layer = editor.document().unwrap().layers().last().unwrap();
        let meta = layer.info().mask.as_ref().unwrap();
        assert!(!meta.enabled);
        layer
            .mask()
            .unwrap()
            .sample(
                x + meta.anchor_x - layer.info().x,
                y + meta.anchor_y - layer.info().y,
            )
            .unwrap()
    };
    for y in 0..48 {
        for x in 0..64 {
            assert_eq!(mask_sample(&e, 47 - y, x), mask_sample(&old, x, y));
        }
    }
    let oriented = archive(&e);
    update(&mut e, "改字", json!([0.2, 0.4, 0.8, 1]));
    assert_eq!(
        archive(&e).metadata["layers"][1]["mask"],
        oriented.metadata["layers"][1]["mask"]
    );
    run(&mut e, "edit.undo", json!({}));
    assert!(archive(&e) == oriented);
    run(&mut e, "edit.undo", json!({}));
    assert!(archive(&e) == before);
}

#[test]
fn excessive_retained_metadata_and_failed_publication_never_replace_text() {
    let mut e = fixture(16);
    let before = archive(&e);
    let mut invalid = before.clone();
    invalid.metadata["version"] = json!(8);
    invalid.metadata["layers"][1]["kind"]["text"]["operations"] = Value::Array(vec![
        json!({"type":"crop","bounds":{"x":0,"y":0,"width":64,"height":48}});
        33
    ]);
    assert!(Document::from_archive(invalid).is_err());
    for geometry in [
        json!({"type":"components","scale":[10000,10000]}),
        json!({"type":"matrix","values":[1,0,0,0,1,0,-0.1,0,1]}),
    ] {
        assert!(
            e.execute(
                e.request(Source::User),
                "transform.apply",
                json!({"layer":{"name":"Type"},"scope":"whole_layer","geometry":geometry}),
                |_| Ok(())
            )
            .is_err()
        );
        assert!(archive(&e) == before);
    }
    assert!(e.execute(e.request(Source::User),"transform.apply",json!({"layer":{"name":"Type"},"scope":"whole_layer","geometry":{"type":"components","rotation_degrees":15}}),|_|Err("failed native scene publication".into())).is_err());
    assert!(archive(&e) == before);
}

#[test]
fn v7_local_crop_migrates_to_an_ordered_barrier_before_geometry() {
    let mut e = fixture(16);
    run(
        &mut e,
        "image.crop",
        json!({"rect":{"x":8,"y":8,"width":24,"height":16},"pixels":"delete"}),
    );
    let mut legacy = archive(&e);
    legacy.metadata["version"] = json!(7);
    let bounds = legacy.metadata["layers"][1]["kind"]["text"]["operations"][0]["bounds"].clone();
    legacy.metadata["layers"][1]["kind"]["text"]
        .as_object_mut()
        .unwrap()
        .remove("operations");
    legacy.metadata["layers"][1]["generated_crop"] = json!({"bounds":bounds});
    assert!(Document::from_archive(legacy.clone()).is_ok());
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("legacy7.strata");
    strata_io::project::write_project(&path, &legacy, false, false).unwrap();
    let mut opened = Editor::new();
    let token = opened.grant_read(&path, Source::User).unwrap();
    run(&mut opened, "file.open", json!({"resource":token}));
    let old = samples(&opened);
    transform(
        &mut opened,
        json!({"type":"affine","values":[0,1,-1,0,64,0]}),
        "nearest",
    );
    assert_eq!(
        samples(&opened),
        old.into_iter()
            .map(|((x, y), p)| ((63 - y, x), p))
            .collect()
    );
    assert_eq!(archive(&opened).metadata["version"], 8);
    update(&mut opened, "编辑中文ABC", json!([0.2, 0.4, 0.8, 1]));
    let shown = samples(&opened);
    assert!(!shown.is_empty());
    assert!(
        shown
            .keys()
            .all(|&(x, y)| (40..56).contains(&x) && (8..32).contains(&y)),
        "legacy crop support regrew after geometry"
    );
}

#[test]
fn transparent_black_text_has_readonly_geometry_and_appears_transformed_after_alpha_edit() {
    for depth in [8, 16] {
        let mut e = fixture(depth);
        update(&mut e, "中Ab", json!([0, 0, 0, 0]));
        assert!(samples(&e).is_empty());
        let before = archive(&e);
        let revision = e.revision();
        let bounds = run(
            &mut e,
            "layer.pixel_bounds",
            json!({"layer":{"name":"Type"}}),
        );
        assert!(bounds["bounds"]["width"].as_u64().unwrap() > 0);
        assert_eq!(e.revision(), revision);
        assert!(archive(&e) == before);
        let owner = e
            .begin_canvas_transform(
                strata_core::LayerRef::Name("Type".into()),
                strata_core::commands::TransformScope::WholeLayer,
            )
            .unwrap();
        assert!(e.canvas_transform_bounds(&owner).unwrap().bounds.is_some());
        let result = transform(
            &mut e,
            json!({"type":"affine","values":[0,1,-1,0,64,0]}),
            "nearest",
        );
        assert_eq!(result["changed"], true);
        assert!(samples(&e).is_empty());
        update(&mut e, "中Ab", json!([0.2, 0.4, 0.8, 1]));
        let reference = samples(&fixture(depth));
        let expected: Samples = reference
            .into_iter()
            .map(|((x, y), p)| ((63 - y, x), p))
            .collect();
        assert_eq!(samples(&e), expected);
        run(&mut e, "edit.undo", json!({}));
        run(&mut e, "edit.undo", json!({}));
        assert!(archive(&e) == before);
    }
}
