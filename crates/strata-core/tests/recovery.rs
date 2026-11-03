use serde_json::{Value, json};
use strata_core::{Editor, Source};
use strata_io::recovery::RecoveryStore;

fn run(e: &mut Editor, id: &str, p: Value) {
    let request = e.request(Source::User);
    e.execute(request, id, p, |_| Ok(())).unwrap();
}
#[test]
fn checkpoints_restore_exact_samples_and_remain_dirty_without_original_write_authority() {
    let dir = tempfile::tempdir().unwrap();
    let store = RecoveryStore::new(&dir.path().join("recovery")).unwrap();
    let original = dir.path().join("original.strata");
    let mut e = Editor::new();
    run(
        &mut e,
        "file.new",
        json!({"width":3,"height":2,"bit_depth":16}),
    );
    let write = e.grant_write(&original, false, Source::User).unwrap();
    run(&mut e, "file.save_as", json!({"resource":write}));
    assert!(e.checkpoint_recovery(&store, "original").unwrap().is_none());
    let original_bytes = std::fs::read(&original).unwrap();
    run(
        &mut e,
        "layer.create",
        json!({"name":"precise","fill":[32768,32769,50001,0]}),
    );
    let pixels = e.document().unwrap().export_pixels().unwrap();
    let before = serde_json::to_value(e.snapshot()).unwrap();
    let info = e
        .checkpoint_recovery(&store, "original.strata")
        .unwrap()
        .unwrap();
    assert_eq!(serde_json::to_value(e.snapshot()).unwrap(), before);
    assert_eq!(std::fs::read(&original).unwrap(), original_bytes);
    let entries = store.list().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0].info.as_ref().unwrap().original_path.as_deref(),
        std::fs::canonicalize(&original).unwrap().to_str()
    );
    let mut restored = Editor::new();
    let read = restored.grant_read(&entries[0].path, Source::User).unwrap();
    let request = restored.request(Source::User);
    assert!(
        restored
            .execute(request, "file.recover", json!({"resource":read}), |_| Err(
                "display failed".into()
            ))
            .is_err()
    );
    assert!(restored.document().is_none());
    assert!(entries[0].path.exists());
    run(&mut restored, "file.recover", json!({"resource":read}));
    assert!(restored.dirty());
    assert!(restored.project_path().is_none());
    assert_eq!(
        restored.document().unwrap().export_pixels().unwrap(),
        pixels
    );
    assert!(restored.snapshot().history.is_empty());
    let request = restored.request(Source::User);
    assert!(
        restored
            .prepare(request, "file.save", json!({"resource":original}))
            .is_err()
    );
    store.remove(&info.session_id).unwrap();
    assert!(store.list().unwrap().is_empty());
}

#[test]
fn checkpoint_replacement_failure_preserves_previous_copy_and_corrupt_entries_are_retained() {
    let dir = tempfile::tempdir().unwrap();
    let store = RecoveryStore::new(dir.path()).unwrap();
    let mut e = Editor::new();
    run(
        &mut e,
        "file.new",
        json!({"width":2,"height":2,"bit_depth":8}),
    );
    let info = e.checkpoint_recovery(&store, "first").unwrap().unwrap();
    let path = store.path(&info.session_id).unwrap();
    let before = std::fs::read(&path).unwrap();
    assert!(e.checkpoint_recovery(&store, &"x".repeat(1025)).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), before);
    std::fs::write(&path, b"corrupt").unwrap();
    let entry = store.list().unwrap().remove(0);
    assert!(entry.error.is_some());
    assert!(path.exists());
    assert!(store.remove("../../original").is_err());
    let resource = e.grant_read(&path, Source::User).unwrap();
    let state = e.document().unwrap().state_id();
    let request = e.request(Source::User);
    assert!(
        e.prepare(
            request,
            "file.recover",
            json!({"resource":resource,"discard_changes":true})
        )
        .is_err()
    );
    assert_eq!(e.document().unwrap().state_id(), state);
}

#[test]
fn recovery_crash_helper() {
    let Ok(path) = std::env::var("STRATA_RECOVERY_CRASH_PATH") else {
        return;
    };
    use strata_io::project::{SaveStage, read_project, write_project_with_hook};
    let path = std::path::Path::new(&path);
    let mut archive = read_project(path).unwrap();
    archive.metadata["recovery"]["label"] = json!("interrupted checkpoint");
    archive.tiles[0].bytes[0] ^= 1;
    write_project_with_hook(path, &archive, true, true, |stage| {
        if stage == SaveStage::BeforeCommit {
            std::process::exit(78);
        }
        Ok(())
    })
    .unwrap();
}

#[test]
fn killed_incremental_checkpoint_is_discoverable_and_restores_last_committed_samples() {
    let dir = tempfile::tempdir().unwrap();
    let store = RecoveryStore::new(dir.path()).unwrap();
    let mut e = Editor::new();
    run(
        &mut e,
        "file.new",
        json!({"width":2,"height":2,"bit_depth":16}),
    );
    run(
        &mut e,
        "layer.create",
        json!({"name":"data","fill":[32768,32769,50001,65535]}),
    );
    let info = e
        .checkpoint_recovery(&store, "last committed")
        .unwrap()
        .unwrap();
    // Subsequent successful checkpoints update the same private database.
    e.checkpoint_recovery(&store, "last committed").unwrap();
    let path = store.path(&info.session_id).unwrap();
    let result = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "recovery_crash_helper", "--nocapture"])
        .env("STRATA_RECOVERY_CRASH_PATH", &path)
        .status()
        .unwrap();
    assert_eq!(result.code(), Some(78));
    let entry = store.list().unwrap().remove(0);
    assert_eq!(entry.info.unwrap().label, "last committed");
    let mut restored = Editor::new();
    let read = restored.grant_read(&path, Source::User).unwrap();
    run(&mut restored, "file.recover", json!({"resource":read}));
    assert_eq!(
        restored.document().unwrap().export_pixels().unwrap(),
        e.document().unwrap().export_pixels().unwrap()
    );
    store.remove(&info.session_id).unwrap();
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}
