use serde_json::{Value, json};
use strata_core::{Document, Editor, LayerKind, Source};
use strata_pixels::PixelValue;
fn run(e: &mut Editor, id: &str, p: Value) -> Value {
    e.execute(e.request(Source::User), id, p, |_| Ok(()))
        .unwrap()
}
fn archive(e: &Editor) -> strata_io::project::ProjectArchive {
    e.document().unwrap().to_archive().unwrap()
}
fn raw(e: &Editor, name: &str, x: i64, y: i64) -> [u16; 4] {
    let l = e
        .document()
        .unwrap()
        .layers()
        .iter()
        .find(|l| l.info().name == name)
        .unwrap();
    match l.pixels().pixel(x - l.info().x, y - l.info().y).unwrap() {
        PixelValue::Rgba8(v) => v.map(u16::from),
        PixelValue::Rgba16(v) => v,
    }
}
fn fixture(depth: u8) -> (Editor, Value) {
    let mut e = Editor::new();
    run(
        &mut e,
        "file.new",
        json!({"width":64,"height":48,"bit_depth":depth,"working_space":"display_p3"}),
    );
    let g = run(&mut e, "layer.create_group", json!({"name":"G"}))["layer_id"].clone();
    run(&mut e, "layer.create", json!({"name":"A"}));
    run(
        &mut e,
        "select.marquee",
        json!({"x":3,"y":4,"width":10,"height":8}),
    );
    run(
        &mut e,
        "paint.fill",
        json!({"layer":{"name":"A"},"color":[0.2,0.4,0.8,0.7]}),
    );
    run(
        &mut e,
        "layer.mask.add",
        json!({"layer":{"name":"A"},"source":"from_selection"}),
    );
    run(
        &mut e,
        "layer.mask.set_enabled",
        json!({"layer":{"name":"A"},"enabled":false}),
    );
    run(&mut e, "select.none", json!({}));
    run(&mut e, "layer.select", json!({"layer":{"id":g}}));
    run(&mut e, "layer.create_group", json!({"name":"Inner"}));
    run(
        &mut e,
        "type.create",
        json!({"name":"Type","spec":{"text":"Ab 中","font":"sans","font_size":16,"layout":{"type":"point"}},"color":[0.7,0.2,0.1,1],"x":20,"y":30}),
    );
    run(&mut e, "layer.create", json!({"name":"Hidden"}));
    run(
        &mut e,
        "select.marquee",
        json!({"x":0,"y":0,"width":5,"height":6}),
    );
    run(
        &mut e,
        "paint.fill",
        json!({"layer":{"name":"Hidden"},"color":[0.8,0.2,0.4,0.5]}),
    );
    run(&mut e, "select.none", json!({}));
    run(
        &mut e,
        "layer.set_position",
        json!({"layer":{"name":"Hidden"},"x":-20,"y":2}),
    );
    run(
        &mut e,
        "layer.set_visibility",
        json!({"layer":{"name":"Hidden"},"visible":false}),
    );
    run(&mut e, "layer.select", json!({"layer":{"id":g}}));
    run(
        &mut e,
        "layer.mask.add",
        json!({"layer":{"id":g},"source":"white"}),
    );
    (e, g)
}
fn transform(e: &mut Editor, g: &Value, geometry: Value) -> Value {
    run(
        e,
        "transform.apply",
        json!({"layer":{"id":g},"scope":"whole_layer","geometry":geometry,"interpolation":"nearest"}),
    )
}
#[test]
fn group_integer_translation_keeps_every_source_and_mask_version_and_has_one_exact_undo() {
    for depth in [8, 16] {
        let (mut e, g) = fixture(depth);
        let before = archive(&e);
        let old = e.clone();
        let rev = e.revision();
        let noop = transform(&mut e, &g, json!({"type":"components"}));
        assert_eq!(noop["changed"], false);
        assert_eq!(e.revision(), rev);
        assert!(archive(&e) == before);
        let result = transform(
            &mut e,
            &g,
            json!({"type":"components","translation":[70,-9]}),
        );
        assert_eq!(result["changed"], true);
        assert_eq!(archive(&e).tiles, before.tiles);
        assert_eq!(raw(&e, "A", 75, -3), raw(&old, "A", 5, 6));
        for (a, b) in old
            .document()
            .unwrap()
            .layers()
            .iter()
            .skip(1)
            .zip(e.document().unwrap().layers().iter().skip(1))
        {
            assert_eq!(b.info().x, a.info().x + 70);
            assert_eq!(b.info().y, a.info().y - 9);
            assert_eq!(b.info().mask, a.info().mask);
            for c in a.pixels().tile_coords() {
                assert_eq!(a.pixels().tile_version(*c), b.pixels().tile_version(*c));
            }
        }
        run(&mut e, "edit.undo", json!({}));
        assert!(archive(&e) == before);
    }
}
#[test]
fn nested_group_quarter_turn_matches_independent_pixel_and_mask_centers_and_keeps_text_editable() {
    for depth in [8, 16] {
        let (mut e, g) = fixture(depth);
        let old = e.clone();
        let before = archive(&e);
        let h = e.snapshot().history_cursor;
        transform(
            &mut e,
            &g,
            json!({"type":"affine","values":[0,1,-1,0,64,0]}),
        );
        assert_eq!(e.snapshot().history_cursor, h + 1);
        for name in ["A", "Hidden"] {
            for y in -4..36 {
                for x in -24..48 {
                    assert_eq!(
                        raw(&e, name, 63 - y, x),
                        raw(&old, name, x, y),
                        "{name} {x},{y}"
                    );
                }
            }
        }
        let sample_mask = |editor: &Editor, name: &str, x, y| {
            let l = editor
                .document()
                .unwrap()
                .layers()
                .iter()
                .find(|l| l.info().name == name)
                .unwrap();
            let m = l.info().mask.as_ref().unwrap();
            l.mask()
                .unwrap()
                .sample(x + m.anchor_x - l.info().x, y + m.anchor_y - l.info().y)
                .unwrap()
        };
        for name in ["G", "A"] {
            for y in 0..48 {
                for x in 0..64 {
                    assert_eq!(
                        sample_mask(&e, name, 63 - y, x),
                        sample_mask(&old, name, x, y)
                    );
                }
            }
        }
        assert!(matches!(
            e.document()
                .unwrap()
                .layers()
                .iter()
                .find(|l| l.info().name == "Type")
                .unwrap()
                .info()
                .kind,
            LayerKind::Text { .. }
        ));
        let definition =
            run(&mut e, "type.inspect", json!({"layer":{"name":"Type"}}))["definition"].clone();
        let ops = definition["operations"].clone();
        let mut spec = definition["spec"].clone();
        spec["text"] = json!("改字 Serif");
        run(
            &mut e,
            "type.update",
            json!({"layer":{"name":"Type"},"spec":spec,"color":[0.1,0.5,0.8,1]}),
        );
        assert_eq!(
            run(&mut e, "type.inspect", json!({"layer":{"name":"Type"}}))["definition"]["operations"],
            ops
        );
        let current = archive(&e);
        assert!(
            Document::from_archive(current.clone())
                .unwrap()
                .to_archive()
                .unwrap()
                == current
        );
        run(&mut e, "edit.undo", json!({}));
        run(&mut e, "edit.undo", json!({}));
        assert!(archive(&e) == before);
    }
}
#[test]
fn group_bounds_and_result_never_disclose_hidden_child_extent_and_query_keeps_new_selection() {
    let (mut e, g) = fixture(16);
    let bounds = run(&mut e, "layer.pixel_bounds", json!({"layer":{"id":g}}))["bounds"].clone();
    assert!(bounds["x"].as_i64().unwrap() >= 0);
    run(
        &mut e,
        "layer.set_position",
        json!({"layer":{"name":"Hidden"},"x":-50000,"y":50000}),
    );
    assert_eq!(
        run(&mut e, "layer.pixel_bounds", json!({"layer":{"id":g}}))["bounds"],
        bounds
    );
    let prepared = e
        .prepare(
            e.request(Source::User),
            "layer.pixel_bounds",
            json!({"layer":{"id":g}}),
        )
        .unwrap();
    run(
        &mut e,
        "layer.select",
        json!({"layer":{"name":"Background"}}),
    );
    let active = e.snapshot().active_layer;
    let before = archive(&e);
    e.commit(prepared).unwrap();
    assert_eq!(e.snapshot().active_layer, active);
    assert!(archive(&e) == before);
    let result = transform(&mut e, &g, json!({"type":"components","translation":[1,2]}));
    assert_eq!(
        result["output_bounds"]["x"],
        bounds["x"].as_i64().unwrap() + 1
    );
    assert_eq!(
        result["output_bounds"]["y"],
        bounds["y"].as_i64().unwrap() + 2
    );
}
#[test]
fn groups_accept_fills_and_styles_and_reject_locked_children_or_failed_publication() {
    for change in ["fill", "style", "locked"] {
        let (mut e, g) = fixture(16);
        match change {
            "fill" => {
                run(&mut e, "layer.select", json!({"layer":{"id":g}}));
                run(
                    &mut e,
                    "layer.fill.solid",
                    json!({"name":"Editable fill","color":[65535,0,0,65535]}),
                );
            }
            "style" => {
                run(
                    &mut e,
                    "layer.set_styles",
                    json!({"layer":{"name":"A"},"styles":{"stroke":{"color":[65535,0,0,65535],"size":1,"position":"outside"}}}),
                );
            }
            _ => {
                run(
                    &mut e,
                    "layer.set_locks",
                    json!({"layer":{"name":"Hidden"},"locks":{"pixels":true}}),
                );
            }
        };
        let before = archive(&e);
        let result = e.execute(e.request(Source::User),"transform.apply",json!({"layer":{"id":g},"scope":"whole_layer","geometry":{"type":"components","rotation_degrees":12}}),|_|Ok(()));
        if change == "locked" {
            assert!(result.is_err());
        } else {
            assert_eq!(result.unwrap()["changed"], true);
            run(&mut e, "edit.undo", json!({}));
        }
        assert!(archive(&e) == before);
    }
    let (mut e, g) = fixture(16);
    run(&mut e, "select.all", json!({}));
    let before = archive(&e);
    assert!(e.execute(e.request(Source::User),"transform.apply",json!({"layer":{"id":g},"scope":"selection","geometry":{"type":"components","translation":[2,3]}}),|_|Ok(())).is_err());
    assert!(archive(&e) == before);
    assert!(e.execute(e.request(Source::User),"transform.apply",json!({"layer":{"id":g},"scope":"whole_layer","geometry":{"type":"components","rotation_degrees":12}}),|_|Err("display failed".into())).is_err());
    assert!(archive(&e) == before);
}

