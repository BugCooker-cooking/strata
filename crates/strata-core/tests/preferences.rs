use serde_json::{Value, json};
use strata_core::{Editor, Source, commands::AppPreferencesStore};
use strata_pixels::memory::{MemoryKind, MemoryLimits, MemoryPool};

fn run(editor: &mut Editor, command: &str, params: Value) -> Value {
    editor
        .execute(editor.request(Source::User), command, params, |_| Ok(()))
        .unwrap()
}
fn ai() -> Value {
    json!({"mode":"ask_first","max_steps":12,"request_timeout_seconds":15,"turn_timeout_seconds":90})
}
fn fixture() -> (Editor, MemoryPool) {
    let pool = MemoryPool::new(MemoryLimits::default());
    (Editor::with_memory_pool(pool.clone()), pool)
}

#[test]
fn preferences_work_without_a_document_and_reject_invalid_or_extra_fields() {
    let (mut editor, _) = fixture();
    let original = run(&mut editor, "system.preferences.get", json!({}));
    assert_eq!(original["ai"]["max_steps"], 50);
    assert_eq!(original["ai"]["mode"], "ask_first");
    assert_eq!(original["memory"]["cpu_mib"], 2048);
    for params in [
        json!({}),
        json!({"path":"ignored"}),
        json!({"ai":{"mode":"direct","max_steps":0,"request_timeout_seconds":15,"turn_timeout_seconds":90}}),
        json!({"ai":{"mode":"direct","max_steps":201,"request_timeout_seconds":15,"turn_timeout_seconds":90}}),
        json!({"ai":{"mode":"direct","max_steps":12,"request_timeout_seconds":4,"turn_timeout_seconds":90}}),
        json!({"ai":{"mode":"direct","max_steps":12,"request_timeout_seconds":121,"turn_timeout_seconds":90}}),
        json!({"ai":{"mode":"direct","max_steps":12,"request_timeout_seconds":15,"turn_timeout_seconds":29}}),
        json!({"ai":{"mode":"direct","max_steps":12,"request_timeout_seconds":15,"turn_timeout_seconds":1801}}),
        json!({"ai":{"mode":"automatic","max_steps":12,"request_timeout_seconds":15,"turn_timeout_seconds":90}}),
        json!({"ai":{"mode":"direct","max_steps":12,"request_timeout_seconds":15,"turn_timeout_seconds":90,"disable_approval":true}}),
        json!({"memory":{"cpu_mib":255,"gpu_mib":512,"disk_mib":32768}}),
        json!({"memory":{"cpu_mib":2048,"gpu_mib":63,"disk_mib":32768}}),
        json!({"memory":{"cpu_mib":2048,"gpu_mib":512,"disk_mib":1023}}),
        json!({"memory":{"cpu_mib":u64::MAX,"gpu_mib":512,"disk_mib":32768}}),
    ] {
        assert!(
            editor
                .prepare(
                    editor.request(Source::User),
                    "system.preferences.set",
                    params.clone()
                )
                .is_err(),
            "{params}"
        );
        assert_eq!(
            run(&mut editor, "system.preferences.get", json!({})),
            original
        );
    }
    let updated = run(
        &mut editor,
        "system.preferences.set",
        json!({"ai":ai(),"expected_version":0}),
    );
    let same = run(
        &mut editor,
        "system.preferences.set",
        json!({"ai":ai(),"expected_version":1}),
    );
    assert_eq!(updated, same);
    assert_eq!(updated["memory"], original["memory"]);
}

