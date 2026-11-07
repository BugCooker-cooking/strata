use serde_json::{Value, json};
use strata_core::{Document, Editor, LayerKind, Source};
use strata_pixels::memory::{MemoryLimits, MemoryPool};
fn run(e: &mut Editor, id: &str, p: Value) -> Value {
    e.execute(e.request(Source::User), id, p, |_| Ok(()))
        .unwrap()
}
fn editor(depth: u8, space: &str) -> Editor {
    let mut e = Editor::new();
    run(
        &mut e,
        "file.new",
        json!({"width":160,"height":120,"bit_depth":depth,"working_space":space}),
    );
    e
}
fn spec(text: &str) -> Value {
    json!({"text":text,"font":"sans","font_size":28,"line_height":1.3,"tracking":0.5,"align":"left","layout":{"type":"point"}})
}
fn create(e: &mut Editor, text: &str) -> Value {
    run(
        e,
        "type.create",
        json!({"name":"Type","spec":spec(text),"color":[0.2,0.4,0.8,0.7],"x":5,"y":40}),
    )
}
fn archive(e: &Editor) -> strata_io::project::ProjectArchive {
    e.document().unwrap().to_archive().unwrap()
}
#[test]
fn fonts_are_readonly_without_document_and_create_update_keep_source_depth_icc_and_exact_undo() {
    let mut blank = Editor::new();
    let fonts = run(&mut blank, "type.fonts", json!({}));
    assert_eq!(fonts["fonts"].as_array().unwrap().len(), 3);
    assert_eq!(fonts["system_fonts_exposed"], false);
    assert_eq!(blank.revision(), 0);
    assert!(blank.document().is_none());
    for depth in [8, 16] {
        for space in ["srgb", "display_p3"] {
            let mut e = editor(depth, space);
            let before = archive(&e);
            let result = create(&mut e, "Strata 中文");
            assert_eq!(result["changed"], true);
            assert_eq!(result["definition"]["spec"]["text"], "Strata 中文");
            assert_eq!(result["metrics"]["missing_glyphs"], 0);
            let id = e.snapshot().active_layer;
            assert!(matches!(
                e.document().unwrap().layers().last().unwrap().info().kind,
                LayerKind::Text { .. }
            ));
            assert_eq!(e.document().unwrap().bit_depth(), depth);
            assert_eq!(
                e.document().unwrap().working_icc(),
                Document::from_archive(before.clone())
                    .unwrap()
                    .working_icc()
            );
            let current = archive(&e);
            let query = run(&mut e, "type.inspect", json!({"layer":{"id":id}}));
            assert_eq!(query["definition"], result["definition"]);
            assert_eq!(query["metrics"], Value::Null);
            assert!(archive(&e) == current);
            let edited = run(
                &mut e,
                "type.update",
                json!({"layer":{"id":id},"spec":spec("重编辑 Latin"),"color":[0.9,0.3,0.1,1]}),
            );
            assert_eq!(edited["changed"], true);
            assert_eq!(e.snapshot().active_layer, id);
            assert_eq!(e.snapshot().history.len(), 2);
            let reopened = Document::from_archive(archive(&e)).unwrap();
            assert!(reopened.to_archive().unwrap() == archive(&e));
            run(&mut e, "edit.undo", json!({}));
            assert!(archive(&e) == current);
            run(&mut e, "edit.undo", json!({}));
            assert!(archive(&e) == before);
        }
    }
}
#[test]
fn glyph_rgb_codes_are_exact_at_both_depths_and_source_alpha_uses_only_coverage() {
    for depth in [8, 16] {
        let mut e = editor(depth, "display_p3");
        let result = create(&mut e, "中文 Aa");
        let color = result["definition"]["color"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_u64().unwrap() as u16)
            .collect::<Vec<_>>();
        let layer = e.document().unwrap().layers().last().unwrap();
        let mut samples = 0;
        let mut alphas = std::collections::BTreeSet::new();
        for (_, tile) in layer.pixels().tiles() {
            let tile = tile.unwrap();
            for i in 0..256 * 256 {
                let p = match &*tile {
                    strata_pixels::PixelBuffer::Rgba8(v) => {
                        std::array::from_fn::<_, 4, _>(|c| u16::from(v[i * 4 + c]))
                    }
                    strata_pixels::PixelBuffer::Rgba16(v) => std::array::from_fn(|c| v[i * 4 + c]),
                };
                if p.iter().all(|v| *v == 0) {
                    continue;
                };
                assert_eq!(&p[..3], &color[..3]);
                assert!(p[3] <= color[3]);
                alphas.insert(p[3]);
                samples += 1;
            }
        }
        assert!(samples > 100);
        assert!(alphas.len() > 20);
        assert_eq!(alphas.last(), Some(&color[3]));
    }
}
#[test]
fn same_spec_is_a_noop_and_keeps_newer_selection_while_locks_and_bad_inputs_are_atomic() {
    let mut e = editor(16, "srgb");
    let original = create(&mut e, "Hello 中文");
    let id = e.snapshot().active_layer;
    let request = e.request(Source::User);
    let candidate=e.prepare(request,"type.update",json!({"layer":{"id":id},"spec":original["definition"]["spec"],"color":[0.2,0.4,0.8,0.7]})).unwrap();
    let revision = e.revision();
    let cursor = e.snapshot().history_cursor;
    run(
        &mut e,
        "layer.select",
        json!({"layer":{"name":"Background"}}),
    );
    let active = e.snapshot().active_layer;
    assert_eq!(e.commit(candidate).unwrap()["changed"], false);
    assert_eq!(e.revision(), revision);
    assert_eq!(e.snapshot().history_cursor, cursor);
    assert_eq!(e.snapshot().active_layer, active);
    let p = json!({"layer":{"id":id},"spec":spec("changed"),"color":[0.,0.,0.,1]});
    for locks in [
        json!({"pixels":true}),
        json!({"transparency":true}),
        json!({"all":true}),
    ] {
        run(
            &mut e,
            "layer.set_locks",
            json!({"layer":{"id":id},"locks":locks}),
        );
        let before = archive(&e);
        assert!(
            e.execute(
                e.request(Source::User),
                "type.update",
                p.clone(),
                |_| Ok(())
            )
            .is_err()
        );
        assert!(archive(&e) == before);
        run(
            &mut e,
            "layer.set_locks",
            json!({"layer":{"id":id},"locks":{}}),
        );
    }
    for (field, value) in [
        ("font_size", json!(0)),
        ("line_height", json!(11)),
        ("font", json!("unknown-font")),
        ("tracking", json!(-50)),
    ] {
        let mut invalid = p.clone();
        invalid["spec"][field] = value;
        let before = archive(&e);
        assert!(
            e.execute(e.request(Source::User), "type.update", invalid, |_| Ok(()))
                .is_err()
        );
        assert!(archive(&e) == before);
    }
    let mut missing = p.clone();
    missing["spec"]["text"] = json!("\u{10ffff}");
    let before = archive(&e);
    assert!(
        e.execute(e.request(Source::User), "type.update", missing, |_| Ok(()))
            .is_err()
    );
    assert!(archive(&e) == before);
    assert!(
        e.execute(e.request(Source::User), "type.update", p, |_| Err(
            "failed display".into()
        ))
        .is_err()
    );
    assert!(archive(&e) == before);
}
#[test]
fn text_source_survives_real_save_recovery_rasterization_masks_styles_and_depth_conversion() {
    let mut e = editor(16, "srgb");
    create(&mut e, "中文 Test");
    let id = e.snapshot().active_layer;
    run(
        &mut e,
        "layer.mask.add",
        json!({"layer":{"id":id},"source":"white"}),
    );
    run(
        &mut e,
        "layer.set_styles",
        json!({"layer":{"id":id},"styles":{"stroke":{"color":[65535,10001,0,65535],"size":1,"position":"outside"},"drop_shadow":{"color":[0,0,0,65535],"offset_x":2,"offset_y":2,"blur":1}}}),
    );
    let before = archive(&e);
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("text.strata");
    let resource = e.grant_write(&file, false, Source::User).unwrap();
    run(&mut e, "file.save_as", json!({"resource":resource}));
    let mut opened = Editor::new();
    let resource = opened.grant_read(&file, Source::User).unwrap();
    run(&mut opened, "file.open", json!({"resource":resource}));
    assert!(archive(&opened) == before);
    assert_eq!(before.metadata["version"], 7);
    let store = strata_io::recovery::RecoveryStore::new(&dir.path().join("recovery")).unwrap();
    run(
        &mut opened,
        "layer.rename",
        json!({"layer":{"id":id},"name":"dirty text"}),
    );
    let info = opened.checkpoint_recovery(&store, "text").unwrap().unwrap();
    let saved = archive(&opened);
    let path = store.path(&info.session_id).unwrap();
    let mut recovered = Editor::new();
    let resource = recovered.grant_read(&path, Source::User).unwrap();
    run(&mut recovered, "file.recover", json!({"resource":resource}));
    assert!(archive(&recovered).tiles == saved.tiles);
    assert_eq!(
        archive(&recovered).metadata["layers"],
        saved.metadata["layers"]
    );
    let pixels = e.document().unwrap().export_pixels().unwrap();
    run(&mut e, "layer.rasterize", json!({"layer":{"id":id}}));
    assert!(matches!(
        e.document().unwrap().layers().last().unwrap().info().kind,
        LayerKind::Pixel
    ));
    let baked = e.document().unwrap().export_pixels().unwrap();
    match (pixels, baked) {
        (strata_pixels::PixelBuffer::Rgba16(a), strata_pixels::PixelBuffer::Rgba16(b)) => {
            assert!(a.iter().zip(b).all(|(a, b)| a.abs_diff(b) <= 3))
        }
        _ => panic!(),
    };
    run(&mut e, "edit.undo", json!({}));
    assert!(archive(&e) == before);
    run(&mut e, "image.convert_bit_depth", json!({"bit_depth":8}));
    let definition = run(&mut e, "type.inspect", json!({"layer":{"id":id}}))["definition"].clone();
    assert!(
        definition["color"]
            .as_array()
            .unwrap()
            .iter()
            .all(|v| v.as_u64().unwrap() <= 255)
    );
    assert_eq!(
        definition["spec"],
        before.metadata["layers"]
            .as_array()
            .unwrap()
            .last()
            .unwrap()["kind"]["text"]["spec"]
    );
    run(&mut e, "edit.undo", json!({}));
    assert!(archive(&e) == before);
}
#[test]
fn crop_keeps_text_editable_and_reedit_cannot_regrow_deleted_support() {
    let mut e = editor(16, "srgb");
    create(&mut e, "宽广的中文文字 Test");
    let id = e.snapshot().active_layer;
    let original = archive(&e);
    run(
        &mut e,
        "image.crop",
        json!({"rect":{"x":12,"y":10,"width":40,"height":50},"pixels":"delete"}),
    );
    let cropped = archive(&e);
    assert!(matches!(
        e.document().unwrap().layers().last().unwrap().info().kind,
        LayerKind::Text { .. }
    ));
    run(
        &mut e,
        "type.update",
        json!({"layer":{"id":id},"spec":spec("重排后的中文文字很长 ABC"),"color":[1.,0.2,0.1,1]}),
    );
    let layer = e.document().unwrap().layers().last().unwrap();
    for (_, tile) in layer.pixels().tiles() {
        assert!(tile.is_ok())
    }
    let bounds = strata_core::geometry_pixels::content_bounds(
        layer.pixels(),
        [layer.info().x, layer.info().y],
        &|| false,
    )
    .unwrap();
    if let Some(b) = bounds {
        assert!(
            b.x >= 12
                && b.y >= 10
                && b.x + i64::from(b.width) <= 52
                && b.y + i64::from(b.height) <= 60
        )
    }
    run(&mut e, "edit.undo", json!({}));
    assert!(archive(&e) == cropped);
    run(&mut e, "edit.undo", json!({}));
    assert!(archive(&e) == original);
}
#[test]
fn unsupported_text_geometry_rejects_and_old_manifests_cannot_claim_new_text() {
    let mut e = editor(16, "srgb");
    create(&mut e, "中文");
    let before = archive(&e);
    for (id, p) in [
        ("image.orient", json!({"operation":"rotate_90_cw"})),
        ("image.resize", json!({"width":100,"height":100})),
        ("image.rotate", json!({"angle_degrees":12})),
    ] {
        assert!(
            e.execute(e.request(Source::User), id, p, |_| Ok(()))
                .is_err()
        );
        assert!(archive(&e) == before);
    }
    let mut old = before;
    old.metadata["version"] = json!(6);
    assert!(Document::from_archive(old).is_err());
    let p = MemoryPool::new(MemoryLimits {
        cpu_bytes: 16 * 1024 * 1024,
        gpu_bytes: 0,
        disk_bytes: 16 * 1024 * 1024,
    });
    let mut low = Editor::with_memory_pool(p.clone());
    run(
        &mut low,
        "file.new",
        json!({"width":160,"height":120,"bit_depth":16}),
    );
    let old = archive(&low);
    assert!(
        low.execute(
            low.request(Source::User),
            "type.create",
            json!({"name":"budget","spec":spec("中文"),"color":[0.,0.,0.,1]}),
            |_| Ok(())
        )
        .is_err()
    );
    assert!(archive(&low) == old);
    assert_eq!(p.stats().transient_bytes, 0);
}