#[test]
fn owned_multi_layer_transform_carries_nested_selection_once_and_rejects_target_swap() {
    use strata_core::LayerRef;
    let (mut e, group) = fixture(16);
    run(
        &mut e,
        "layer.select",
        json!({"layer":{"name":"Background"}}),
    );
    let fill = run(
        &mut e,
        "layer.fill.solid",
        json!({"name":"New fill","color":[10000,30000,50000,65535]}),
    )["layer_id"]
        .clone();
    let refs = json!([{"id":group}, {"name":"A"}, {"id":fill}]);
    let params = json!({"layers":refs,"geometry":{"type":"components","translation":[7,-3]}});
    let layers: Vec<LayerRef> = serde_json::from_value(refs).unwrap();
    let before = archive(&e);
    let history = e.snapshot().history_cursor;
    let owned = e.begin_canvas_transform_layers(&layers).unwrap();
    assert!(e.canvas_transform_bounds(&owned).unwrap().bounds.is_some());
    assert!(e.prepare_canvas_transform(&owned, json!({
        "layers":[{"id":group},{"name":"A"}],"geometry":{"type":"components","translation":[7,-3]}
    })).is_err());
    assert_eq!(archive(&e), before);
    let result = run(&mut e, "transform.apply_layers", params);
    assert_eq!(result["layer_ids"].as_array().unwrap().len(), 2);
    assert_eq!(e.snapshot().history_cursor, history + 1);
    let previous = before.metadata["layers"].as_array().unwrap();
    for layer in &e.snapshot().layers {
        if ["A", "G", "New fill"].contains(&layer.name.as_str()) {
            let old = previous
                .iter()
                .find(|entry| entry["id"] == json!(layer.id))
                .unwrap();
            assert_eq!(layer.x, old["x"].as_i64().unwrap() + 7);
            assert_eq!(layer.y, old["y"].as_i64().unwrap() - 3);
        }
    }
    run(&mut e, "edit.undo", json!({}));
    assert_eq!(archive(&e), before);
}

