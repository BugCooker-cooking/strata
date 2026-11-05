use serde_json::json;
use strata_ai::{
    ProviderConfig, ProviderKind,
    provider::{Message, StreamDecoder, encode},
};

fn event(v: serde_json::Value) -> String {
    format!("data: {v}\r\n\r\n")
}
#[test]
fn openai_utf8_and_tool_arguments_survive_every_byte_boundary() {
    let stream = event(
        json!({"choices":[{"index":0,"delta":{"content":"创建草稿"},"finish_reason":null}]}),
    ) + &event(
        json!({"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"a","function":{"name":"invoke_command","arguments":"{\"id\":\"layer.create\","}}]}}]}),
    ) + &event(
        json!({"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"params\":{\"name\":\"草稿\"}}"}}]},"finish_reason":"tool_calls"}]}),
    ) + &event(
        json!({"choices":[],"usage":{"prompt_tokens":100,"completion_tokens":20}}),
    ) + "data: [DONE]\r\n\r\n";
    for chunk_size in [1, 2, 3, 7, 31, 1024] {
        let mut decoder = StreamDecoder::new(ProviderKind::OpenAi);
        let mut text = String::new();
        for part in stream.as_bytes().chunks(chunk_size) {
            text.push_str(&decoder.feed(part).unwrap());
        }
        let reply = decoder.finish().unwrap();
        assert_eq!(text, "创建草稿");
        assert_eq!(reply.text, text);
        assert_eq!(
            reply.tool_calls[0].arguments,
            json!({"id":"layer.create","params":{"name":"草稿"}})
        );
        assert_eq!(reply.usage.input_tokens, 100);
        assert_eq!(reply.usage.output_tokens, 20);
    }
}
#[test]
fn anthropic_multi_tool_stream_and_usage() {
    let stream = event(json!({"type":"message_start","message":{"usage":{"input_tokens":12}}}))
        + &event(
            json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
        )
        + &event(
            json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"查看"}}),
        )
        + &event(
            json!({"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"one","name":"inspect_document","input":{}}}),
        )
        + &event(
            json!({"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{}"}}),
        )
        + &event(
            json!({"type":"content_block_start","index":2,"content_block":{"type":"tool_use","id":"two","name":"browse_catalog","input":{}}}),
        )
        + &event(
            json!({"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":9}}),
        )
        + &event(json!({"type":"message_stop"}));
    let mut d = StreamDecoder::new(ProviderKind::Anthropic);
    for byte in stream.bytes() {
        d.feed(&[byte]).unwrap();
    }
    let r = d.finish().unwrap();
    assert_eq!(r.text, "查看");
    assert_eq!(r.tool_calls.len(), 2);
    assert_eq!(r.usage.output_tokens, 9);
}
#[test]
fn invalid_json_duplicate_ids_and_length_finish_never_yield_tools() {
    for finish in ["length", "content_filter", ""] {
        let mut d = StreamDecoder::new(ProviderKind::OpenAi);
        d.feed(
            (event(json!({"choices":[{"delta":{},"finish_reason":finish}]})) + "data: [DONE]\n\n")
                .as_bytes(),
        )
        .unwrap();
        assert!(d.finish().is_err());
    }
    let mut d = StreamDecoder::new(ProviderKind::OpenAi);
    assert!(d.feed(b"data: {bad}\n\n").is_err());
    let mut d = StreamDecoder::new(ProviderKind::OpenAi);
    d.feed(
        serde_json::to_string(
            &json!({"choices":[{"finish_reason":"tool_calls","message":{"tool_calls":[
                {"id":"same","function":{"name":"inspect_document","arguments":"{}"}},
                {"id":"same","function":{"name":"inspect_document","arguments":"{}"}}
            ]}}]}),
        )
        .unwrap()
        .as_bytes(),
    )
    .unwrap();
    assert!(d.finish().is_err());
}
#[test]
fn non_streaming_compatible_json_preserves_pretty_printed_data() {
    let bytes=serde_json::to_vec_pretty(&json!({"choices":[{"finish_reason":"stop","message":{"content":"done"}}],"usage":{"prompt_tokens":5}})).unwrap();
    let mut d = StreamDecoder::new(ProviderKind::OpenAi);
    for part in bytes.chunks(3) {
        d.feed(part).unwrap();
    }
    let reply = d.finish().unwrap();
    assert_eq!(reply.text, "done");
    assert_eq!(reply.usage.input_tokens, 5);
}
#[test]
fn endpoint_restrictions_and_fixed_eight_tools() {
    let mut c = ProviderConfig {
        kind: ProviderKind::OpenAi,
        endpoint: "https://example.test/v1/".into(),
        model: "test".into(),
        vision: false,
        allow_insecure_http: false,
    };
    assert_eq!(
        c.request_url().unwrap(),
        "https://example.test/v1/chat/completions"
    );
    let request = encode(&c, &[Message::text("user", "test")]).unwrap();
    assert_eq!(request.body["tools"].as_array().unwrap().len(), 8);
    for endpoint in [
        "http://example.test/v1",
        "https://key:secret@example.test/v1",
        "https://example.test/v1?key=secret",
        "https://example.test/v1#secret",
        "file:///tmp/test",
        "http://127.0.0.1.evil.test/v1",
    ] {
        c.endpoint = endpoint.into();
        assert!(c.request_url().is_err(), "{endpoint}");
    }
    c.kind = ProviderKind::Local;
    for endpoint in [
        "http://127.0.0.1:11434/v1",
        "http://localhost:1234/v1",
        "http://[::1]:1234/v1",
    ] {
        c.endpoint = endpoint.into();
        assert!(c.request_url().is_ok(), "{endpoint}");
    }
    c.endpoint = "https://example.test/v1".into();
    assert!(c.request_url().is_err());
    c.kind = ProviderKind::OpenAi;
    c.endpoint = "http://example.test:8080/v1".into();
    assert!(c.request_url().is_err());
    c.allow_insecure_http = true;
    assert_eq!(
        c.request_url().unwrap(),
        "http://example.test:8080/v1/chat/completions"
    );
    let models = strata_ai::provider::models_request(&c).unwrap();
    assert_eq!(models.url, "http://example.test:8080/v1/models");
    assert_eq!(models.method, "GET");
    assert!(models.body.is_null());
}

