use serde_json::{Value, json};
use strata_core::{Editor, Source, commands::ExportPresetStore};
fn run(e: &mut Editor, id: &str, p: Value) -> Value {
    e.execute(e.request(Source::User), id, p, |_| Ok(()))
        .unwrap()
}
fn preset(e: &mut Editor) -> Value {
    run(e, "file.export_preset.get", json!({}))
}
fn document(e: &mut Editor, depth: u8) {
    run(
        e,
        "file.new",
        json!({"width":17,"height":13,"bit_depth":depth,"working_space":"display_p3","resolution_dpi":144}),
    );
    run(
        e,
        "layer.create",
        json!({"name":"Source","fill":if depth==8{[210,110,31,128]}else{[50001,30003,10005,32769]}}),
    );
}
#[test]
fn preset_settings_work_without_a_document_are_canonical_and_never_enter_undo_history() {
    let mut e = Editor::new();
    assert_eq!(preset(&mut e)["version"], 0);
    assert_eq!(preset(&mut e)["options"]["format"], "png");
    let changed = run(
        &mut e,
        "file.export_preset.set",
        json!({"options":{"format":"jpeg","background":[12,34,56]},"expected_version":0}),
    );
    assert_eq!(changed["version"], 1);
    assert_eq!(changed["options"]["quality"], 90);
    let same = run(
        &mut e,
        "file.export_preset.set",
        json!({"options":{"format":"jpeg","quality":90,"background":[12,34,56]},"expected_version":1}),
    );
    assert_eq!(same, changed);
    document(&mut e, 16);
    let before = e.document().unwrap().to_archive().unwrap();
    let revision = e.revision();
    let dirty = e.dirty();
    run(
        &mut e,
        "file.export_preset.set",
        json!({"options":{"format":"png","width":8,"height":6}}),
    );
    assert!(e.document().unwrap().to_archive().unwrap() == before);
    assert_eq!(e.revision(), revision);
    assert_eq!(e.dirty(), dirty);
    run(&mut e, "edit.undo", json!({}));
    assert_eq!(preset(&mut e)["options"]["width"], 8);
    assert_eq!(e.document().unwrap().layers().len(), 1);
}
#[test]
fn actual_quick_exports_decode_like_regular_exports_and_preserve_original_state() {
    for depth in [8, 16] {
        for (format, extra) in [
            ("png", json!({})),
            ("tiff", json!({})),
            ("jpeg", json!({"quality":100,"background":[255,255,255]})),
            ("webp", json!({"webp_lossless":true})),
            ("webp", json!({"webp_lossless":false,"quality":83})),
        ] {
            let mut e = Editor::new();
            document(&mut e, depth);
            let before = e.document().unwrap().to_archive().unwrap();
            let state = e.snapshot();
            let mut options = json!({"format":format,"width":8,"height":6,"target_space":"srgb","allow_quantization":true});
            options
                .as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            let p = run(&mut e, "file.export_preset.set", json!({"options":options}));
            let dir = tempfile::tempdir().unwrap();
            let quick = dir.path().join(format!("quick.{format}"));
            let regular = dir.path().join(format!("regular.{format}"));
            let grant = e.grant_write(&quick, false, Source::User).unwrap();
            let result = run(
                &mut e,
                "file.quick_export",
                json!({"resource":grant,"preset_version":p["version"]}),
            );
            assert_eq!(result["format"], format);
            assert_eq!(result["width"], 8);
            assert_eq!(result["height"], 6);
            let token = e.grant_write(&regular, false, Source::User).unwrap();
            let mut direct = options.clone();
            direct["resource"] = json!(token);
            run(&mut e, "file.export", direct);
            let a = strata_io::decode_image(&quick).unwrap();
            let b = strata_io::decode_image(&regular).unwrap();
            assert_eq!(a.pixels, b.pixels);
            assert_eq!((a.width, a.height), (8, 6));
            assert_eq!(a.resolution_dpi, 144);
            assert_eq!(
                a.pixels.format().bit_depth(),
                if matches!(format, "png" | "tiff") {
                    depth
                } else {
                    8
                }
            );
            assert!(
                strata_color::resolve_profile(a.icc_profile.as_deref())
                    .unwrap()
                    .name
                    .contains("sRGB")
            );
            assert!(e.document().unwrap().to_archive().unwrap() == before);
            assert_eq!(e.revision(), state.revision);
            assert_eq!(e.snapshot().history_cursor, state.history_cursor);
            assert_eq!(e.dirty(), state.dirty);
            assert_eq!(e.project_path(), None);
        }
    }
}
#[test]
fn shared_versions_reject_late_settings_and_prepared_quick_exports_before_file_publication() {
    let store = ExportPresetStore::default();
    let mut a = Editor::new().with_export_presets(store.clone());
    let mut b = Editor::new().with_export_presets(store);
    document(&mut a, 8);
    let pending = a
        .prepare(
            a.request(Source::User),
            "file.export_preset.set",
            json!({"options":{"format":"tiff"},"expected_version":0}),
        )
        .unwrap();
    let newer = run(
        &mut b,
        "file.export_preset.set",
        json!({"options":{"format":"jpeg","background":[0,0,0]}}),
    );
    assert!(a.commit(pending).is_err());
    assert_eq!(preset(&mut a), newer);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("out.jpg");
    let token = a.grant_write(&path, false, Source::User).unwrap();
    let candidate = a
        .prepare(
            a.request(Source::User),
            "file.quick_export",
            json!({"resource":token,"preset_version":newer["version"]}),
        )
        .unwrap();
    run(
        &mut b,
        "file.export_preset.set",
        json!({"options":{"format":"png"}}),
    );
    assert!(a.commit(candidate).is_err());
    assert!(!path.exists());
    assert_eq!(preset(&mut a), preset(&mut b));
}
#[test]
fn invalid_preset_fields_and_dimensions_are_atomic_and_do_not_create_file_capabilities() {
    let mut e = Editor::new();
    let before = preset(&mut e);
    for options in [
        json!({"format":"heic"}),
        json!({"format":"png","quality":90}),
        json!({"format":"jpeg"}),
        json!({"format":"jpeg","background":[255,255,255],"webp_lossless":true}),
        json!({"format":"webp","quality":0}),
        json!({"format":"webp","webp_lossless":false,"quality":101}),
        json!({"width":1}),
        json!({"width":0,"height":1}),
        json!({"width":u32::MAX,"height":u32::MAX}),
        json!({"format":"webp","width":16384,"height":1}),
        json!({"target_space":"cmyk"}),
        json!({"resource":"guessed"}),
        json!({"path":"/tmp/unapproved.png"}),
    ] {
        assert!(
            e.prepare(
                e.request(Source::User),
                "file.export_preset.set",
                json!({"options":options})
            )
            .is_err(),
            "{options}"
        );
        assert_eq!(preset(&mut e), before);
    }
    document(&mut e, 8);
    assert!(
        e.prepare(
            e.request(Source::User),
            "file.quick_export",
            json!({"resource":"guessed","preset_version":0})
        )
        .is_err()
    );
    assert!(
        e.prepare(
            e.request(Source::User),
            "file.quick_export",
            json!({"resource":"guessed","preset_version":0,"format":"jpeg"})
        )
        .is_err()
    );
}
#[test]
fn quantization_is_never_implicit_and_write_resources_stay_single_use() {
    let mut e = Editor::new();
    document(&mut e, 16);
    let p = run(
        &mut e,
        "file.export_preset.set",
        json!({"options":{"format":"jpeg","background":[255,255,255]}}),
    );
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("quantized.jpg");
    let token = e.grant_write(&path, false, Source::User).unwrap();
    assert!(
        e.prepare(
            e.request(Source::User),
            "file.quick_export",
            json!({"resource":token,"preset_version":p["version"]})
        )
        .is_err()
    );
    assert!(!path.exists());
    let p = run(
        &mut e,
        "file.export_preset.set",
        json!({"options":{"format":"jpeg","background":[255,255,255],"allow_quantization":true}}),
    );
    run(
        &mut e,
        "file.quick_export",
        json!({"resource":token,"preset_version":p["version"]}),
    );
    assert!(
        e.prepare(
            e.request(Source::User),
            "file.quick_export",
            json!({"resource":token,"preset_version":p["version"]})
        )
        .is_err()
    );
    assert_eq!(
        strata_io::decode_image(&path)
            .unwrap()
            .pixels
            .format()
            .bit_depth(),
        8
    );
}
#[test]
fn cancellation_document_changes_and_destination_races_do_not_publish_old_candidates() {
    for change in ["cancel", "document", "destination"] {
        let mut e = Editor::new();
        document(&mut e, 16);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.png");
        let token = e.grant_write(&path, false, Source::User).unwrap();
        let candidate = e
            .prepare(
                e.request(Source::User),
                "file.quick_export",
                json!({"resource":token,"preset_version":0}),
            )
            .unwrap();
        match change {
            "cancel" => e.cancel_pending_work(),
            "document" => {
                run(
                    &mut e,
                    "layer.rename",
                    json!({"layer":{"name":"Source"},"name":"changed"}),
                );
            }
            "destination" => std::fs::write(&path, b"independent existing file").unwrap(),
            _ => unreachable!(),
        }
        assert!(e.commit(candidate).is_err(), "{change}");
        if change == "destination" {
            assert_eq!(std::fs::read(&path).unwrap(), b"independent existing file");
        } else {
            assert!(!path.exists());
        }
    }
}