#[test]
fn affine_group_keeps_shared_source_space_and_u16_mask_coverage_against_independent_weights() {
    let (mut e, g) = fixture(16);
    let old = e.clone();
    let source_alpha = raw(&old, "A", 5, 6)[3];
    run(
        &mut e,
        "transform.apply",
        json!({"layer":{"id":g},"scope":"whole_layer","geometry":{"type":"affine","values":[1.5,0,0,0.5,2.25,-1.75]},"interpolation":"bilinear"}),
    );
    let weight = |x: i64, y: i64, inside: &dyn Fn(i64, i64) -> bool| {
        let sx = (x as f64 + 0.5 - 2.25) / 1.5 - 0.5;
        let sy = (y as f64 + 0.5 + 1.75) / 0.5 - 0.5;
        let ix = sx.floor() as i64;
        let iy = sy.floor() as i64;
        let fx = sx - ix as f64;
        let fy = sy - iy as f64;
        [
            ((ix, iy), (1. - fx) * (1. - fy)),
            ((ix + 1, iy), fx * (1. - fy)),
            ((ix, iy + 1), (1. - fx) * fy),
            ((ix + 1, iy + 1), fx * fy),
        ]
        .into_iter()
        .filter(|(p, _)| inside(p.0, p.1))
        .map(|(_, w)| w)
        .sum::<f64>()
    };
    let group = e
        .document()
        .unwrap()
        .layers()
        .iter()
        .find(|l| l.info().name == "G")
        .unwrap();
    let meta = group.info().mask.as_ref().unwrap();
    for y in -4..28 {
        for x in -5..104 {
            let alpha = (f64::from(source_alpha)
                * weight(x, y, &|x, y| (3..13).contains(&x) && (4..12).contains(&y)))
            .round() as u16;
            assert!(
                raw(&e, "A", x, y)[3].abs_diff(alpha) <= 1,
                "child alpha {x},{y}"
            );
            let expected = (65535.
                * weight(x, y, &|x, y| (0..64).contains(&x) && (0..48).contains(&y)))
            .round() as u16;
            let mask = group
                .mask()
                .unwrap()
                .sample(
                    x + meta.anchor_x - group.info().x,
                    y + meta.anchor_y - group.info().y,
                )
                .unwrap();
            assert!(
                mask.abs_diff(expected) <= 1,
                "group mask {x},{y}: {mask}!={expected}"
            );
        }
    }
    assert!(e.document().unwrap().selection().is_none());
}
#[test]
fn empty_editable_text_inside_group_inherits_geometry_before_later_typing() {
    let (mut e, g) = fixture(16);
    run(&mut e, "layer.select", json!({"layer":{"id":g}}));
    run(
        &mut e,
        "type.create",
        json!({"name":"BlankType","spec":{"text":" ","font":"sans","font_size":16,"layout":{"type":"point"}},"color":[0.2,0.4,0.8,1],"x":10,"y":20}),
    );
    transform(
        &mut e,
        &g,
        json!({"type":"affine","values":[0,1,-1,0,64,0]}),
    );
    let definition = run(
        &mut e,
        "type.inspect",
        json!({"layer":{"name":"BlankType"}}),
    )["definition"]
        .clone();
    assert!(!definition["operations"].as_array().unwrap().is_empty());
    let mut spec = definition["spec"].clone();
    spec["text"] = json!("Ab");
    run(
        &mut e,
        "type.update",
        json!({"layer":{"name":"BlankType"},"spec":spec,"color":[0.2,0.4,0.8,1]}),
    );
    let mut reference = Editor::new();
    run(
        &mut reference,
        "file.new",
        json!({"width":64,"height":48,"bit_depth":16,"working_space":"display_p3"}),
    );
    run(
        &mut reference,
        "type.create",
        json!({"name":"BlankType","spec":{"text":"Ab","font":"sans","font_size":16,"layout":{"type":"point"}},"color":[0.2,0.4,0.8,1],"x":10,"y":20}),
    );
    run(
        &mut reference,
        "transform.apply",
        json!({"layer":{"name":"BlankType"},"scope":"whole_layer","geometry":{"type":"affine","values":[0,1,-1,0,64,0]},"interpolation":"nearest"}),
    );
    let mut visible = 0;
    for y in -8..64 {
        for x in -8..80 {
            let p = raw(&e, "BlankType", x, y);
            assert_eq!(p, raw(&reference, "BlankType", x, y));
            visible += usize::from(p[3] > 0);
        }
    }
    assert!(visible > 30);
}

