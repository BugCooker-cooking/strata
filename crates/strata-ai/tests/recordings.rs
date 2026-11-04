//! Replays the successful real-service transcripts. UUIDs returned by the
//! editor are rebound from recorded tool results; model text is never rewritten.
use serde_json::{Value, json};
use std::{collections::HashMap, sync::Mutex};
use strata_ai::{
    AiError, ContextGrant, ExecutionMode, ModelReply, ProviderConfig, Session, SessionOptions,
    Step, ToolHost,
    provider::Message,
    session::{CanvasResult, HostAnswer},
    tools::CanvasRequest,
};
use strata_core::{
    Editor, Source,
    commands::{Control, Snapshot},
};
use uuid::Uuid;

struct Host(Mutex<Editor>);
impl ToolHost for Host {
    fn snapshot(&self) -> Result<Snapshot, AiError> {
        Ok(self.0.lock().unwrap().snapshot())
    }
    fn execute(
        &self,
        turn: Uuid,
        call: Uuid,
        id: &str,
        params: Value,
        timeout: std::time::Duration,
    ) -> Result<Value, AiError> {
        let mut editor = self.0.lock().unwrap();
        let mut request = editor
            .request(Source::Ai { turn_id: turn })
            .with_timeout(timeout);
        request.call_id = call;
        editor
            .execute(request, id, params, |_| Ok(()))
            .map_err(|e| AiError::Host(e.to_string()))
    }
    fn pause(&self, _: Uuid) -> Result<(), AiError> {
        self.0
            .lock()
            .unwrap()
            .pause_turn()
            .map_err(|e| AiError::Host(e.to_string()))
    }
    fn resume(&self, _: Uuid) -> Result<(), AiError> {
        self.0
            .lock()
            .unwrap()
            .resume_turn()
            .map_err(|e| AiError::Host(e.to_string()))
    }
    fn stop(&self, _: Uuid) -> Result<(), AiError> {
        self.0.lock().unwrap().stop_turn();
        Ok(())
    }
    fn canvas(&self, _: &CanvasRequest, _: bool) -> Result<CanvasResult, AiError> {
        Ok(CanvasResult {
            data: json!({"image_available":false,"statistics":{}}),
            image: None,
        })
    }
}
fn user(editor: &mut Editor, id: &str, params: Value) {
    editor
        .execute(editor.request(Source::User), id, params, |_| Ok(()))
        .unwrap();
}
fn bindings(recorded: &Value, actual: &Value, map: &mut HashMap<String, String>) {
    match (recorded, actual) {
        (Value::String(old), Value::String(new))
            if Uuid::parse_str(old).is_ok() && Uuid::parse_str(new).is_ok() =>
        {
            map.insert(old.clone(), new.clone());
        }
        (Value::Object(old), Value::Object(new)) => {
            for (key, value) in old {
                if let Some(actual) = new.get(key) {
                    bindings(value, actual, map);
                }
            }
        }
        (Value::Array(old), Value::Array(new)) => {
            for (value, actual) in old.iter().zip(new) {
                bindings(value, actual, map);
            }
        }
        _ => {}
    }
}
fn substitute(value: &mut Value, map: &HashMap<String, String>) {
    match value {
        Value::String(old) => {
            if let Some(new) = map.get(old) {
                *old = new.clone();
            }
        }
        Value::Array(values) => {
            for value in values {
                substitute(value, map)
            }
        }
        Value::Object(values) => {
            for value in values.values_mut() {
                substitute(value, map)
            }
        }
        _ => {}
    }
}

