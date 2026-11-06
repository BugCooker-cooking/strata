use serde_json::{Value, json};
use std::sync::Mutex;
use strata_ai::{
    AiError, ContextGrant, ExecutionMode, ModelReply, ProviderConfig, ProviderKind, Session,
    SessionOptions, Step, ToolCall, ToolHost, Usage,
    session::{CanvasResult, HostAnswer},
    tools::CanvasRequest,
};
use strata_core::{
    Editor, Source,
    commands::{Control, Snapshot},
};
use uuid::Uuid;

struct Host {
    editor: Mutex<Editor>,
}
impl Host {
    fn new() -> Self {
        let mut editor = Editor::new();
        editor
            .execute(
                editor.request(Source::User),
                "file.new",
                json!({"width":8,"height":8,"bit_depth":16}),
                |_| Ok(()),
            )
            .unwrap();
        Self {
            editor: Mutex::new(editor),
        }
    }
    fn user(&self, id: &str, params: Value) -> Value {
        let mut editor = self.editor.lock().unwrap();
        let request = editor.request(Source::User);
        editor.execute(request, id, params, |_| Ok(())).unwrap()
    }
    fn session(&self, mode: ExecutionMode) -> Session {
        self.session_with_prompt(mode, "新建草稿并设置不透明度")
    }
    fn session_with_prompt(&self, mode: ExecutionMode, prompt: &str) -> Session {
        let config = config();
        let grant =
            ContextGrant::approved(&self.snapshot().unwrap(), &config, false, None).unwrap();
        let turn = self.editor.lock().unwrap().begin_turn().unwrap();
        Session::new(
            config,
            grant,
            SessionOptions {
                mode,
                ..Default::default()
            },
            turn,
            &self.snapshot().unwrap(),
            prompt.into(),
            vec![],
        )
        .unwrap()
    }
}
fn config() -> ProviderConfig {
    ProviderConfig {
        kind: ProviderKind::OpenAi,
        endpoint: "https://example.test/v1".into(),
        model: "test".into(),
        vision: false,
        allow_insecure_http: false,
    }
}
impl ToolHost for Host {
    fn snapshot(&self) -> Result<Snapshot, AiError> {
        Ok(self.editor.lock().unwrap().snapshot())
    }
    fn execute(
        &self,
        turn: Uuid,
        call: Uuid,
        id: &str,
        params: Value,
        timeout: std::time::Duration,
    ) -> Result<Value, AiError> {
        let mut editor = self.editor.lock().unwrap();
        let mut request = editor
            .request(Source::Ai { turn_id: turn })
            .with_timeout(timeout);
        request.call_id = call;
        editor
            .execute(request, id, params, |_| Ok(()))
            .map_err(|e| AiError::Host(e.to_string()))
    }
    fn pause(&self, _: Uuid) -> Result<(), AiError> {
        self.editor
            .lock()
            .unwrap()
            .pause_turn()
            .map_err(|e| AiError::Host(e.to_string()))
    }
    fn resume(&self, _: Uuid) -> Result<(), AiError> {
        self.editor
            .lock()
            .unwrap()
            .resume_turn()
            .map_err(|e| AiError::Host(e.to_string()))
    }
    fn stop(&self, turn: Uuid) -> Result<(), AiError> {
        let mut editor = self.editor.lock().unwrap();
        if matches!(editor.control(), Control::Running { turn_id } | Control::Paused { turn_id } if *turn_id == turn)
        {
            editor.stop_turn();
        }
        Ok(())
    }
    fn canvas(&self, _: &CanvasRequest, vision: bool) -> Result<CanvasResult, AiError> {
        assert!(!vision);
        Ok(CanvasResult {
            data: json!({"statistics":{"histogram":[0,1]},"image_available":false}),
            image: None,
        })
    }
}
fn call(id: &str, name: &str, arguments: Value) -> ToolCall {
    ToolCall {
        id: id.into(),
        name: name.into(),
        arguments,
    }
}
fn invoke(call_id: &str, id: &str, params: Value) -> ToolCall {
    call(call_id, "invoke_command", json!({"id":id,"params":params}))
}
fn reply(session: &mut Session, host: &Host, tools: Vec<ToolCall>) {
    let Step::Request { request_id, .. } = session.poll(host).unwrap() else {
        panic!("expected request")
    };
    session
        .accept_reply(
            host,
            request_id,
            ModelReply {
                tool_calls: tools,
                ..Default::default()
            },
        )
        .unwrap();
}
fn result(step: Step) -> Value {
    let Step::Tool { result, .. } = step else {
        panic!("expected tool")
    };
    result
}