#[test]
fn aggregate_group_mask_budget_is_preflighted_before_any_source_changes() {
    let mut e = Editor::new();
    run(
        &mut e,
        "file.new",
        json!({"width":5000,"height":5000,"bit_depth":16}),
    );
    let g = run(&mut e, "layer.create_group", json!({"name":"G"}))["layer_id"].clone();
    run(
        &mut e,
        "layer.mask.add",
        json!({"layer":{"id":g},"source":"white"}),
    );
    run(&mut e, "layer.create", json!({"name":"A"}));
    run(
        &mut e,
        "select.marquee",
        json!({"x":2,"y":2,"width":4,"height":5}),
    );
    run(
        &mut e,
        "paint.fill",
        json!({"layer":{"name":"A"},"color":[0.2,0.4,0.8,1]}),
    );
    run(&mut e, "select.none", json!({}));
    run(
        &mut e,
        "layer.mask.add",
        json!({"layer":{"name":"A"},"source":"white"}),
    );
    run(&mut e, "layer.create", json!({"name":"Empty"}));
    run(
        &mut e,
        "layer.mask.add",
        json!({"layer":{"name":"Empty"},"source":"white"}),
    );
    let revision = e.revision();
    let identity = transform(&mut e, &g, json!({"type":"components"}));
    assert_eq!(identity["changed"], false);
    assert_eq!(e.revision(), revision);
    let original = archive(&e);
    transform(&mut e, &g, json!({"type":"components","translation":[1,2]}));
    assert_eq!(archive(&e).tiles, original.tiles);
    run(&mut e, "edit.undo", json!({}));
    assert!(archive(&e) == original);
    let before = archive(&e);
    let stats = e.document().unwrap().memory_pool().stats();
    assert!(e.execute(e.request(Source::User),"transform.apply",json!({"layer":{"id":g},"scope":"whole_layer","geometry":{"type":"components","scale":[1.01,1]}}),|_|Ok(())).is_err());
    assert!(archive(&e) == before);
    assert_eq!(
        e.document().unwrap().memory_pool().stats().transient_bytes,
        stats.transient_bytes
    );
}

