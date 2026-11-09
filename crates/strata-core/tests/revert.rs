use serde_json::{Value, json};
use std::path::Path;
use strata_core::{Editor, Source};
fn run(e: &mut Editor, id: &str, p: Value) -> Value {
    e.execute(e.request(Source::User), id, p, |_| Ok(()))
        .unwrap()
}
fn make(depth: u8) -> Editor {
    let mut e = Editor::new();
    run(
        &mut e,
        "file.new",
        json!({"width":9,"height":7,"bit_depth":depth,"working_space":"display_p3","resolution_dpi":144,"background":if depth==16{[10001,30003,50001,32769]}else{[41,117,191,129]}}),
    );
    e
}
fn open(e: &mut Editor, path: &Path) {
    let resource = e.grant_read(path, Source::User).unwrap();
    run(
        e,
        "file.open",
        json!({"resource":resource,"discard_changes":true}),
    );
}
fn revert(e: &mut Editor, path: &Path) {
    let resource = e.grant_read(path, Source::User).unwrap();
    run(
        e,
        "file.revert",
        json!({"resource":resource,"discard_changes":true}),
    );
}
fn save(e: &mut Editor, path: &Path) {
    let resource = e.grant_write(path, false, Source::User).unwrap();
    run(e, "file.save_as", json!({"resource":resource}));
}
#[test]
fn original_images_revert_real_disk_pixels_depth_icc_and_dpi_with_clean_history() {
    for depth in [8, 16] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("original.png");
        let mut writer = make(depth);
        let resource = writer.grant_write(&path, false, Source::User).unwrap();
        run(
            &mut writer,
            "file.export",
            json!({"resource":resource,"format":"png"}),
        );
        let disk = std::fs::read(&path).unwrap();
        let mut e = Editor::new();
        open(&mut e, &path);
        let pixels = e.document().unwrap().export_pixels().unwrap();
        let icc = e.document().unwrap().working_icc().to_vec();
        let old = e.document().unwrap().id();
        let info = run(&mut e, "file.revert_status", json!({}));
        assert_eq!(info["name"], "original.png");
        assert_eq!(info["source_kind"], "opened_image");
        assert_eq!(info["available"], true);
        assert!(!info.to_string().contains(dir.path().to_str().unwrap()));
        run(
            &mut e,
            "layer.set_position",
            json!({"layer":{"name":"original"},"x":2,"y":3}),
        );
        run(
            &mut e,
            "view.guides.add",
            json!({"name":"Unsaved","axis":"vertical","position":2.5}),
        );
        run(
            &mut e,
            "view.presentation.set",
            json!({"ruler_origin":[2,3],"ruler_unit":"percent"}),
        );
        let presentation = e.snapshot().presentation.clone();
        revert(&mut e, &path);
        assert_eq!(e.document().unwrap().export_pixels().unwrap(), pixels);
        assert_eq!(e.document().unwrap().working_icc(), icc);
        assert_eq!(e.document().unwrap().bit_depth(), depth);
        assert_eq!(e.document().unwrap().resolution_dpi(), 144);
        assert_eq!(e.document().unwrap().layers().len(), 1);
        assert!(e.document().unwrap().guides().is_empty());
        assert_ne!(e.document().unwrap().id(), old);
        assert!(e.snapshot().history.is_empty());
        assert_eq!(e.snapshot().history_cursor, 0);
        assert!(!e.dirty());
        assert_eq!(e.snapshot().presentation, presentation);
        assert_eq!(std::fs::read(&path).unwrap(), disk);
        assert_eq!(e.project_path(), None);
    }
}
#[test]
fn saved_projects_restore_editable_layers_masks_text_and_guides_and_save_as_changes_the_target() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("first.strata");
    let b = dir.path().join("second.strata");
    let mut e = make(16);
    run(
        &mut e,
        "select.marquee",
        json!({"x":1,"y":1,"width":3,"height":2}),
    );
    run(
        &mut e,
        "layer.mask.add",
        json!({"layer":{"name":"Background"},"source":"from_selection"}),
    );
    run(&mut e, "select.none", json!({}));
    run(
        &mut e,
        "type.create",
        json!({"name":"Editable","spec":{"text":"Retained 中文","font":"sans","font_size":12,"layout":{"type":"point"}},"color":[0.2,0.4,0.8,1],"x":1,"y":5}),
    );
    run(
        &mut e,
        "view.guides.add",
        json!({"name":"Saved","axis":"vertical","position":3.5}),
    );
    save(&mut e, &a);
    let first = e.document().unwrap().to_archive().unwrap();
    run(
        &mut e,
        "layer.rename",
        json!({"layer":{"name":"Editable"},"name":"Unsaved"}),
    );
    revert(&mut e, &a);
    assert!(e.document().unwrap().to_archive().unwrap() == first);
    assert!(e.snapshot().history.is_empty());
    run(
        &mut e,
        "layer.rename",
        json!({"layer":{"name":"Editable"},"name":"Second saved text"}),
    );
    save(&mut e, &b);
    let second = e.document().unwrap().to_archive().unwrap();
    let canonical_b = std::fs::canonicalize(&b).unwrap();
    assert_eq!(e.revert_source(), Some(canonical_b.as_path()));
    let wrong = e.grant_read(&a, Source::User).unwrap();
    assert!(
        e.prepare(
            e.request(Source::User),
            "file.revert",
            json!({"resource":wrong,"discard_changes":true})
        )
        .is_err()
    );
    run(
        &mut e,
        "layer.delete",
        json!({"layer":{"name":"Second saved text"}}),
    );
    revert(&mut e, &b);
    assert!(e.document().unwrap().to_archive().unwrap() == second);
    assert_eq!(e.project_path(), Some(canonical_b.as_path()));
    assert!(!e.dirty());
}
#[test]
fn no_source_missing_discard_flag_and_unrelated_or_wrong_source_resources_are_atomic() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a.strata");
    let b = dir.path().join("b.strata");
    let mut e = make(8);
    assert_eq!(
        run(&mut e, "file.revert_status", json!({}))["available"],
        false
    );
    let mut writer = make(8);
    save(&mut writer, &a);
    save(&mut writer, &b);
    let r = e.grant_read(&a, Source::User).unwrap();
    assert!(
        e.prepare(
            e.request(Source::User),
            "file.revert",
            json!({"resource":r,"discard_changes":true})
        )
        .is_err()
    );
    open(&mut e, &a);
    let r = e.grant_read(&a, Source::User).unwrap();
    let before = e.document().unwrap().to_archive().unwrap();
    let state = e.snapshot();
    for p in [
        json!({"resource":r,"discard_changes":true,"expected_revision":state.revision+1}),
        json!({"resource":r}),
        json!({"resource":"guessed","discard_changes":true}),
        json!({"resource":r,"discard_changes":true,"path":a}),
    ] {
        assert!(
            e.prepare(e.request(Source::User), "file.revert", p)
                .is_err()
        );
    }
    let other = e.grant_read(&b, Source::User).unwrap();
    assert!(
        e.prepare(
            e.request(Source::User),
            "file.revert",
            json!({"resource":other,"discard_changes":true})
        )
        .is_err()
    );
    assert!(e.document().unwrap().to_archive().unwrap() == before);
    assert_eq!(e.revision(), state.revision);
    let turn = e.begin_turn().unwrap();
    assert!(
        e.prepare(
            e.request(Source::Ai { turn_id: turn }),
            "file.revert",
            json!({"resource":r,"discard_changes":true})
        )
        .is_err()
    );
    e.stop_turn();
    run(
        &mut e,
        "file.new",
        json!({"width":4,"height":4,"bit_depth":8,"discard_changes":true}),
    );
    assert!(e.revert_source().is_none());
}
#[test]
fn corrupt_removed_replaced_or_changed_destination_files_never_destroy_current_truth() {
    for change in [
        "corrupt",
        "removed",
        "changed_after_prepare",
        "display_failure",
        "cancel",
        "revision",
        "save_as",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("original.strata");
        let mut e = make(16);
        save(&mut e, &path);
        run(
            &mut e,
            "layer.create",
            json!({"name":"Keep until successful revert"}),
        );
        let before = e.document().unwrap().to_archive().unwrap();
        let r = e.grant_read(&path, Source::User).unwrap();
        if change == "corrupt" || change == "removed" {
            let r = if change == "corrupt" {
                std::fs::write(&path, b"not a project").unwrap();
                e.grant_read(&path, Source::User).unwrap()
            } else {
                std::fs::remove_file(&path).unwrap();
                r
            };
            assert!(
                e.prepare(
                    e.request(Source::User),
                    "file.revert",
                    json!({"resource":r,"discard_changes":true})
                )
                .is_err()
            );
            assert!(e.document().unwrap().to_archive().unwrap() == before);
            continue;
        }
        let candidate = e
            .prepare(
                e.request(Source::User),
                "file.revert",
                json!({"resource":r,"discard_changes":true}),
            )
            .unwrap();
        match change {
            "changed_after_prepare" => std::fs::write(&path, b"replaced independently").unwrap(),
            "display_failure" => {
                let error = e.execute(
                    e.request(Source::User),
                    "file.revert",
                    json!({"resource":r,"discard_changes":true}),
                    |_| Err("display failed".into()),
                );
                assert!(error.is_err());
                assert!(e.document().unwrap().to_archive().unwrap() == before);
                continue;
            }
            "cancel" => e.cancel_pending_work(),
            "revision" => {
                run(
                    &mut e,
                    "layer.rename",
                    json!({"layer":{"name":"Keep until successful revert"},"name":"Newer truth"}),
                );
            }
            "save_as" => {
                save(&mut e, &dir.path().join("newest.strata"));
            }
            _ => unreachable!(),
        }
        let truth = e.document().unwrap().to_archive().unwrap();
        let history = e.snapshot().history.len();
        assert!(e.commit(candidate).is_err(), "{change}");
        assert!(e.document().unwrap().to_archive().unwrap() == truth);
        assert_eq!(e.snapshot().history.len(), history);
    }
}