#[test]
fn catalog_is_lazy_bilingual_bounded_and_has_no_unimplemented_commands() {
    use strata_ai::catalog;
    let root = catalog::browse(None).unwrap();
    assert!(!root.to_string().contains("layer.create"));
    assert!(
        catalog::browse(Some("layer"))
            .unwrap()
            .to_string()
            .contains("layer.create")
    );
    assert!(
        root["children"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["path"] == "filter")
    );
    assert!(
        catalog::search("图层", 3).unwrap()["matches"]
            .as_array()
            .unwrap()
            .len()
            <= 3
    );
    assert!(catalog::search("", 1).is_err());
    assert!(catalog::search("layer", 100).is_err());
    for (query, command) in [
        ("吸管", "paint.sample_color"),
        ("油漆桶", "paint.bucket"),
        ("菱形", "paint.gradient"),
        ("蒙版绘制", "paint.stroke"),
        ("仿制图章", "retouch.clone"),
        ("修复画笔", "retouch.heal"),
        ("污点修复", "retouch.spot_heal"),
        ("减淡", "retouch.dodge"),
        ("加深", "retouch.burn"),
        ("海绵", "retouch.sponge"),
        ("高斯模糊", "filter.gaussian_blur"),
        ("USM", "filter.unsharp_mask"),
        ("颗粒", "filter.add_noise"),
    ] {
        assert!(
            catalog::search(query, 32).unwrap()["matches"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v["id"] == command),
            "missing paint search {query}"
        );
    }
    for query in [
        "色阶",
        "曲线",
        "阴影",
        "高光",
        "自然饱和度",
        "色温",
        "色彩平衡",
        "黑白",
        "照片滤镜",
        "通道混合器",
        "色调分离",
        "渐变映射",
        "可选颜色",
        "去色",
    ] {
        let matches = catalog::search(query, 32).unwrap();
        assert!(
            matches["matches"]
                .as_array()
                .unwrap()
                .iter()
                .any(|m| m["id"] == "layer.add_tonal_adjustment"),
            "missing Chinese tonal discovery: {query}"
        );
    }
    assert!(catalog::describe(&[]).is_err());
    assert!(catalog::browse(Some("unknown")).is_err());
    assert!(catalog::describe(&["type.create".into()]).is_err());
    let export = catalog::describe(&["file.export".into()]).unwrap();
    assert!(
        export["commands"][0]["host_resource_workflow"]
            .as_str()
            .unwrap()
            .contains("omit params.resource")
    );
    assert!(
        !export["commands"][0]["params"]["required"]
            .as_array()
            .unwrap()
            .contains(&json!("resource"))
    );
}

#[test]
fn image_results_follow_all_parallel_tool_results_in_openai_and_stay_inside_anthropic_result() {
    use strata_ai::{ModelReply, ToolCall, provider::ImageContent};
    let calls = vec![
        ToolCall {
            id: "view".into(),
            name: "view_canvas".into(),
            arguments: json!({}),
        },
        ToolCall {
            id: "inspect".into(),
            name: "inspect_document".into(),
            arguments: json!({}),
        },
    ];
    let reply = ModelReply {
        tool_calls: calls,
        ..Default::default()
    };
    let messages = vec![
        Message::text("user", "inspect"),
        Message::assistant(&reply),
        Message::tool(
            "view",
            &json!({"image_available":true}),
            Some(ImageContent {
                mime: "image/png".into(),
                base64: "fixture".into(),
            }),
        ),
        Message::tool("inspect", &json!({"layers":[]}), None),
    ];
    let mut config = ProviderConfig {
        kind: ProviderKind::OpenAi,
        endpoint: "https://example.test/v1".into(),
        model: "test".into(),
        vision: true,
        allow_insecure_http: false,
    };
    let body = encode(&config, &messages).unwrap().body;
    let wire = body["messages"].as_array().unwrap();
    assert_eq!(
        wire.iter()
            .map(|m| m["role"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["user", "assistant", "tool", "tool", "user"]
    );
    assert_eq!(wire[2]["tool_call_id"], "view");
    assert_eq!(wire[3]["tool_call_id"], "inspect");
    config.kind = ProviderKind::Anthropic;
    let body = encode(&config, &messages).unwrap().body;
    let result = &body["messages"][2]["content"];
    assert_eq!(result.as_array().unwrap().len(), 2);
    assert_eq!(result[0]["type"], "tool_result");
    assert_eq!(result[0]["content"][1]["type"], "image");
    assert_eq!(result[1]["tool_use_id"], "inspect");
}