#[test]
fn assigning_profile_through_ai_keeps_codes_and_has_one_reversible_turn() {
    let host = Host::new();
    host.user(
        "paint.fill",
        json!({"layer":{"name":"Background"},"color":[0.8,0.2,0.1,0.5]}),
    );
    let before = host
        .editor
        .lock()
        .unwrap()
        .document()
        .unwrap()
        .to_archive()
        .unwrap();
    let mut session = host.session_with_prompt(
        ExecutionMode::Direct,
        "指定 Display P3 配置文件，保留 RGB 数值，不要转换像素。",
    );
    reply(
        &mut session,
        &host,
        vec![
            call("search", "search_catalog", json!({"query":"指定配置文件"})),
            call(
                "describe",
                "describe_commands",
                json!({"ids":["color.assign"]}),
            ),
        ],
    );
    assert!(
        result(session.poll(&host).unwrap())["matches"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["id"] == "color.assign")
    );
    assert_eq!(
        result(session.poll(&host).unwrap())["commands"][0]["id"],
        "color.assign"
    );
    reply(
        &mut session,
        &host,
        vec![invoke(
            "assign",
            "color.assign",
            json!({"space":"display_p3"}),
        )],
    );
    assert_eq!(
        result(session.poll(&host).unwrap())["result"]["working_space"],
        "display_p3"
    );
    let after = host
        .editor
        .lock()
        .unwrap()
        .document()
        .unwrap()
        .to_archive()
        .unwrap();
    assert!(after.tiles == before.tiles);
    assert_ne!(
        after.metadata["working_icc"],
        before.metadata["working_icc"]
    );
    host.editor.lock().unwrap().stop_turn();
    host.user("edit.undo_turn", json!({"turn_id":session.turn_id()}));
    assert!(
        host.editor
            .lock()
            .unwrap()
            .document()
            .unwrap()
            .to_archive()
            .unwrap()
            == before
    );
}
#[test]
fn filter_prompts_discover_describe_invoke_real_edits_and_undo_the_ai_turn() {
    for (prompt, query, command, params) in [
        (
            "把当前像素图层做标准差 2px 的高斯模糊。",
            "高斯模糊",
            "filter.gaussian_blur",
            json!({"sigma":2}),
        ),
        (
            "给当前像素图层做 USM 锐化。",
            "USM",
            "filter.unsharp_mask",
            json!({"sigma":1,"amount":1.2,"threshold":0.01}),
        ),
        (
            "添加 2% 单色高斯颗粒，种子 42。",
            "颗粒",
            "filter.add_noise",
            json!({"amount":0.02,"distribution":"gaussian","monochrome":true,"seed":42}),
        ),
    ] {
        let host = Host::new();
        host.user("paint.gradient",json!({"layer":{"name":"Background"},"start":[0.5,0.5],"end":[7.5,7.5],"stops":[{"at":0,"color":[0.1,0.2,0.3,1]},{"at":1,"color":[0.8,0.7,0.6,1]}]}));
        let before = host
            .editor
            .lock()
            .unwrap()
            .document()
            .unwrap()
            .to_archive()
            .unwrap();
        let initial = host.snapshot().unwrap();
        let mut session = host.session_with_prompt(ExecutionMode::Direct, prompt);
        reply(
            &mut session,
            &host,
            vec![
                call("search", "search_catalog", json!({"query":query})),
                call("describe", "describe_commands", json!({"ids":[command]})),
            ],
        );
        assert!(
            result(session.poll(&host).unwrap())["matches"]
                .as_array()
                .unwrap()
                .iter()
                .any(|m| m["id"] == command)
        );
        assert_eq!(
            result(session.poll(&host).unwrap())["commands"][0]["id"],
            command
        );
        let mut params = params;
        params["layer"] = json!({"id":initial.active_layer});
        reply(&mut session, &host, vec![invoke("apply", command, params)]);
        let output = result(session.poll(&host).unwrap());
        assert!(
            output["result"]["touched_tiles"].as_u64().unwrap() > 0,
            "{command}: {output}"
        );
        let state = host.snapshot().unwrap();
        assert_eq!(state.history.len(), initial.history.len() + 1);
        assert_eq!(
            state.history.last().unwrap().turn_id,
            Some(session.turn_id())
        );
        assert_eq!(state.history.last().unwrap().command, command);
        host.editor.lock().unwrap().stop_turn();
        host.user("edit.undo_turn", json!({"turn_id":session.turn_id()}));
        assert!(
            host.editor
                .lock()
                .unwrap()
                .document()
                .unwrap()
                .to_archive()
                .unwrap()
                == before
        );
    }
}

