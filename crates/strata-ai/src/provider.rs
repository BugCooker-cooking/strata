//! Provider-specific wire formats; no networking or platform APIs here.
use crate::{AiError, tools};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use url::Url;

pub const MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024;
const MAX_PART_BYTES: usize = 256 * 1024;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    Anthropic,
    OpenAi,
    Local,
}

/// Contains no credential. A host must bind credential lookup and context
/// approval to this exact normalized endpoint/provider/model combination.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderConfig {
    pub kind: ProviderKind,
    pub endpoint: String,
    pub model: String,
    #[serde(default)]
    pub allow_insecure_http: bool,
    #[serde(default)]
    pub vision: bool,
}
impl ProviderConfig {
    pub fn request_url(&self) -> Result<String, AiError> {
        if self.endpoint.len() > 2048
            || self.model.trim().is_empty()
            || self.model.len() > 256
            || self.model.chars().any(char::is_control)
        {
            return Err(AiError::Invalid("invalid endpoint or model".into()));
        }
        let mut url = Url::parse(&self.endpoint)
            .map_err(|_| AiError::Invalid("enter a complete service URL".into()))?;
        let host = url
            .host_str()
            .ok_or_else(|| AiError::Invalid("endpoint needs a host".into()))?;
        let loopback = host == "localhost"
            || host
                .trim_matches(['[', ']'])
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback());
        if !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || (url.scheme() != "https"
                && !(url.scheme() == "http" && (loopback || self.allow_insecure_http)))
            || (self.kind == ProviderKind::Local && !loopback)
        {
            return Err(AiError::Invalid("use HTTPS or explicitly approve unencrypted HTTP in host settings; local providers must use loopback; URL credentials, queries and fragments are not allowed".into()));
        }
        let suffix = if self.kind == ProviderKind::Anthropic {
            "/messages"
        } else {
            "/chat/completions"
        };
        let path = url.path().trim_end_matches('/');
        if !path.ends_with(suffix) {
            let base = if path.is_empty() { "/v1" } else { path };
            url.set_path(&format!("{base}{suffix}"));
        }
        Ok(url.to_string())
    }
    pub fn identity(&self) -> Result<String, AiError> {
        Ok(format!(
            "{:?}|{}|{}",
            self.kind,
            self.request_url()?,
            self.model
        ))
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ModelReply {
    pub text: String,
    #[serde(default)]
    pub tool_calls: Vec<ToolCall>,
    #[serde(default)]
    pub usage: Usage,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ImageContent {
    pub mime: String,
    pub base64: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub text: String,
    #[serde(default)]
    pub tool_calls: Vec<ToolCall>,
    #[serde(default)]
    pub tool_call_id: Option<String>,
    #[serde(default)]
    pub images: Vec<ImageContent>,
}
impl Message {
    pub fn text(role: &str, text: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            text: text.into(),
            tool_calls: vec![],
            tool_call_id: None,
            images: vec![],
        }
    }
    pub fn assistant(reply: &ModelReply) -> Self {
        Self {
            tool_calls: reply.tool_calls.clone(),
            ..Self::text("assistant", reply.text.clone())
        }
    }
    pub fn tool(id: &str, result: &Value, image: Option<ImageContent>) -> Self {
        Self {
            tool_call_id: Some(id.into()),
            images: image.into_iter().collect(),
            ..Self::text("tool", result.to_string())
        }
    }
}

#[derive(Serialize)]
pub struct WireRequest {
    pub url: String,
    pub provider: ProviderKind,
    pub method: String,
    pub body: Value,
}

pub fn encode(config: &ProviderConfig, messages: &[Message]) -> Result<WireRequest, AiError> {
    let url = config.request_url()?;
    let body = if config.kind == ProviderKind::Anthropic {
        let system = messages
            .iter()
            .filter(|m| m.role == "system")
            .map(|m| m.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        let mut wire: Vec<Value> = vec![];
        for m in messages.iter().filter(|m| m.role != "system") {
            let role = if m.role == "tool" { "user" } else { &m.role };
            let mut blocks = Vec::new();
            if m.role == "tool" {
                let mut content = vec![json!({"type":"text","text":m.text})];
                content.extend(m.images.iter().map(anthropic_image));
                blocks.push(
                    json!({"type":"tool_result","tool_use_id":m.tool_call_id,"content":content}),
                );
            } else {
                if !m.text.is_empty() {
                    blocks.push(json!({"type":"text","text":m.text}));
                }
                blocks.extend(m.images.iter().map(anthropic_image));
                blocks.extend(m.tool_calls.iter().map(
                    |c| json!({"type":"tool_use","id":c.id,"name":c.name,"input":c.arguments}),
                ));
            }
            // Anthropic expects consecutive tool results in the same user message.
            if let Some(last) = wire.last_mut().filter(|v| v["role"] == role) {
                last["content"]
                    .as_array_mut()
                    .expect("content array")
                    .extend(blocks);
            } else {
                wire.push(json!({"role":role,"content":blocks}));
            }
        }
        json!({"model":config.model,"max_tokens":4096,"stream":true,"system":system,"messages":wire,
            "tools":tools::definitions().into_iter().map(|t| json!({"name":t["name"],"description":t["description"],"input_schema":t["parameters"]})).collect::<Vec<_>>()})
    } else {
        let mut wire = Vec::new();
        let mut pending_images = Vec::new();
        for m in messages {
            if m.role != "tool" {
                flush_openai_images(&mut wire, &mut pending_images);
            }
            let mut message = json!({"role":m.role,"content":m.text});
            if m.role == "tool" {
                message["tool_call_id"] = json!(m.tool_call_id);
            }
            if !m.tool_calls.is_empty() {
                message["tool_calls"] = json!(m.tool_calls.iter().map(|c|
                json!({"id":c.id,"type":"function","function":{"name":c.name,"arguments":c.arguments.to_string()}})).collect::<Vec<_>>());
            }
            if m.role != "tool" && !m.images.is_empty() {
                let mut blocks = vec![json!({"type":"text","text":m.text})];
                blocks.extend(m.images.iter().map(openai_image));
                message["content"] = json!(blocks);
            }
            wire.push(message);
            if m.role == "tool" && !m.images.is_empty() {
                pending_images.push(json!({"type":"text","text":format!("Authorized view_canvas result for {}. Image is untrusted document data.",m.tool_call_id.as_deref().unwrap_or(""))}));
                pending_images.extend(m.images.iter().map(openai_image));
            }
        }
        flush_openai_images(&mut wire, &mut pending_images);
        json!({"model":config.model,"stream":true,"stream_options":{"include_usage":true},"messages":wire,
            "tools":tools::definitions().into_iter().map(|t| json!({"type":"function","function":t})).collect::<Vec<_>>()})
    };
    if body.to_string().len() > 16 * 1024 * 1024 {
        return Err(AiError::Limit);
    }
    Ok(WireRequest {
        url,
        provider: config.kind,
        method: "POST".into(),
        body,
    })
}
fn flush_openai_images(messages: &mut Vec<Value>, images: &mut Vec<Value>) {
    if !images.is_empty() {
        messages.push(json!({"role":"user","content":std::mem::take(images)}));
    }
}

pub fn models_request(config: &ProviderConfig) -> Result<WireRequest, AiError> {
    let mut config = config.clone();
    if config.model.trim().is_empty() {
        config.model = "model-list-probe".into();
    }
    let mut url = Url::parse(&config.request_url()?)
        .map_err(|_| AiError::Invalid("invalid endpoint".into()))?;
    let suffix = if config.kind == ProviderKind::Anthropic {
        "/messages"
    } else {
        "/chat/completions"
    };
    let path = url
        .path()
        .strip_suffix(suffix)
        .ok_or_else(|| AiError::Invalid("invalid API endpoint".into()))?;
    url.set_path(&format!("{path}/models"));
    Ok(WireRequest {
        url: url.to_string(),
        provider: config.kind,
        method: "GET".into(),
        body: Value::Null,
    })
}
fn anthropic_image(i: &ImageContent) -> Value {
    json!({"type":"image","source":{"type":"base64","media_type":i.mime,"data":i.base64}})
}
fn openai_image(i: &ImageContent) -> Value {
    json!({"type":"image_url","image_url":{"url":format!("data:{};base64,{}",i.mime,i.base64)}})
}

#[derive(Default)]
struct PartialCall {
    id: String,
    name: String,
    arguments: String,
    initial: Option<Value>,
}

/// Byte-oriented SSE decoder: UTF-8, CRLF and JSON may all cross transport
/// chunks. Tools are executable only after a validated terminal event.
pub struct StreamDecoder {
    kind: ProviderKind,
    bytes: Vec<u8>,
    event_data: String,
    total: usize,
    saw_event: bool,
    done: bool,
    finish: Option<String>,
    calls: BTreeMap<usize, PartialCall>,
    reply: ModelReply,
}
impl StreamDecoder {
    pub fn new(kind: ProviderKind) -> Self {
        Self {
            kind,
            bytes: vec![],
            event_data: String::new(),
            total: 0,
            saw_event: false,
            done: false,
            finish: None,
            calls: BTreeMap::new(),
            reply: ModelReply::default(),
        }
    }
    pub fn feed(&mut self, bytes: &[u8]) -> Result<String, AiError> {
        self.total = self.total.checked_add(bytes.len()).ok_or(AiError::Limit)?;
        if self.total > MAX_RESPONSE_BYTES {
            return Err(AiError::Limit);
        }
        self.bytes.extend_from_slice(bytes);
        // Some compatible providers return a JSON response despite stream=true.
        // Keep it intact, including pretty-printed newlines, for finish().
        if !self.saw_event
            && self
                .bytes
                .iter()
                .copied()
                .find(|b| !b.is_ascii_whitespace())
                == Some(b'{')
        {
            return Ok(String::new());
        }
        let old_len = self.reply.text.len();
        while let Some(end) = self.bytes.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = self.bytes.drain(..=end).collect();
            let line = std::str::from_utf8(&line)
                .map_err(|_| AiError::Protocol("invalid UTF-8".into()))?
                .trim_end_matches(['\r', '\n']);
            if line.is_empty() {
                self.flush()?;
            } else if let Some(data) = line.strip_prefix("data:") {
                self.saw_event = true;
                if !self.event_data.is_empty() {
                    self.event_data.push('\n');
                }
                self.event_data
                    .push_str(data.strip_prefix(' ').unwrap_or(data));
                if self.event_data.len() > MAX_PART_BYTES {
                    return Err(AiError::Limit);
                }
            }
        }
        if self.bytes.len() > MAX_PART_BYTES {
            return Err(AiError::Limit);
        }
        Ok(self.reply.text[old_len..].into())
    }
    fn flush(&mut self) -> Result<(), AiError> {
        if self.event_data.is_empty() {
            return Ok(());
        }
        let data = std::mem::take(&mut self.event_data);
        if self.done {
            return Err(AiError::Protocol("data after terminal stream event".into()));
        }
        if data == "[DONE]" {
            self.done = true;
            return Ok(());
        }
        let value: Value = serde_json::from_str(&data)
            .map_err(|_| AiError::Protocol("invalid event JSON".into()))?;
        if value.get("error").is_some() || value["type"] == "error" {
            return Err(AiError::Protocol(
                "provider returned a stream error; no tools committed from this response".into(),
            ));
        }
        if self.kind == ProviderKind::Anthropic {
            self.anthropic(&value)?;
        } else {
            self.openai(&value)?;
        }
        if self.reply.text.len() > MAX_PART_BYTES
            || self.calls.len() > 32
            || self.calls.values().any(|c| {
                c.arguments.len() > MAX_PART_BYTES || c.id.len() > 256 || c.name.len() > 128
            })
        {
            return Err(AiError::Limit);
        }
        Ok(())
    }
    fn openai(&mut self, v: &Value) -> Result<(), AiError> {
        if let Some(n) = v["usage"]["prompt_tokens"].as_u64() {
            self.reply.usage.input_tokens = n;
        }
        if let Some(n) = v["usage"]["completion_tokens"].as_u64() {
            self.reply.usage.output_tokens = n;
        }
        let Some(choices) = v["choices"].as_array() else {
            return Err(AiError::Protocol("missing completion choices".into()));
        };
        for choice in choices {
            if choice["index"].as_u64().unwrap_or(0) != 0 {
                continue;
            }
            if let Some(text) = choice["delta"]["content"].as_str() {
                self.reply.text.push_str(text);
            }
            if let Some(finish) = choice["finish_reason"].as_str() {
                self.finish = Some(finish.into());
            }
            if let Some(calls) = choice["delta"]["tool_calls"].as_array() {
                for c in calls {
                    let index = c["index"]
                        .as_u64()
                        .filter(|n| *n < 32)
                        .ok_or_else(|| AiError::Protocol("invalid tool index".into()))?
                        as usize;
                    let call = self.calls.entry(index).or_default();
                    if let Some(s) = c["id"].as_str() {
                        call.id.push_str(s);
                    }
                    if let Some(s) = c["function"]["name"].as_str() {
                        call.name.push_str(s);
                    }
                    if let Some(s) = c["function"]["arguments"].as_str() {
                        call.arguments.push_str(s);
                    }
                }
            }
        }
        Ok(())
    }
    fn anthropic(&mut self, v: &Value) -> Result<(), AiError> {
        match v["type"].as_str().unwrap_or("") {
            "message_start" => {
                self.reply.usage.input_tokens =
                    v["message"]["usage"]["input_tokens"].as_u64().unwrap_or(0);
            }
            "content_block_start" => {
                let block = &v["content_block"];
                if block["type"] == "text"
                    && let Some(s) = block["text"].as_str()
                {
                    self.reply.text.push_str(s);
                }
                if block["type"] == "tool_use" {
                    let index = v["index"]
                        .as_u64()
                        .filter(|n| *n < 64)
                        .ok_or_else(|| AiError::Protocol("invalid content index".into()))?
                        as usize;
                    if self.calls.contains_key(&index) {
                        return Err(AiError::Protocol("duplicate content block".into()));
                    }
                    self.calls.insert(
                        index,
                        PartialCall {
                            id: v["content_block"]["id"].as_str().unwrap_or("").into(),
                            name: v["content_block"]["name"].as_str().unwrap_or("").into(),
                            initial: block.get("input").cloned(),
                            ..PartialCall::default()
                        },
                    );
                }
            }
            "content_block_delta" => match v["delta"]["type"].as_str().unwrap_or("") {
                "text_delta" => {
                    if let Some(s) = v["delta"]["text"].as_str() {
                        self.reply.text.push_str(s);
                    }
                }
                "input_json_delta" => {
                    let index = v["index"].as_u64().unwrap_or(u64::MAX) as usize;
                    let call = self
                        .calls
                        .get_mut(&index)
                        .ok_or_else(|| AiError::Protocol("tool delta before block start".into()))?;
                    call.arguments.push_str(
                        v["delta"]["partial_json"]
                            .as_str()
                            .ok_or_else(|| AiError::Protocol("missing tool JSON delta".into()))?,
                    );
                }
                _ => {}
            },
            "message_delta" => {
                self.finish = v["delta"]["stop_reason"].as_str().map(str::to_owned);
                self.reply.usage.output_tokens = v["usage"]["output_tokens"].as_u64().unwrap_or(0);
            }
            "message_stop" => self.done = true,
            "ping" | "content_block_stop" => {}
            _ => {}
        }
        Ok(())
    }
    pub fn finish(mut self) -> Result<ModelReply, AiError> {
        if !self.bytes.is_empty() {
            let remaining = std::mem::take(&mut self.bytes);
            if self.saw_event {
                let line = std::str::from_utf8(&remaining)
                    .map_err(|_| AiError::Protocol("invalid UTF-8".into()))?
                    .trim_end_matches('\r');
                if let Some(data) = line.strip_prefix("data:") {
                    self.event_data.push_str(data.trim_start_matches(' '));
                }
            } else {
                return decode_reply(self.kind, &remaining);
            }
        }
        self.flush()?;
        if !self.done
            || !matches!(
                self.finish.as_deref(),
                Some("stop" | "tool_calls" | "end_turn" | "tool_use" | "stop_sequence")
            )
        {
            return Err(AiError::Protocol(
                "incomplete or truncated response; pending tools were discarded".into(),
            ));
        }
        let mut ids = std::collections::HashSet::new();
        for (_, call) in self.calls {
            if call.id.is_empty() || call.name.is_empty() || !ids.insert(call.id.clone()) {
                return Err(AiError::Protocol("invalid/duplicate tool call ID".into()));
            }
            let arguments = if call.arguments.is_empty() {
                call.initial.unwrap_or(json!({}))
            } else {
                serde_json::from_str(&call.arguments)
                    .map_err(|_| AiError::Protocol("tool arguments are not valid JSON".into()))?
            };
            self.reply.tool_calls.push(ToolCall {
                id: call.id,
                name: call.name,
                arguments,
            });
        }
        validate_reply(&self.reply)?;
        Ok(self.reply)
    }
}

pub fn validate_reply(reply: &ModelReply) -> Result<(), AiError> {
    let mut ids = std::collections::HashSet::new();
    if reply.text.len() > MAX_PART_BYTES || reply.tool_calls.len() > 32 {
        return Err(AiError::Limit);
    }
    for c in &reply.tool_calls {
        if c.id.is_empty()
            || c.id.len() > 256
            || c.name.is_empty()
            || c.name.len() > 128
            || !c.arguments.is_object()
            || c.arguments.to_string().len() > MAX_PART_BYTES
            || !ids.insert(&c.id)
        {
            return Err(AiError::Protocol("invalid tool call".into()));
        }
    }
    Ok(())
}

pub fn decode_reply(kind: ProviderKind, bytes: &[u8]) -> Result<ModelReply, AiError> {
    if bytes.len() > MAX_RESPONSE_BYTES {
        return Err(AiError::Limit);
    }
    let v: Value = serde_json::from_slice(bytes)
        .map_err(|_| AiError::Protocol("invalid response JSON".into()))?;
    if v.get("error").is_some() {
        return Err(AiError::Protocol("provider request failed".into()));
    }
    let mut reply = ModelReply::default();
    if kind == ProviderKind::Anthropic {
        if !matches!(
            v["stop_reason"].as_str(),
            Some("end_turn" | "tool_use" | "stop_sequence")
        ) {
            return Err(AiError::Protocol("incomplete response".into()));
        }
        for b in v["content"]
            .as_array()
            .ok_or_else(|| AiError::Protocol("missing content".into()))?
        {
            if b["type"] == "text" {
                reply.text.push_str(b["text"].as_str().unwrap_or(""));
            }
            if b["type"] == "tool_use" {
                reply.tool_calls.push(ToolCall {
                    id: b["id"].as_str().unwrap_or("").into(),
                    name: b["name"].as_str().unwrap_or("").into(),
                    arguments: b["input"].clone(),
                });
            }
        }
        reply.usage = Usage {
            input_tokens: v["usage"]["input_tokens"].as_u64().unwrap_or(0),
            output_tokens: v["usage"]["output_tokens"].as_u64().unwrap_or(0),
        };
    } else {
        let c = &v["choices"][0];
        if !matches!(c["finish_reason"].as_str(), Some("stop" | "tool_calls")) {
            return Err(AiError::Protocol("incomplete response".into()));
        }
        reply.text = c["message"]["content"].as_str().unwrap_or("").into();
        if let Some(calls) = c["message"]["tool_calls"].as_array() {
            for c in calls {
                reply.tool_calls.push(ToolCall {
                    id: c["id"].as_str().unwrap_or("").into(),
                    name: c["function"]["name"].as_str().unwrap_or("").into(),
                    arguments: serde_json::from_str(
                        c["function"]["arguments"].as_str().unwrap_or(""),
                    )
                    .map_err(|_| AiError::Protocol("invalid tool arguments JSON".into()))?,
                });
            }
        }
        reply.usage = Usage {
            input_tokens: v["usage"]["prompt_tokens"].as_u64().unwrap_or(0),
            output_tokens: v["usage"]["completion_tokens"].as_u64().unwrap_or(0),
        };
    }
    validate_reply(&reply)?;
    Ok(reply)
}