#[test]
fn shared_versions_and_failed_budget_reduction_preserve_the_entire_settings_update() {
    let pool = MemoryPool::new(MemoryLimits::default());
    let store = AppPreferencesStore::new(pool.clone());
    let mut a = Editor::new().with_app_preferences(store.clone());
    let mut b = Editor::new().with_app_preferences(store);
    let pending = a
        .prepare(
            a.request(Source::User),
            "system.preferences.set",
            json!({"ai":ai(),"expected_version":0}),
        )
        .unwrap();
    let newer = run(
        &mut b,
        "system.preferences.set",
        json!({"ai":{"mode":"direct","max_steps":7,"request_timeout_seconds":11,"turn_timeout_seconds":45}}),
    );
    assert!(a.commit(pending).is_err());
    assert_eq!(run(&mut a, "system.preferences.get", json!({})), newer);

    let pinned = pool
        .reserve(MemoryKind::Transient, 270 * 1024 * 1024)
        .unwrap();
    let params = json!({"ai":ai(),"memory":{"cpu_mib":256,"gpu_mib":64,"disk_mib":1024},"expected_version":1});
    assert!(
        a.execute(
            a.request(Source::User),
            "system.preferences.set",
            params.clone(),
            |_| Ok(())
        )
        .is_err()
    );
    assert_eq!(run(&mut a, "system.preferences.get", json!({})), newer);
    assert_eq!(pool.limits(), MemoryLimits::default());
    drop(pinned);
    let changed = run(&mut a, "system.preferences.set", params);
    assert_eq!(changed["version"], 2);
    assert_eq!(
        pool.limits(),
        MemoryLimits {
            cpu_bytes: 256 * 1024 * 1024,
            gpu_bytes: 64 * 1024 * 1024,
            disk_bytes: 1024 * 1024 * 1024
        }
    );
    assert_eq!(run(&mut b, "system.preferences.get", json!({})), changed);
}

#[test]
fn trimming_idle_resources_preserves_document_and_actual_memory_limits() {
    let (mut editor, pool) = fixture();
    run(
        &mut editor,
        "file.new",
        json!({"width":16,"height":16,"bit_depth":16,"background":[10001,30003,50001,32769]}),
    );
    let original = editor.document().unwrap().to_archive().unwrap();
    let state = serde_json::to_value(editor.snapshot()).unwrap();
    let before = run(&mut editor, "system.memory.get", json!({}));
    assert!(before["usage"]["source_bytes"].as_u64().unwrap() > 0);
    let after = run(&mut editor, "system.memory.trim", json!({}));
    assert_eq!(after["usage"]["source_bytes"], 0);
    assert!(after["usage"]["disk_bytes"].as_u64().unwrap() > 0);
    assert_eq!(after["limits"], before["limits"]);
    assert!(editor.document().unwrap().to_archive().unwrap() == original);
    assert_eq!(serde_json::to_value(editor.snapshot()).unwrap(), state);
    run(&mut editor, "system.preferences.set", json!({"ai":ai()}));
    assert_eq!(pool.limits(), MemoryLimits::default());
    assert_eq!(serde_json::to_value(editor.snapshot()).unwrap(), state);
}

#[test]
fn settings_cancellation_ownership_and_retry_keep_document_history_independent() {
    let (mut editor, _) = fixture();
    run(
        &mut editor,
        "file.new",
        json!({"width":8,"height":8,"bit_depth":8}),
    );
    run(&mut editor, "layer.create", json!({"name":"Keep history"}));
    let original = editor.document().unwrap().to_archive().unwrap();
    let request = editor.request(Source::User);
    let params = json!({"ai":ai()});
    let prepared = editor
        .prepare(request.clone(), "system.preferences.set", params.clone())
        .unwrap();
    let result = editor.commit(prepared).unwrap();
    let retry = editor
        .prepare(request, "system.preferences.set", params)
        .unwrap();
    assert_eq!(editor.commit(retry).unwrap(), result);
    assert!(editor.document().unwrap().to_archive().unwrap() == original);
    run(&mut editor, "edit.undo", json!({}));
    assert_eq!(
        run(&mut editor, "system.preferences.get", json!({})),
        result
    );

    let pending = editor
        .prepare(
            editor.request(Source::User),
            "system.preferences.set",
            json!({"memory":{"cpu_mib":512,"gpu_mib":128,"disk_mib":2048}}),
        )
        .unwrap();
    editor.cancel_pending_work();
    assert!(editor.commit(pending).is_err());
    editor.begin_turn().unwrap();
    assert!(
        editor
            .prepare(
                editor.request(Source::User),
                "system.preferences.set",
                json!({"ai":ai()})
            )
            .is_err()
    );
    assert!(
        editor
            .prepare(
                editor.request(Source::User),
                "system.memory.trim",
                json!({})
            )
            .is_err()
    );
    editor.stop_turn();
}