#[test]
fn transform_prompt_discovers_bounds_invokes_after_exact_approval_and_undoes_turn() {
    let host = Host::new();
    host.user(
        "paint.fill",
        json!({"layer":{"name":"Background"},"color":[0.8,0.2,0.1,0.5]}),
    );
    let before = host
        .editor
        .lock()
        .unwrap()
        .document()
        .unwrap()
        .to_archive()
        .unwrap();
    let initial = host.snapshot().unwrap();
    let layer = json!({"id":initial.active_layer});
    let mut session = host.session_with_prompt(
        ExecutionMode::Direct,
        "读取当前像素层边界，将整层围绕中心旋转 90 度，包括链接蒙版并保留画板外内容。",
    );
    reply(
        &mut session,
        &host,
        vec![
            call("search", "search_catalog", json!({"query":"自由变换"})),
            call(
                "describe",
                "describe_commands",
                json!({"ids":["layer.pixel_bounds","transform.apply"]}),
            ),
            invoke("bounds", "layer.pixel_bounds", json!({"layer":layer})),
        ],
    );
    assert!(
        result(session.poll(&host).unwrap())["matches"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["id"] == "transform.apply")
    );
    let description = result(session.poll(&host).unwrap());
    assert_eq!(description["commands"].as_array().unwrap().len(), 2);
    assert_eq!(
        result(session.poll(&host).unwrap())["result"]["bounds"],
        json!({"x":0,"y":0,"width":8,"height":8})
    );
    reply(
        &mut session,
        &host,
        vec![invoke(
            "transform",
            "transform.apply",
            json!({"layer":layer,"scope":"whole_layer","geometry":{"type":"components","rotation_degrees":90,"pivot":[4,4]}}),
        )],
    );
    let Step::Paused {
        question_id,
        operation,
        ..
    } = session.poll(&host).unwrap()
    else {
        panic!("replacing pixels needs exact host approval")
    };
    assert_eq!(operation.unwrap()["command"], "transform.apply");
    assert_eq!(host.snapshot().unwrap().revision, initial.revision);
    let output = result(
        session
            .respond(
                &host,
                HostAnswer {
                    question_id,
                    answer: String::new(),
                    approved: true,
                    resource: None,
                },
            )
            .unwrap(),
    );
    assert_eq!(output["result"]["scope"], "whole_layer");
    let state = host.snapshot().unwrap();
    assert_eq!(state.history.len(), initial.history.len() + 1);
    assert_eq!(
        state.history.last().unwrap().turn_id,
        Some(session.turn_id())
    );
    assert_eq!(state.history.last().unwrap().command, "transform.apply");
    host.editor.lock().unwrap().stop_turn();
    host.user("edit.undo_turn", json!({"turn_id":session.turn_id()}));
    assert!(
        host.editor
            .lock()
            .unwrap()
            .document()
            .unwrap()
            .to_archive()
            .unwrap()
            == before
    );
}