#[test]
fn runtime_text_cancellation_and_parent_locks_preserve_source_and_release_scratch() {
    let pool = MemoryPool::new(MemoryLimits {
        cpu_bytes: 256 * 1024 * 1024,
        gpu_bytes: 0,
        disk_bytes: 128 * 1024 * 1024,
    });
    let mut e = Editor::with_memory_pool(pool.clone());
    run(
        &mut e,
        "file.new",
        json!({"width":160,"height":120,"bit_depth":16}),
    );
    let before = archive(&e);
    let request = e.request(Source::User);
    let worker = e.clone();
    let join = std::thread::spawn(move || {
        worker.prepare(request,"type.create",json!({"name":"cancel","spec":{"text":"中文".repeat(1000),"font":"cjk","font_size":128,"layout":{"type":"paragraph","width":1000,"height":1000}},"color":[0.2,0.3,0.4,1]}))
    });
    let started = std::time::Instant::now();
    while pool.stats().transient_bytes < 96 * 1024 * 1024
        && started.elapsed() < std::time::Duration::from_secs(3)
    {
        std::thread::yield_now()
    }
    assert!(
        pool.stats().transient_bytes >= 96 * 1024 * 1024,
        "font work reservation was not observed"
    );
    e.cancel_pending_work();
    assert!(join.join().unwrap().is_err());
    assert!(archive(&e) == before);
    assert_eq!(pool.stats().transient_bytes, 0);
    create(&mut e, "locked source");
    let id = e.snapshot().active_layer;
    let group = run(
        &mut e,
        "layer.group",
        json!({"layers":[{"id":id}],"name":"parent"}),
    )["layer_id"]
        .clone();
    run(
        &mut e,
        "layer.set_locks",
        json!({"layer":{"id":group},"locks":{"pixels":true}}),
    );
    let old = archive(&e);
    assert!(
        e.execute(
            e.request(Source::User),
            "type.update",
            json!({"layer":{"id":id},"spec":spec("blocked"),"color":[0.,0.,0.,1]}),
            |_| Ok(())
        )
        .is_err()
    );
    assert!(
        e.execute(
            e.request(Source::User),
            "type.create",
            json!({"name":"blocked","spec":spec("blocked"),"color":[0.,0.,0.,1]}),
            |_| Ok(())
        )
        .is_err()
    );
    assert!(archive(&e) == old);
}

