use serde_json::json;
use strata_core::{Editor, Source};
fn list(e: &mut Editor, source: Source) -> serde_json::Value {
    let r = e.request(source);
    e.execute(r, "file.recent.list", json!({}), |_| Ok(()))
        .unwrap()
}
#[test]
fn recent_history_does_not_disclose_or_authorize_files_to_a_model() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("allowed.png");
    let b = dir.path().join("private-client.png");
    std::fs::write(&a, b"a").unwrap();
    std::fs::write(&b, b"b").unwrap();
    let mut e = Editor::new();
    e.set_recent_files(vec![a.clone(), b.clone()]).unwrap();
    assert_eq!(
        list(&mut e, Source::User)["files"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let turn = e.begin_turn().unwrap();
    let source = Source::Ai { turn_id: turn };
    assert!(
        list(&mut e, source.clone())["files"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let token = e.grant_read(&a, source.clone()).unwrap();
    let result = list(&mut e, source.clone());
    assert_eq!(result["scope"], "authorized_resources");
    assert_eq!(result["files"].as_array().unwrap().len(), 1);
    assert_eq!(result["files"][0]["read_resource"], token);
    assert!(result["files"][0]["path"].is_null());
    assert!(!result.to_string().contains("private-client"));
    std::fs::write(&a, b"changed source").unwrap();
    assert!(list(&mut e, source)["files"].as_array().unwrap().is_empty());
    e.stop_turn();
    let next = e.begin_turn().unwrap();
    assert!(
        list(&mut e, Source::Ai { turn_id: next })["files"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}
#[test]
fn recent_metadata_is_bounded_deduplicated_and_independent_of_content_history() {
    let dir = tempfile::tempdir().unwrap();
    let mut e = Editor::new();
    let paths: Vec<_> = (0..12)
        .map(|i| dir.path().join(format!("{i}.png")))
        .collect();
    let before = serde_json::to_value(e.snapshot()).unwrap();
    let mut repeated = vec![paths[0].clone(), paths[0].clone()];
    repeated.extend(paths.iter().skip(1).cloned());
    e.set_recent_files(repeated).unwrap();
    let first = list(&mut e, Source::User);
    assert_eq!(first["files"].as_array().unwrap().len(), 10);
    assert_eq!(first["files"][0]["available"], false);
    assert!(first["files"][0]["read_resource"].is_null());
    e.set_recent_files(paths).unwrap();
    let next = list(&mut e, Source::User);
    assert_eq!(first["files"][0]["id"], next["files"][0]["id"]);
    assert_eq!(serde_json::to_value(e.snapshot()).unwrap(), before);
    e.set_recent_files(vec![]).unwrap();
    assert!(
        list(&mut e, Source::User)["files"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}