#[test]
fn replay_create_opacity_discovery_and_turn_undo() {
    let host = Host::new();
    let mut session = host.session(ExecutionMode::Direct);
    reply(
        &mut session,
        &host,
        vec![
            call("browse", "browse_catalog", json!({})),
            call(
                "describe",
                "describe_commands",
                json!({"ids":["layer.create","layer.set_opacity"]}),
            ),
            invoke("create", "layer.create", json!({"name":"草稿"})),
        ],
    );
    assert!(
        result(session.poll(&host).unwrap())["children"]
            .as_array()
            .unwrap()
            .len()
            >= 5
    );
    assert_eq!(
        result(session.poll(&host).unwrap())["commands"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let created = result(session.poll(&host).unwrap())["result"]["layer_id"].clone();
    reply(
        &mut session,
        &host,
        vec![invoke(
            "opacity",
            "layer.set_opacity",
            json!({"layer":{"id":created},"opacity":0.5}),
        )],
    );
    result(session.poll(&host).unwrap());
    assert_eq!(host.snapshot().unwrap().layers.last().unwrap().opacity, 0.5);
    let Step::Request { request_id, .. } = session.poll(&host).unwrap() else {
        panic!()
    };
    session
        .accept_reply(
            &host,
            request_id,
            ModelReply {
                text: "已创建草稿".into(),
                usage: Usage {
                    input_tokens: 10,
                    output_tokens: 5,
                },
                ..Default::default()
            },
        )
        .unwrap();
    assert!(matches!(
        session.poll(&host).unwrap(),
        Step::Finished {
            usage: Usage {
                input_tokens: 10,
                output_tokens: 5
            },
            ..
        }
    ));
    host.user("edit.undo_turn", json!({"turn_id":session.turn_id()}));
    assert_eq!(host.snapshot().unwrap().layers.len(), 1);
}

#[test]
fn canvas_size_prompt_discovers_real_anchor_edit_and_preserves_source_for_turn_undo() {
    let host = Host::new();
    host.user(
        "paint.fill",
        json!({"layer":{"name":"Background"},"color":[0.8,0.2,0.1,0.5]}),
    );
    let before = host
        .editor
        .lock()
        .unwrap()
        .document()
        .unwrap()
        .to_archive()
        .unwrap();
    let initial = host.snapshot().unwrap();
    let mut session = host.session_with_prompt(
        ExecutionMode::Direct,
        "固定右下角，将画布扩展到 16×12 像素，不缩放或删除图层内容。",
    );
    reply(
        &mut session,
        &host,
        vec![
            call("search", "search_catalog", json!({"query":"画布大小"})),
            call(
                "describe",
                "describe_commands",
                json!({"ids":["image.canvas_size"]}),
            ),
        ],
    );
    assert!(
        result(session.poll(&host).unwrap())["matches"]
            .as_array()
            .unwrap()
            .iter()
            .any(|m| m["id"] == "image.canvas_size")
    );
    assert_eq!(
        result(session.poll(&host).unwrap())["commands"][0]["id"],
        "image.canvas_size"
    );
    reply(
        &mut session,
        &host,
        vec![invoke(
            "resize",
            "image.canvas_size",
            json!({"width":16,"height":12,"anchor":"bottom_right"}),
        )],
    );
    let answer = result(session.poll(&host).unwrap());
    assert_eq!(answer["result"]["frame"]["x"], -8);
    assert_eq!(answer["result"]["frame"]["y"], -4);
    assert_eq!(answer["result"]["preserved_external_content"], true);
    let after = host
        .editor
        .lock()
        .unwrap()
        .document()
        .unwrap()
        .to_archive()
        .unwrap();
    assert_eq!(after.tiles, before.tiles);
    assert_eq!(
        host.snapshot().unwrap().history.len(),
        initial.history.len() + 1
    );
    host.editor.lock().unwrap().stop_turn();
    host.user("edit.undo_turn", json!({"turn_id":session.turn_id()}));
    assert_eq!(
        host.editor
            .lock()
            .unwrap()
            .document()
            .unwrap()
            .to_archive()
            .unwrap(),
        before
    );
}

#[test]
fn inspect_and_direct_queries_and_batches_never_expose_hidden_layers_or_paths() {
    let host = Host::new();
    host.user(
        "layer.create",
        json!({"name":"PRIVATE /Users/alice/private.png"}),
    );
    host.user(
        "layer.set_visibility",
        json!({"layer":{"name":"PRIVATE /Users/alice/private.png"},"visible":false}),
    );
    let before = host.snapshot().unwrap().revision;
    let mut session = host.session(ExecutionMode::Direct);
    reply(
        &mut session,
        &host,
        vec![
            call("inspect", "inspect_document", json!({})),
            invoke("query", "document.inspect", json!({})),
            call(
                "batch",
                "invoke_batch",
                json!({"steps":[{"command":"layer.list","params":{}}]}),
            ),
        ],
    );
    for _ in 0..3 {
        let data = result(session.poll(&host).unwrap()).to_string();
        assert!(!data.contains("PRIVATE"));
        assert!(!data.contains("/Users/"));
    }
    // An all-query atomic batch currently creates one history item by design.
    assert_eq!(host.snapshot().unwrap().revision, before + 1);
}

#[test]
fn hidden_groups_do_not_disclose_descendant_names_in_any_inspect_entry() {
    let host = Host::new();
    host.user("layer.create", json!({"name":"PRIVATE child"}));
    let group = host.user(
        "layer.group",
        json!({"layers":[{"name":"PRIVATE child"}],"name":"PRIVATE group"}),
    )["layer_id"]
        .clone();
    host.user(
        "layer.set_visibility",
        json!({"layer":{"id":group},"visible":false}),
    );
    let mut session = host.session(ExecutionMode::Direct);
    reply(
        &mut session,
        &host,
        vec![
            call("inspect", "inspect_document", json!({})),
            invoke("query", "layer.list", json!({})),
        ],
    );
    for _ in 0..2 {
        assert!(
            !result(session.poll(&host).unwrap())
                .to_string()
                .contains("PRIVATE")
        );
    }
}

#[test]
fn clipboard_cut_pauses_for_host_write_and_never_clears_before_acknowledgement() {
    let host = Host::new();
    host.user(
        "layer.create",
        json!({"name":"filled","fill":[32768,32769,50001,65535]}),
    );
    let before = host
        .editor
        .lock()
        .unwrap()
        .document()
        .unwrap()
        .export_pixels()
        .unwrap();
    let revision = host.snapshot().unwrap().revision;
    let mut session = host.session(ExecutionMode::Direct);
    reply(
        &mut session,
        &host,
        vec![invoke("cut", "edit.cut", json!({}))],
    );
    let Step::Paused {
        question_id,
        reason,
        ..
    } = session.poll(&host).unwrap()
    else {
        panic!()
    };
    assert_eq!(reason, "clipboard_access");
    let Step::Paused {
        question_id: write_id,
        reason,
        operation,
        ..
    } = session
        .respond(
            &host,
            HostAnswer {
                question_id,
                answer: String::new(),
                approved: true,
                resource: None,
            },
        )
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(reason, "clipboard_write");
    assert_eq!(host.snapshot().unwrap().revision, revision);
    let token = operation.unwrap()["resource"].as_str().unwrap().to_owned();
    host.editor
        .lock()
        .unwrap()
        .confirm_clipboard_write(
            &token,
            &Source::Ai {
                turn_id: session.turn_id(),
            },
        )
        .unwrap();
    let result = result(
        session
            .respond(
                &host,
                HostAnswer {
                    question_id: write_id,
                    answer: String::new(),
                    approved: true,
                    resource: Some(token),
                },
            )
            .unwrap(),
    );
    assert!(result.get("error").is_none());
    assert_eq!(host.snapshot().unwrap().revision, revision + 1);
    session.abort(&host, "stopped").unwrap();
    host.user("edit.undo", json!({}));
    assert_eq!(
        host.editor
            .lock()
            .unwrap()
            .document()
            .unwrap()
            .export_pixels()
            .unwrap(),
        before
    );
}
#[test]
fn clipboard_write_failure_leaves_source_and_returns_truthful_status() {
    let host = Host::new();
    let revision = host.snapshot().unwrap().revision;
    let mut session = host.session(ExecutionMode::Direct);
    reply(
        &mut session,
        &host,
        vec![invoke("cut", "edit.cut", json!({}))],
    );
    let Step::Paused { question_id, .. } = session.poll(&host).unwrap() else {
        panic!()
    };
    let Step::Paused { question_id, .. } = session
        .respond(
            &host,
            HostAnswer {
                question_id,
                answer: String::new(),
                approved: true,
                resource: None,
            },
        )
        .unwrap()
    else {
        panic!()
    };
    let result = result(
        session
            .respond(
                &host,
                HostAnswer {
                    question_id,
                    answer: String::new(),
                    approved: false,
                    resource: None,
                },
            )
            .unwrap(),
    );
    assert_eq!(result["status"], "clipboard_write_failed");
    assert_eq!(host.snapshot().unwrap().revision, revision);
}

#[test]
fn ask_first_is_core_enforced_and_question_does_not_implicitly_approve_edit() {
    let host = Host::new();
    let mut session = host.session(ExecutionMode::AskFirst);
    reply(
        &mut session,
        &host,
        vec![
            call(
                "question",
                "ask_user",
                json!({"question":"要继续吗？","options":["是","否"]}),
            ),
            invoke("create", "layer.create", json!({"name":"草稿"})),
        ],
    );
    let Step::Paused {
        question_id,
        reason,
        ..
    } = session.poll(&host).unwrap()
    else {
        panic!()
    };
    assert_eq!(reason, "question");
    host.user(
        "layer.set_visibility",
        json!({"layer":{"name":"Background"},"visible":false}),
    );
    let data = result(
        session
            .respond(
                &host,
                HostAnswer {
                    question_id,
                    answer: "是".into(),
                    approved: true,
                    resource: None,
                },
            )
            .unwrap(),
    );
    assert_eq!(data["state_after_pause"]["hidden_layer_count"], 1);
    let Step::Paused {
        question_id,
        reason,
        ..
    } = session.poll(&host).unwrap()
    else {
        panic!()
    };
    assert_eq!(reason, "edit_approval");
    assert_eq!(host.snapshot().unwrap().layers.len(), 1);
    session
        .respond(
            &host,
            HostAnswer {
                question_id,
                answer: String::new(),
                approved: true,
                resource: None,
            },
        )
        .unwrap();
    assert_eq!(host.snapshot().unwrap().layers.len(), 2);
    assert!(!host.snapshot().unwrap().layers[0].visible);
}

#[test]
fn destructive_edit_requires_exact_approval_even_in_direct_mode() {
    let host = Host::new();
    let mut session = host.session(ExecutionMode::Direct);
    reply(
        &mut session,
        &host,
        vec![invoke(
            "delete",
            "layer.delete",
            json!({"layer":{"name":"Background"}}),
        )],
    );
    let Step::Paused { question_id, .. } = session.poll(&host).unwrap() else {
        panic!()
    };
    session
        .respond(
            &host,
            HostAnswer {
                question_id,
                answer: String::new(),
                approved: false,
                resource: None,
            },
        )
        .unwrap();
    assert_eq!(host.snapshot().unwrap().layers.len(), 1);
}

#[test]
fn ambiguous_subject_flag_always_requests_exact_host_confirmation() {
    let host = Host::new();
    let mut session = host.session(ExecutionMode::Direct);
    reply(
        &mut session,
        &host,
        vec![invoke(
            "ambiguous",
            "select.subject",
            json!({"accept_ambiguous":true}),
        )],
    );
    let Step::Paused {
        reason,
        question,
        question_id,
        ..
    } = session.poll(&host).unwrap()
    else {
        panic!()
    };
    assert_eq!(reason, "edit_approval");
    assert!(question.contains("主体"));
    session
        .respond(
            &host,
            HostAnswer {
                question_id,
                answer: String::new(),
                approved: false,
                resource: None,
            },
        )
        .unwrap();
    assert!(host.snapshot().unwrap().selection.is_none());
}

#[test]
fn stop_discards_pending_delete_and_late_responses_and_releases_lock() {
    let host = Host::new();
    let mut session = host.session(ExecutionMode::Direct);
    reply(
        &mut session,
        &host,
        vec![
            invoke("create", "layer.create", json!({"name":"keep committed"})),
            invoke(
                "delete",
                "layer.delete",
                json!({"layer":{"name":"Background"}}),
            ),
        ],
    );
    session.poll(&host).unwrap();
    host.stop(session.turn_id()).unwrap();
    assert!(matches!(session.poll(&host), Err(AiError::Stopped)));
    assert_eq!(host.snapshot().unwrap().control, Control::User);
    assert_eq!(host.snapshot().unwrap().layers.len(), 2);
    let mut next = host.session(ExecutionMode::Direct);
    let Step::Request { request_id, .. } = next.poll(&host).unwrap() else {
        panic!()
    };
    host.stop(next.turn_id()).unwrap();
    assert!(
        next.accept_reply(
            &host,
            request_id,
            ModelReply {
                tool_calls: vec![invoke(
                    "late",
                    "layer.delete",
                    json!({"layer":{"name":"Background"}})
                )],
                ..Default::default()
            }
        )
        .is_err()
    );
    assert_eq!(host.snapshot().unwrap().layers.len(), 2);
}

#[test]
fn duplicate_calls_are_idempotent_and_conflicts_return_actionable_errors() {
    let host = Host::new();
    let mut session = host.session(ExecutionMode::Direct);
    let create = invoke("same", "layer.create", json!({"name":"once"}));
    reply(&mut session, &host, vec![create.clone()]);
    let first = result(session.poll(&host).unwrap());
    reply(&mut session, &host, vec![create]);
    assert_eq!(result(session.poll(&host).unwrap()), first);
    reply(
        &mut session,
        &host,
        vec![invoke("same", "layer.create", json!({"name":"different"}))],
    );
    assert_eq!(
        result(session.poll(&host).unwrap())["error"],
        "tool_call_conflict"
    );
    assert_eq!(host.snapshot().unwrap().layers.len(), 2);
}

#[test]
fn external_write_requires_host_resource_and_cannot_be_batched_or_undone() {
    let host = Host::new();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("allowed.png");
    let mut session = host.session(ExecutionMode::Direct);
    reply(
        &mut session,
        &host,
        vec![
            invoke(
                "screen",
                "layer.set_blend_mode",
                json!({"layer":{"name":"Background"},"blend_mode":"screen"}),
            ),
            call(
                "badbatch",
                "invoke_batch",
                json!({"steps":[{"command":"file.export","params":{"resource":"guessed","format":"png"}}]}),
            ),
            invoke(
                "export",
                "file.export",
                json!({"resource":"/tmp/not-a-grant.png","format":"png"}),
            ),
        ],
    );
    session.poll(&host).unwrap();
    assert!(result(session.poll(&host).unwrap()).get("error").is_some());
    let Step::Paused {
        question_id,
        reason,
        operation,
        ..
    } = session.poll(&host).unwrap()
    else {
        panic!()
    };
    assert_eq!(reason, "file_resource");
    assert!(operation.unwrap()["params"].get("resource").is_none());
    assert!(!path.exists());
    let resource = host
        .editor
        .lock()
        .unwrap()
        .grant_write(
            &path,
            false,
            Source::Ai {
                turn_id: session.turn_id(),
            },
        )
        .unwrap();
    session
        .respond(
            &host,
            HostAnswer {
                question_id,
                answer: String::new(),
                approved: true,
                resource: Some(resource),
            },
        )
        .unwrap();
    assert!(path.exists());
    session.abort(&host, "stopped").unwrap();
    host.user("edit.undo_turn", json!({"turn_id":session.turn_id()}));
    assert!(path.exists());
    assert!(matches!(
        session.poll(&host).unwrap(),
        Step::Finished {
            external_writes: 1,
            ..
        }
    ));
}

#[test]
fn model_cannot_forge_source_approval_or_extra_tool_parameters() {
    let host = Host::new();
    let mut session = host.session(ExecutionMode::Direct);
    reply(
        &mut session,
        &host,
        vec![
            call(
                "forged",
                "invoke_command",
                json!({"id":"layer.create","params":{"name":"bad"},"source":"user"}),
            ),
            call("inspect", "inspect_document", json!({"approved":true})),
            invoke(
                "replace",
                "file.new",
                json!({"width":2,"height":2,"bit_depth":8,"discard_changes":true}),
            ),
        ],
    );
    for _ in 0..2 {
        assert!(result(session.poll(&host).unwrap()).get("error").is_some());
    }
    let Step::Paused {
        question_id,
        reason,
        ..
    } = session.poll(&host).unwrap()
    else {
        panic!()
    };
    assert_eq!(reason, "lifecycle_approval");
    session
        .respond(
            &host,
            HostAnswer {
                question_id,
                answer: String::new(),
                approved: false,
                resource: None,
            },
        )
        .unwrap();
    assert_eq!(host.snapshot().unwrap().layers.len(), 1);
}

#[test]
fn document_replacement_requires_new_context_approval_and_rebinds_only_after_success() {
    let host = Host::new();
    let before = host.snapshot().unwrap().document_id;
    let mut session = host.session(ExecutionMode::AskFirst);
    reply(
        &mut session,
        &host,
        vec![invoke(
            "new",
            "file.new",
            json!({"width":3,"height":5,"bit_depth":8,"discard_changes":true}),
        )],
    );
    let Step::Paused {
        question_id,
        reason,
        question,
        ..
    } = session.poll(&host).unwrap()
    else {
        panic!()
    };
    assert_eq!(reason, "lifecycle_approval");
    assert!(question.contains("example.test"));
    assert_eq!(host.snapshot().unwrap().document_id, before);
    session
        .respond(
            &host,
            HostAnswer {
                question_id,
                answer: String::new(),
                approved: true,
                resource: None,
            },
        )
        .unwrap();
    assert_ne!(host.snapshot().unwrap().document_id, before);
    assert_eq!(host.snapshot().unwrap().bit_depth, Some(8));
    reply(
        &mut session,
        &host,
        vec![invoke(
            "layer",
            "layer.create",
            json!({"name":"new document layer"}),
        )],
    );
    let Step::Paused { reason, .. } = session.poll(&host).unwrap() else {
        panic!()
    };
    assert_eq!(reason, "edit_approval");
}

#[test]
fn bounded_retry_and_step_limit_release_control_without_replaying_commands() {
    let host = Host::new();
    let mut session = host.session(ExecutionMode::Direct);
    for _ in 0..3 {
        let Step::Request { request_id, .. } = session.poll(&host).unwrap() else {
            panic!()
        };
        session.request_failed(&host, request_id, true).unwrap();
    }
    assert!(
        matches!(session.poll(&host).unwrap(),Step::Finished{status,..} if status=="network_error")
    );
    assert_eq!(host.snapshot().unwrap().control, Control::User);
    let grant = ContextGrant::approved(&host.snapshot().unwrap(), &config(), false, None).unwrap();
    let turn = host.editor.lock().unwrap().begin_turn().unwrap();
    let mut limited = Session::new(
        config(),
        grant,
        SessionOptions {
            max_steps: 1,
            mode: ExecutionMode::Direct,
            ..Default::default()
        },
        turn,
        &host.snapshot().unwrap(),
        "test".into(),
        vec![],
    )
    .unwrap();
    reply(
        &mut limited,
        &host,
        vec![
            invoke("1", "layer.create", json!({"name":"once"})),
            invoke("2", "layer.create", json!({"name":"never"})),
        ],
    );
    limited.poll(&host).unwrap();
    assert!(matches!(limited.poll(&host), Err(AiError::Limit)));
    assert_eq!(host.snapshot().unwrap().control, Control::User);
    assert_eq!(host.snapshot().unwrap().layers.len(), 2);
}

#[test]
fn grant_is_bound_to_destination_document_and_context_scope() {
    let host = Host::new();
    let grant = ContextGrant::approved(&host.snapshot().unwrap(), &config(), false, None).unwrap();
    let turn = host.editor.lock().unwrap().begin_turn().unwrap();
    let mut changed = config();
    changed.endpoint = "https://different.test/v1".into();
    assert!(matches!(
        Session::new(
            changed,
            grant,
            SessionOptions::default(),
            turn,
            &host.snapshot().unwrap(),
            "test".into(),
            vec![]
        ),
        Err(AiError::Permission)
    ));
    host.stop(turn).unwrap();
    let mut session = host.session(ExecutionMode::Direct);
    reply(
        &mut session,
        &host,
        vec![call("view", "view_canvas", json!({"max_size":64}))],
    );
    assert_eq!(
        result(session.poll(&host).unwrap())["image_available"],
        false
    );
    reply(
        &mut session,
        &host,
        vec![call("hidden", "view_canvas", json!({"layers":["secret"]}))],
    );
    assert!(result(session.poll(&host).unwrap()).get("error").is_some());
}

#[test]
fn malformed_or_truncated_stream_stops_without_committing_partial_tools() {
    let host = Host::new();
    let mut session = host.session(ExecutionMode::Direct);
    let Step::Request { request_id, .. } = session.poll(&host).unwrap() else {
        panic!()
    };
    session.feed(request_id,b"data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"bad\",\"function\":{\"name\":\"invoke_command\",\"arguments\":\"{\"}}]}}]}\n\n").unwrap();
    assert!(session.finish_response(&host, request_id).is_err());
    assert_eq!(host.snapshot().unwrap().control, Control::User);
    assert_eq!(host.snapshot().unwrap().layers.len(), 1);
}