#[test]
fn text_profile_conversion_updates_both_editable_color_and_cached_source_without_changing_appearance()
 {
    for depth in [8, 16] {
        let mut e = editor(depth, "srgb");
        create(&mut e, "Profile 中文");
        let id = e.snapshot().active_layer;
        let before = archive(&e);
        let old_pixels = e.document().unwrap().export_pixels().unwrap();
        let old =
            strata_color::to_linear_p3(&old_pixels, 160, 120, e.document().unwrap().working_icc())
                .unwrap();
        run(&mut e, "color.convert", json!({"space":"display_p3"}));
        let result = run(&mut e, "type.inspect", json!({"layer":{"id":id}}));
        let codes = result["definition"]["color"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_u64().unwrap() as u16)
            .collect::<Vec<_>>();
        let doc = e.document().unwrap();
        let actual = doc.export_pixels().unwrap();
        let converted = strata_color::to_linear_p3(&actual, 160, 120, doc.working_icc()).unwrap();
        for (a, b) in old.iter().zip(&converted) {
            assert_eq!(a[3], b[3]);
            if a[3] > 0. {
                for c in 0..3 {
                    assert!(
                        (a[c] - b[c]).abs() < if depth == 8 { 0.006 } else { 0.0002 },
                        "depth{depth} component{c}"
                    )
                }
            }
        }
        for (_, tile) in doc.layers().last().unwrap().pixels().tiles() {
            let tile = tile.unwrap();
            for i in 0..256 * 256 {
                let p = match &*tile {
                    strata_pixels::PixelBuffer::Rgba8(v) => {
                        std::array::from_fn::<_, 4, _>(|c| u16::from(v[i * 4 + c]))
                    }
                    strata_pixels::PixelBuffer::Rgba16(v) => std::array::from_fn(|c| v[i * 4 + c]),
                };
                if p[3] > 0 {
                    assert_eq!(&p[..3], &codes[..3]);
                }
            }
        }
        let max = if depth == 8 { 255. } else { 65535. };
        let rgba = codes
            .into_iter()
            .map(|v| f64::from(v) / max)
            .collect::<Vec<_>>();
        let cursor = e.snapshot().history_cursor;
        assert_eq!(
            run(
                &mut e,
                "type.update",
                json!({"layer":{"id":id},"spec":result["definition"]["spec"],"color":rgba})
            )["changed"],
            false
        );
        assert_eq!(e.snapshot().history_cursor, cursor);
        run(&mut e, "edit.undo", json!({}));
        assert!(archive(&e) == before);
    }
}