#[test]
fn real_service_recordings_replay_all_five_tasks_for_both_protocols() {
    let initial: Value = serde_json::from_str(include_str!(
        "../../../docs/research/data/2026-11-05-m2a-live-initial.json"
    ))
    .unwrap();
    let rerun: Value = serde_json::from_str(include_str!(
        "../../../docs/research/data/2026-11-05-m2a-live-export-rerun.json"
    ))
    .unwrap();
    let cases = initial["cases"]
        .as_array()
        .unwrap()
        .iter()
        .chain(rerun["cases"].as_array().unwrap())
        .filter(|case| case["passed"] == true);
    let mut completed = 0;
    for case in cases {
        let task = case["task"].as_str().unwrap();
        let messages: Vec<Message> = serde_json::from_value(case["transcript"].clone()).unwrap();
        let mut editor = Editor::new();
        user(
            &mut editor,
            "file.new",
            json!({"width":16,"height":16,"bit_depth":8}),
        );
        user(
            &mut editor,
            "layer.rename",
            json!({"layer":{"name":"Background"},"name":"sample-photo"}),
        );
        if ["export", "question", "stop"].contains(&task) {
            user(&mut editor, "layer.create", json!({"name":"草稿"}));
        }
        let host = Host(Mutex::new(editor));
        let mut config:ProviderConfig=serde_json::from_value(json!({"kind":case["provider"],"endpoint":"https://recorded.test/v1","model":case["model"],"vision":false})).unwrap();
        config.allow_insecure_http = false;
        let grant =
            ContextGrant::approved(&host.snapshot().unwrap(), &config, false, None).unwrap();
        let turn = host.0.lock().unwrap().begin_turn().unwrap();
        let prompt = messages
            .iter()
            .find(|m| m.role == "user")
            .unwrap()
            .text
            .clone();
        let mut session = Session::new(
            config,
            grant,
            SessionOptions {
                mode: ExecutionMode::AskFirst,
                ..Default::default()
            },
            turn,
            &host.snapshot().unwrap(),
            prompt,
            vec![],
        )
        .unwrap();
        let mut map = HashMap::new();
        let temp = tempfile::tempdir().unwrap();
        let output = temp.path().join("recorded-export.png");
        for (index, message) in messages
            .iter()
            .enumerate()
            .filter(|(_, m)| m.role == "assistant")
        {
            let Step::Request { request_id, .. } = session.poll(&host).unwrap() else {
                panic!("expected recorded request for {task}")
            };
            let mut calls = message.tool_calls.clone();
            for call in &mut calls {
                substitute(&mut call.arguments, &map);
            }
            session
                .accept_reply(
                    &host,
                    request_id,
                    ModelReply {
                        text: message.text.clone(),
                        tool_calls: calls,
                        ..Default::default()
                    },
                )
                .unwrap();
            for call in &message.tool_calls {
                let expected = messages[index + 1..]
                    .iter()
                    .find(|m| m.role == "tool" && m.tool_call_id.as_deref() == Some(&call.id))
                    .map(|m| serde_json::from_str::<Value>(&m.text).unwrap());
                let step = session.poll(&host).unwrap();
                let actual = match step {
                    Step::Tool { result, .. } => result,
                    Step::Paused {
                        question_id,
                        reason,
                        ..
                    } => {
                        if task == "stop" {
                            session.abort(&host, "stopped").unwrap();
                            break;
                        }
                        let mut answer = HostAnswer {
                            question_id,
                            answer: expected
                                .as_ref()
                                .and_then(|v| v["answer"].as_str())
                                .unwrap_or("保留")
                                .into(),
                            approved: true,
                            resource: None,
                        };
                        if reason == "file_resource" {
                            answer.resource = Some(
                                host.0
                                    .lock()
                                    .unwrap()
                                    .grant_write(&output, false, Source::Ai { turn_id: turn })
                                    .unwrap(),
                            );
                        }
                        let Step::Tool { result, .. } = session.respond(&host, answer).unwrap()
                        else {
                            panic!("expected resumed result")
                        };
                        result
                    }
                    _ => panic!("unexpected replay state for {task}"),
                };
                assert!(
                    actual.get("error").is_none(),
                    "recorded {task} failed: {actual}"
                );
                if let Some(expected) = expected {
                    bindings(&expected, &actual, &mut map);
                }
            }
        }
        let final_state = host.snapshot().unwrap();
        assert_eq!(final_state.control, Control::User);
        match task {
            "create" => {
                assert_eq!(final_state.layers.last().unwrap().name, "草稿");
                assert_eq!(final_state.layers.last().unwrap().opacity, 0.5);
            }
            "export" => {
                assert_eq!(
                    final_state.layers.last().unwrap().blend_mode,
                    strata_core::BlendMode::Screen
                );
                assert!(output.exists());
            }
            "inspect" => assert_eq!(final_state.layers.len(), 1),
            "question" | "stop" => assert_eq!(final_state.layers.len(), 2),
            _ => panic!("unknown task"),
        }
        completed += 1;
    }
    assert_eq!(completed, 10);
}