#[test]
fn runtime_group_cancellation_during_shared_resampling_keeps_original_tree_and_releases_work() {
    use strata_pixels::memory::{MemoryLimits, MemoryPool};
    let pool = MemoryPool::new(MemoryLimits {
        cpu_bytes: 256 * 1024 * 1024,
        gpu_bytes: 0,
        disk_bytes: 128 * 1024 * 1024,
    });
    let mut e = Editor::with_memory_pool(pool.clone());
    run(
        &mut e,
        "file.new",
        json!({"width":1024,"height":1024,"bit_depth":16}),
    );
    let g = run(&mut e, "layer.create_group", json!({"name":"G"}))["layer_id"].clone();
    for name in ["A", "B"] {
        run(
            &mut e,
            "layer.create",
            json!({"name":name,"fill":[10001,30003,50001,32769]}),
        );
    }
    let before = archive(&e);
    let request = e.request(Source::User);
    let worker = e.clone();
    let thread = std::thread::spawn(move || {
        worker.prepare(request,"transform.apply",json!({"layer":{"id":g},"scope":"whole_layer","geometry":{"type":"components","rotation_degrees":13,"pivot":[512,512]},"interpolation":"bilinear"}))
    });
    let start = std::time::Instant::now();
    while pool.stats().transient_bytes < 8 * 1024 * 1024
        && start.elapsed() < std::time::Duration::from_secs(5)
    {
        std::thread::yield_now()
    }
    assert!(
        pool.stats().transient_bytes >= 8 * 1024 * 1024,
        "real sampler scratch not observed"
    );
    e.cancel_pending_work();
    assert!(thread.join().unwrap().is_err());
    assert!(archive(&e) == before);
    assert_eq!(pool.stats().transient_bytes, 0);
}
