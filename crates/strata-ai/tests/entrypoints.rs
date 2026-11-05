use serde_json::Value;
use strata_ai::catalog;

#[test]
fn mapped_feature_prompts_find_real_commands_with_strict_descriptions() {
    let manifest: Value = serde_json::from_str(include_str!(
        "../../../docs/research/ai-editing-entrypoints.json"
    ))
    .unwrap();
    let mut checked = 0;
    for (feature, entry) in manifest["features"].as_object().unwrap() {
        for prompt in entry["prompts"].as_array().unwrap() {
            let query = prompt["query"].as_str().unwrap();
            let command = prompt["command"].as_str().unwrap();
            assert!(
                catalog::search(query, 32).unwrap()["matches"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|m| m["id"] == command),
                "{feature}: query {query} cannot find {command}"
            );
            assert!(!prompt["text"].as_str().unwrap().trim().is_empty());
            checked += 1;
        }
        for command in entry["commands"].as_array().unwrap() {
            let command = command.as_str().unwrap();
            let described = catalog::describe(&[command.into()]).unwrap();
            let spec = &described["commands"][0];
            assert_eq!(spec["id"], command);
            assert_eq!(
                spec["params"]["additionalProperties"], false,
                "{feature}: loose input schema for {command}"
            );
            assert!(spec["returns"].is_object());
            assert!(!spec["description"].as_str().unwrap().is_empty());
            assert!(!spec["examples"].as_array().unwrap().is_empty());
        }
    }
    assert!(checked >= 50, "the mapping was silently reduced");
}
