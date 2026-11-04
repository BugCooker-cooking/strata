use crate::{AiError, catalog, parse, provider::*, tools::*};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, VecDeque},
    time::{Duration, Instant},
};
use strata_core::{
    DocumentId,
    commands::{CommandKind, Control, Snapshot},
};
use uuid::Uuid;

/// Implemented by the editor facade, never by model input. Native and future
/// MCP hosts reuse this interface and the same command/ownership checks.
pub trait ToolHost {
    fn snapshot(&self) -> Result<Snapshot, AiError>;
    fn execute(
        &self,
        turn: Uuid,
        call: Uuid,
        id: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, AiError>;
    fn pause(&self, turn: Uuid) -> Result<(), AiError>;
    fn resume(&self, turn: Uuid) -> Result<(), AiError>;
    fn stop(&self, turn: Uuid) -> Result<(), AiError>;
    fn canvas(&self, request: &CanvasRequest, vision: bool) -> Result<CanvasResult, AiError>;
}

pub struct CanvasResult {
    pub data: Value,
    pub image: Option<ImageContent>,
}

/// Constructed by a trusted host after showing the exact provider destination
/// and context scope. There is intentionally no Deserialize implementation.
#[derive(Clone)]
pub struct ContextGrant {
    session_id: Uuid,
    document_id: Option<DocumentId>,
    destination: String,
    region: Option<Region>,
    pixels: bool,
}
impl ContextGrant {
    pub fn approved(
        snapshot: &Snapshot,
        config: &ProviderConfig,
        pixels: bool,
        region: Option<Region>,
    ) -> Result<Self, AiError> {
        Ok(Self {
            session_id: snapshot.session_id,
            document_id: snapshot.document_id,
            destination: config.identity()?,
            region,
            pixels,
        })
    }
    fn check(&self, snapshot: &Snapshot, config: &ProviderConfig) -> Result<(), AiError> {
        if snapshot.session_id != self.session_id
            || snapshot.document_id != self.document_id
            || config.identity()? != self.destination
        {
            return Err(AiError::Permission);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionMode {
    AskFirst,
    Direct,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SessionOptions {
    pub mode: ExecutionMode,
    pub max_steps: usize,
    pub request_timeout_seconds: u64,
    pub turn_timeout_seconds: u64,
}
impl Default for SessionOptions {
    fn default() -> Self {
        Self {
            mode: ExecutionMode::AskFirst,
            max_steps: 50,
            request_timeout_seconds: 60,
            turn_timeout_seconds: 300,
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Step {
    Request {
        request_id: Uuid,
        request: Value,
        timeout_seconds: u64,
    },
    Tool {
        call_id: String,
        name: String,
        arguments: Value,
        result: Value,
        elapsed_ms: u64,
    },
    Paused {
        question_id: Uuid,
        reason: String,
        question: String,
        options: Vec<String>,
        operation: Option<Value>,
    },
    Finished {
        status: String,
        usage: Usage,
        steps: usize,
        elapsed_ms: u64,
        external_writes: usize,
    },
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostAnswer {
    pub question_id: Uuid,
    #[serde(default)]
    pub answer: String,
    #[serde(default)]
    pub approved: bool,
    /// Only an opaque resource returned by a native file picker, never a path.
    #[serde(default)]
    pub resource: Option<String>,
}

#[derive(Clone)]
enum WaitReason {
    Question,
    Edit { destructive: bool },
    File { id: String, params: Value },
    Lifecycle { id: String, params: Value },
}
#[derive(Clone)]
struct Waiting {
    id: Uuid,
    call: ToolCall,
    step: Step,
    reason: WaitReason,
    started: Instant,
}
#[derive(Clone)]
struct Receipt {
    call: ToolCall,
    data: Value,
    image: Option<ImageContent>,
}

pub struct Session {
    config: ProviderConfig,
    grant: ContextGrant,
    options: SessionOptions,
    turn: Uuid,
    messages: Vec<Message>,
    pending: VecDeque<ToolCall>,
    awaiting: Option<(Uuid, Instant)>,
    decoder: Option<StreamDecoder>,
    waiting: Option<Waiting>,
    receipts: HashMap<String, Receipt>,
    approved_ordinary: bool,
    steps: usize,
    requests: usize,
    retries: usize,
    started: Instant,
    paused_time: Duration,
    usage: Usage,
    finished: Option<String>,
    external_writes: usize,
    ended_elapsed: Option<Duration>,
}
impl Session {
    pub fn new(
        config: ProviderConfig,
        grant: ContextGrant,
        options: SessionOptions,
        turn: Uuid,
        snapshot: &Snapshot,
        prompt: String,
        prior: Vec<Message>,
    ) -> Result<Self, AiError> {
        grant.check(snapshot, &config)?;
        if snapshot.control != (Control::Running { turn_id: turn }) {
            return Err(AiError::Stopped);
        }
        if prompt.trim().is_empty()
            || prompt.len() > 32768
            || !(1..=200).contains(&options.max_steps)
            || !(5..=120).contains(&options.request_timeout_seconds)
            || !(30..=1800).contains(&options.turn_timeout_seconds)
            || prior.len() > 100
            || prior.iter().any(|m| {
                !matches!(m.role.as_str(), "user" | "assistant")
                    || !m.images.is_empty()
                    || !m.tool_calls.is_empty()
                    || m.tool_call_id.is_some()
                    || m.text.len() > 32768
            })
        {
            return Err(AiError::Invalid(
                "invalid message, conversation or execution limits".into(),
            ));
        }
        let mut messages = vec![Message::text(
            "system",
            catalog::system_prompt(config.vision),
        )];
        messages.extend(prior);
        messages.push(Message::text("user", prompt));
        Ok(Self {
            config,
            grant,
            options,
            turn,
            messages,
            pending: VecDeque::new(),
            awaiting: None,
            decoder: None,
            waiting: None,
            receipts: HashMap::new(),
            approved_ordinary: false,
            steps: 0,
            requests: 0,
            retries: 0,
            started: Instant::now(),
            paused_time: Duration::ZERO,
            usage: Usage::default(),
            finished: None,
            external_writes: 0,
            ended_elapsed: None,
        })
    }
    pub fn turn_id(&self) -> Uuid {
        self.turn
    }
    pub fn config(&self) -> &ProviderConfig {
        &self.config
    }
    pub fn transcript(&self) -> &[Message] {
        &self.messages
    }
    pub fn elapsed(&self) -> Duration {
        if let Some(ended) = self.ended_elapsed {
            return ended;
        }
        self.started.elapsed().saturating_sub(
            self.paused_time
                + self
                    .waiting
                    .as_ref()
                    .map(|w| w.started.elapsed())
                    .unwrap_or_default(),
        )
    }
    fn command_timeout(&self) -> Duration {
        Duration::from_secs(self.options.turn_timeout_seconds)
            .saturating_sub(self.elapsed())
            .min(Duration::from_secs(self.options.request_timeout_seconds))
    }
    fn check(&self, host: &dyn ToolHost) -> Result<Snapshot, AiError> {
        let snapshot = host.snapshot()?;
        self.grant.check(&snapshot, &self.config)?;
        let matches = matches!(snapshot.control, Control::Running { turn_id } | Control::Paused { turn_id } if turn_id == self.turn);
        if !matches {
            return Err(AiError::Stopped);
        }
        if self.elapsed() >= Duration::from_secs(self.options.turn_timeout_seconds) {
            return Err(AiError::Limit);
        }
        Ok(snapshot)
    }
    pub fn poll(&mut self, host: &dyn ToolHost) -> Result<Step, AiError> {
        if let Some(status) = &self.finished {
            return Ok(self.finished_step(status));
        }
        if let Err(error) = self.check(host) {
            let _ = host.stop(self.turn);
            self.finished = Some(error.to_string());
            return Err(error);
        }
        if let Some(waiting) = &self.waiting {
            return Ok(waiting.step.clone());
        }
        if let Some((_, issued)) = self.awaiting {
            if issued.elapsed() >= Duration::from_secs(self.options.request_timeout_seconds) {
                self.abort(host, "timeout")?;
                return Err(AiError::Limit);
            }
            return Err(AiError::Invalid(
                "a model request is already in flight".into(),
            ));
        }
        if let Some(call) = self.pending.pop_front() {
            if self.steps >= self.options.max_steps {
                self.abort(host, "step_limit")?;
                return Err(AiError::Limit);
            }
            self.steps += 1;
            let started = Instant::now();
            if let Some(old) = self.receipts.get(&call.id) {
                let mut data = if old.call.name == call.name && old.call.arguments == call.arguments
                {
                    old.data.clone()
                } else {
                    json!({"error":"tool_call_conflict","message":"Tool call ID reused with different arguments. Use a fresh call ID."})
                };
                let image = if old.call.name == call.name && old.call.arguments == call.arguments {
                    old.image.clone()
                } else {
                    None
                };
                if call.name == "view_canvas" && image.is_none() && data["image_available"] == true
                {
                    data["image_available"] = json!(false);
                    data["image_context_expired"] = json!(true);
                    data["next"] =
                        json!("Use a fresh tool call ID to obtain a current canvas image.");
                }
                self.messages.push(Message::tool(&call.id, &data, image));
                return Ok(Step::Tool {
                    call_id: call.id,
                    name: call.name,
                    arguments: call.arguments,
                    result: data,
                    elapsed_ms: 0,
                });
            }
            match self.run_tool(host, &call) {
                Ok(Some((data, image))) => Ok(self.record(call, data, image, started.elapsed())),
                Ok(None) => Ok(self.waiting.as_ref().expect("tool paused").step.clone()),
                Err(error) => {
                    // A user stop/context mismatch is terminal, never a prompt
                    // telling the model to retry with a fresh permission token.
                    if matches!(
                        error,
                        AiError::Stopped | AiError::Permission | AiError::Limit
                    ) {
                        self.abort(host, "stopped")?;
                        return Err(error);
                    }
                    let data = json!({"error":"tool_failed","message":error.to_string(),"next":"Read describe_commands, inspect current state, or ask_user. Do not invent permissions."});
                    Ok(self.record(call, data, None, started.elapsed()))
                }
            }
        } else {
            if self.requests > self.options.max_steps {
                self.abort(host, "request_limit")?;
                return Err(AiError::Limit);
            }
            let request_id = Uuid::new_v4();
            let request = serde_json::to_value(encode(&self.config, &self.messages)?)
                .map_err(|e| AiError::Invalid(e.to_string()))?;
            self.awaiting = Some((request_id, Instant::now()));
            self.decoder = Some(StreamDecoder::new(self.config.kind));
            self.requests += 1;
            Ok(Step::Request {
                request_id,
                request,
                timeout_seconds: self.options.request_timeout_seconds,
            })
        }
    }
    fn run_tool(
        &mut self,
        host: &dyn ToolHost,
        call: &ToolCall,
    ) -> Result<Option<(Value, Option<ImageContent>)>, AiError> {
        match call.name.as_str() {
            "browse_catalog" => {
                let p: Browse = parse(call.arguments.clone())?;
                Ok(Some((catalog::browse(p.path.as_deref())?, None)))
            }
            "search_catalog" => {
                let p: Search = parse(call.arguments.clone())?;
                Ok(Some((catalog::search(&p.query, p.limit)?, None)))
            }
            "describe_commands" => {
                let p: Describe = parse(call.arguments.clone())?;
                Ok(Some((catalog::describe(&p.ids)?, None)))
            }
            "inspect_document" => {
                let _: Inspect = parse(call.arguments.clone())?;
                Ok(Some((inspect(&host.snapshot()?), None)))
            }
            "view_canvas" => {
                let mut p: CanvasRequest = parse(call.arguments.clone())?;
                if !(32..=2048).contains(&p.max_size) || !p.layers.is_empty() {
                    return Err(AiError::Invalid("max_size must be 32..2048; only the authorized visible composite is currently available".into()));
                }
                if self.config.vision && !self.grant.pixels {
                    return Err(AiError::Invalid(
                        "image sharing was not authorized; ask the user to enable canvas sharing"
                            .into(),
                    ));
                }
                if let Some(allowed) = &self.grant.region {
                    if p.region.is_none() {
                        p.region = Some(allowed.clone());
                    }
                    if !contains(allowed, p.region.as_ref().expect("region")) {
                        return Err(AiError::Invalid(
                            "requested region is outside the authorized context scope".into(),
                        ));
                    }
                }
                let result = host.canvas(&p, self.config.vision)?;
                Ok(Some((result.data, result.image)))
            }
            "ask_user" => {
                let p: Ask = parse(call.arguments.clone())?;
                if p.question.trim().is_empty()
                    || p.question.len() > 4096
                    || (!p.options.is_empty() && !(2..=6).contains(&p.options.len()))
                    || p.options.iter().any(|o| o.is_empty() || o.len() > 256)
                {
                    return Err(AiError::Invalid(
                        "question must be 1..4096 bytes; supply zero or 2..6 concise options"
                            .into(),
                    ));
                }
                self.wait(
                    host,
                    call.clone(),
                    WaitReason::Question,
                    p.question,
                    p.options,
                    None,
                )?;
                Ok(None)
            }
            "invoke_command" => {
                let p: Invoke = parse(call.arguments.clone())?;
                self.command(host, call, &p.id, p.params)
            }
            "invoke_batch" => {
                let p: Batch = parse(call.arguments.clone())?;
                if p.steps.is_empty() || p.steps.len() > 128 {
                    return Err(AiError::Invalid("batch must contain 1..128 steps".into()));
                }
                for step in &p.steps {
                    if !spec(&step.command)?.batchable {
                        return Err(AiError::Invalid(format!(
                            "{} cannot be batched; split file writes, history and lifecycle into individual calls",
                            step.command
                        )));
                    }
                }
                self.command(host, call, "document.batch", json!({"steps":p.steps}))
            }
            _ => Err(AiError::Invalid(
                "unknown meta-tool; use only the eight registered meta-tools".into(),
            )),
        }
    }
    fn command(
        &mut self,
        host: &dyn ToolHost,
        call: &ToolCall,
        id: &str,
        params: Value,
    ) -> Result<Option<(Value, Option<ImageContent>)>, AiError> {
        let spec = spec(id)?;
        if !params.is_object() {
            return Err(AiError::Invalid("command params must be an object".into()));
        }
        if spec.kind == CommandKind::Lifecycle && id == "file.new" {
            self.wait(host, call.clone(), WaitReason::Lifecycle { id: id.into(), params: params.clone() },
                format!("允许新建文档并替换当前文档？请先保存需要保留的修改。新文档的元数据及已选择的画布范围将继续供同一服务使用：{}。",self.config.request_url()?),
                vec!["新建并授权新文档上下文".into(),"取消".into()],Some(json!({"command":id,"params":params})))?;
            return Ok(None);
        }
        if spec.kind == CommandKind::External
            || spec.kind == CommandKind::Lifecycle
            || id == "file.place"
        {
            let mut safe_params = params;
            safe_params
                .as_object_mut()
                .expect("object")
                .remove("resource");
            self.wait(host, call.clone(), WaitReason::File { id: id.into(), params: safe_params.clone() },
                if spec.kind == CommandKind::Lifecycle { format!("选择并授权要打开/恢复的文件，同时替换当前文档。请先保存需要保留的修改。所选文档的元数据及已选画布范围将供同一服务使用：{}。", self.config.request_url()?) }
                else if id == "file.place" { "请选择本轮允许 AI 置入的图片。".into() } else { "请选择本次保存或导出的目标；已有文件需要单独确认覆盖。文档撤销不会恢复磁盘文件。".into() },
                vec!["选择文件".into(),"取消此操作".into()], Some(json!({"command":id,"params":safe_params})))?;
            return Ok(None);
        }
        let destructive = destructive(id, &params);
        if spec.kind == CommandKind::Mutate
            && ((self.options.mode == ExecutionMode::AskFirst && !self.approved_ordinary)
                || destructive)
        {
            self.wait(host, call.clone(), WaitReason::Edit { destructive },
                if destructive { "允许这次删除、合并或替换内容的操作？".into() } else { "允许 AI 在本轮创建图层、调整属性与执行普通文档编辑？删除/合并内容及文件写入会另行确认。".into() },
                vec!["允许".into(),"拒绝此操作".into()], Some(json!({"command":id,"params":params})))?;
            return Ok(None);
        }
        let data = host.execute(
            self.turn,
            Uuid::new_v4(),
            id,
            params.clone(),
            self.command_timeout(),
        )?;
        let data = sanitize_result(id, &params, data);
        Ok(Some((
            json!({"result":data,"state":inspect(&host.snapshot()?)}),
            None,
        )))
    }
    fn wait(
        &mut self,
        host: &dyn ToolHost,
        call: ToolCall,
        reason: WaitReason,
        question: String,
        options: Vec<String>,
        operation: Option<Value>,
    ) -> Result<(), AiError> {
        host.pause(self.turn)?;
        let id = Uuid::new_v4();
        let label = match &reason {
            WaitReason::Question => "question",
            WaitReason::Edit { .. } => "edit_approval",
            WaitReason::File { .. } => "file_resource",
            WaitReason::Lifecycle { .. } => "lifecycle_approval",
        };
        let step = Step::Paused {
            question_id: id,
            reason: label.into(),
            question,
            options,
            operation,
        };
        self.waiting = Some(Waiting {
            id,
            call,
            step,
            reason,
            started: Instant::now(),
        });
        Ok(())
    }
    fn record(
        &mut self,
        call: ToolCall,
        data: Value,
        image: Option<ImageContent>,
        elapsed: Duration,
    ) -> Step {
        if image.is_some() {
            // Keep at most two canvas images in model context. A new snapshot
            // never makes every prior image a permanent unbudgeted allocation.
            let mut kept = 0;
            for message in self.messages.iter_mut().rev() {
                if !message.images.is_empty() {
                    if kept >= 1 {
                        message.images.clear();
                        if let Ok(mut value) = serde_json::from_str::<Value>(&message.text)
                            && let Some(map) = value.as_object_mut()
                        {
                            map.insert("image_context_expired".into(), json!(true));
                            message.text = value.to_string();
                        }
                    }
                    kept += 1;
                }
            }
            // Receipts retain results for deduplication; the transcript already
            // owns the bounded images. Replays can request a fresh canvas view.
            for receipt in self.receipts.values_mut() {
                receipt.image = None;
            }
        }
        self.messages
            .push(Message::tool(&call.id, &data, image.clone()));
        self.receipts.insert(
            call.id.clone(),
            Receipt {
                call: call.clone(),
                data: data.clone(),
                image,
            },
        );
        Step::Tool {
            call_id: call.id,
            name: call.name,
            arguments: call.arguments,
            result: data,
            elapsed_ms: elapsed.as_millis() as u64,
        }
    }
    pub fn respond(&mut self, host: &dyn ToolHost, answer: HostAnswer) -> Result<Step, AiError> {
        self.check(host)?;
        if self
            .waiting
            .as_ref()
            .is_none_or(|w| w.id != answer.question_id)
        {
            return Err(AiError::Stopped);
        }
        if answer.answer.len() > 32768 {
            return Err(AiError::Invalid("answer too long".into()));
        }
        let waiting = self.waiting.take().expect("checked waiting");
        self.paused_time += waiting.started.elapsed();
        host.resume(self.turn)?;
        let result = match waiting.reason {
            WaitReason::Question => Ok((
                json!({"answer":answer.answer,"state_after_pause":inspect(&host.snapshot()?)}),
                None,
            )),
            WaitReason::Edit { destructive } => {
                if !answer.approved {
                    Ok((
                        json!({"status":"denied","message":"The user declined this edit. Do not repeat it without new instructions."}),
                        None,
                    ))
                } else {
                    // Approval never resumes a precomputed document candidate:
                    // the command is prepared again against the latest revision.
                    if !destructive {
                        self.approved_ordinary = true;
                    }
                    let (id, params) = invocation(&waiting.call)?;
                    match host.execute(
                        self.turn,
                        Uuid::new_v4(),
                        &id,
                        params.clone(),
                        self.command_timeout(),
                    ) {
                        Ok(data) => Ok((
                            json!({"result":sanitize_result(&id, &params, data),"state_after_pause":inspect(&host.snapshot()?)}),
                            None,
                        )),
                        Err(error) => Err(error),
                    }
                }
            }
            WaitReason::File { id, mut params } => {
                if let Some(resource) = answer.resource.filter(|_| answer.approved) {
                    params["resource"] = json!(resource);
                    let data = host.execute(
                        self.turn,
                        Uuid::new_v4(),
                        &id,
                        params,
                        self.command_timeout(),
                    );
                    if data.is_ok() {
                        if spec(&id)?.kind == CommandKind::External {
                            self.external_writes += 1;
                        }
                        if spec(&id)?.kind == CommandKind::Lifecycle {
                            self.rebind_context(host)?;
                        }
                    }
                    data.map(|data| (json!({"result":data,"undo_boundary":"External file writes are not reversed by document undo."}), None))
                } else {
                    Ok((
                        json!({"status":"denied","message":"No file resource was authorized."}),
                        None,
                    ))
                }
            }
            WaitReason::Lifecycle { id, params } => {
                if answer.approved {
                    match host.execute(
                        self.turn,
                        Uuid::new_v4(),
                        &id,
                        params,
                        self.command_timeout(),
                    ) {
                        Ok(data) => {
                            self.rebind_context(host)?;
                            Ok((
                                json!({"result":data,"state_after_context_approval":inspect(&host.snapshot()?)}),
                                None,
                            ))
                        }
                        Err(error) => Err(error),
                    }
                } else {
                    Ok((
                        json!({"status":"denied","message":"Document replacement and new context sharing were declined."}),
                        None,
                    ))
                }
            }
        };
        let (data, image) = result.unwrap_or_else(|e| {
            (
                json!({"error":"command_failed","message":e.to_string()}),
                None,
            )
        });
        Ok(self.record(waiting.call, data, image, Duration::ZERO))
    }
    fn rebind_context(&mut self, host: &dyn ToolHost) -> Result<(), AiError> {
        self.grant =
            ContextGrant::approved(&host.snapshot()?, &self.config, self.grant.pixels, None)?;
        self.approved_ordinary = false;
        for message in &mut self.messages {
            message.images.clear();
        }
        for receipt in self.receipts.values_mut() {
            receipt.image = None;
        }
        Ok(())
    }
    pub fn feed(&mut self, request_id: Uuid, bytes: &[u8]) -> Result<String, AiError> {
        self.check_response_id(request_id)?;
        self.decoder.as_mut().ok_or(AiError::Stopped)?.feed(bytes)
    }
    fn check_response_id(&self, id: Uuid) -> Result<(), AiError> {
        if self.finished.is_some()
            || self
                .awaiting
                .as_ref()
                .is_none_or(|(current, _)| *current != id)
        {
            return Err(AiError::Stopped);
        }
        if self.awaiting.as_ref().is_some_and(|(_, issued)| {
            issued.elapsed() >= Duration::from_secs(self.options.request_timeout_seconds)
        }) {
            return Err(AiError::Limit);
        }
        Ok(())
    }
    fn guard_response(&mut self, host: &dyn ToolHost, id: Uuid) -> Result<(), AiError> {
        // An old network callback must not cancel a newer in-flight request.
        if self
            .awaiting
            .as_ref()
            .is_none_or(|(current, _)| *current != id)
        {
            return Err(AiError::Stopped);
        }
        let result = self.check(host).and_then(|_| self.check_response_id(id));
        if let Err(error) = result {
            self.abort(host, "stopped")?;
            return Err(error);
        }
        Ok(())
    }
    pub fn finish_response(
        &mut self,
        host: &dyn ToolHost,
        request_id: Uuid,
    ) -> Result<(), AiError> {
        self.guard_response(host, request_id)?;
        let reply = match self.decoder.take().ok_or(AiError::Stopped)?.finish() {
            Ok(reply) => reply,
            Err(error) => {
                self.abort(host, "protocol_error")?;
                return Err(error);
            }
        };
        self.accept_reply(host, request_id, reply)
    }
    /// Transport-neutral entry for deterministic replay and non-streaming hosts.
    pub fn accept_reply(
        &mut self,
        host: &dyn ToolHost,
        request_id: Uuid,
        reply: ModelReply,
    ) -> Result<(), AiError> {
        self.guard_response(host, request_id)?;
        validate_reply(&reply)?;
        self.awaiting = None;
        self.decoder = None;
        self.retries = 0;
        self.usage.input_tokens = self
            .usage
            .input_tokens
            .saturating_add(reply.usage.input_tokens);
        self.usage.output_tokens = self
            .usage
            .output_tokens
            .saturating_add(reply.usage.output_tokens);
        self.messages.push(Message::assistant(&reply));
        if reply.tool_calls.is_empty() {
            self.abort(host, "completed")?;
        } else {
            self.pending.extend(reply.tool_calls);
        }
        Ok(())
    }
    /// Retry only the network request before tools were accepted, at most twice.
    pub fn request_failed(
        &mut self,
        host: &dyn ToolHost,
        id: Uuid,
        retryable: bool,
    ) -> Result<(), AiError> {
        self.guard_response(host, id)?;
        self.awaiting = None;
        self.decoder = None;
        if !retryable || self.retries >= 2 {
            self.abort(host, "network_error")?;
        } else {
            self.retries += 1;
        }
        Ok(())
    }
    pub fn abort(&mut self, host: &dyn ToolHost, status: &str) -> Result<(), AiError> {
        self.ended_elapsed = Some(self.elapsed());
        self.pending.clear();
        self.awaiting = None;
        self.decoder = None;
        if let Some(waiting) = self.waiting.take() {
            self.paused_time += waiting.started.elapsed();
        }
        self.finished = Some(status.into());
        host.stop(self.turn)
    }
    fn finished_step(&self, status: &str) -> Step {
        Step::Finished {
            status: status.into(),
            usage: self.usage.clone(),
            steps: self.steps,
            elapsed_ms: self.elapsed().as_millis() as u64,
            external_writes: self.external_writes,
        }
    }
}

fn spec(id: &str) -> Result<&'static strata_core::commands::CommandSpec, AiError> {
    strata_core::commands::catalog()
        .iter()
        .find(|s| s.id == id)
        .ok_or_else(|| AiError::Invalid(format!("unknown command {id}; search_catalog first")))
}
fn invocation(call: &ToolCall) -> Result<(String, Value), AiError> {
    if call.name == "invoke_batch" {
        let _: Batch = parse(call.arguments.clone())?;
        Ok(("document.batch".into(), call.arguments.clone()))
    } else {
        let p: Invoke = parse(call.arguments.clone())?;
        Ok((p.id, p.params))
    }
}
fn destructive(id: &str, params: &Value) -> bool {
    if id == "document.batch" {
        return params["steps"].as_array().is_none_or(|steps| {
            steps
                .iter()
                .any(|s| destructive(s["command"].as_str().unwrap_or(""), &s["params"]))
        });
    }
    matches!(
        id,
        "layer.delete"
            | "layer.merge_down"
            | "layer.merge_visible"
            | "layer.flatten"
            | "layer.mask.apply"
            | "edit.clear"
            | "edit.cut"
    ) || spec(id).is_ok_and(|s| {
        s.effects
            .iter()
            .any(|e| matches!(*e, "pixels.replace" | "layers.remove"))
    })
}
fn contains(outer: &Region, inner: &Region) -> bool {
    inner.width > 0
        && inner.height > 0
        && inner.x >= outer.x
        && inner.y >= outer.y
        && i128::from(inner.x) + i128::from(inner.width)
            <= i128::from(outer.x) + i128::from(outer.width)
        && i128::from(inner.y) + i128::from(inner.height)
            <= i128::from(outer.y) + i128::from(outer.height)
}
/// Export only context covered by the grant: never file paths, hidden layer
/// names, arbitrary execution parameters, or another document's conversation.
pub fn inspect(snapshot: &Snapshot) -> Value {
    let hidden = snapshot.layers.iter().filter(|l| !l.visible).count();
    let layers: Vec<_> = snapshot.layers.iter().filter(|l| l.visible).collect();
    let active = snapshot
        .active_layer
        .filter(|id| layers.iter().any(|l| l.id == *id));
    json!({"document_id":snapshot.document_id,"revision":snapshot.revision,"dirty":snapshot.dirty,"frame":snapshot.frame,
        "bit_depth":snapshot.bit_depth,"profile":snapshot.profile_name,"layers":layers,"hidden_layer_count":hidden,"active_layer":active,
        "history":snapshot.history.iter().rev().take(20).collect::<Vec<_>>(),"history_cursor":snapshot.history_cursor,"control":snapshot.control,
        "pause_events":snapshot.control_events.iter().rev().take(20).collect::<Vec<_>>()})
}

fn sanitize_result(id: &str, params: &Value, mut result: Value) -> Value {
    if id == "document.batch" {
        if let Some(results) = result["results"].as_array_mut()
            && let Some(steps) = params["steps"].as_array()
        {
            for (value, step) in results.iter_mut().zip(steps) {
                *value = sanitize_result(
                    step["command"].as_str().unwrap_or(""),
                    &step["params"],
                    value.take(),
                );
            }
        }
    } else if matches!(id, "document.inspect" | "layer.list") {
        if let Some(layers) = result["layers"].as_array_mut() {
            layers.retain(|l| l["visible"] == true);
        }
        if let Some(map) = result.as_object_mut() {
            map.remove("session_id");
            map.remove("executions");
            map.remove("control_events");
            let active = map.get("active_layer").cloned().unwrap_or(Value::Null);
            if !map["layers"]
                .as_array()
                .is_some_and(|layers| layers.iter().any(|l| l["id"] == active))
            {
                map.remove("active_layer");
            }
        }
    }
    result
}
