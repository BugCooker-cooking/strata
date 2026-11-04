use crate::*;
use base64::Engine as _;
use image::ImageEncoder;
use strata_ai::{
    AiError, ContextGrant, Message, ProviderConfig, Session, SessionOptions, ToolHost,
    provider::{self, ImageContent},
    session::{CanvasResult, HostAnswer},
    tools::{CanvasRequest, Region},
};
use strata_core::commands::{Control, Snapshot};
use strata_pixels::{PixelBuffer, PixelFormat, TileCoord};
use uuid::Uuid;

fn ai_error(error: impl std::fmt::Display) -> StrataError {
    StrataError::Command {
        message: error.to_string(),
    }
}
fn parse_id(id: &str) -> Result<Uuid, StrataError> {
    Uuid::parse_str(id).map_err(ai_error)
}

/// No credential is passed to Rust. Native transport retrieves it from the
/// platform secret store immediately before sending this exact destination.
#[uniffi::export]
pub fn ai_validate_config(config_json: String) -> Result<String, StrataError> {
    let config: ProviderConfig = serde_json::from_str(&config_json).map_err(ai_error)?;
    Ok(json!({"identity":config.identity().map_err(ai_error)?,"url":config.request_url().map_err(ai_error)?}).to_string())
}

#[uniffi::export]
pub fn ai_models_request(config_json: String) -> Result<String, StrataError> {
    let config: ProviderConfig = serde_json::from_str(&config_json).map_err(ai_error)?;
    serde_json::to_string(&provider::models_request(&config).map_err(ai_error)?).map_err(ai_error)
}

#[uniffi::export]
pub fn ai_probe_request(config_json: String, vision: bool) -> Result<String, StrataError> {
    let config: ProviderConfig = serde_json::from_str(&config_json).map_err(ai_error)?;
    let mut message = Message::text(
        "user",
        if vision {
            "Identify the single color in the attached test image. Reply with just its English color name in uppercase."
        } else {
            "Connection capability test. Call inspect_document with {} exactly once. Do not answer with text."
        },
    );
    if vision {
        let mut png = Vec::new();
        image::codecs::png::PngEncoder::new(&mut png)
            .write_image(
                &[255, 0, 255, 255].repeat(16 * 16),
                16,
                16,
                image::ExtendedColorType::Rgba8,
            )
            .map_err(ai_error)?;
        message.images.push(ImageContent {
            mime: "image/png".into(),
            base64: base64::engine::general_purpose::STANDARD.encode(png),
        });
    }
    let mut request = provider::encode(&config, &[message]).map_err(ai_error)?;
    if vision {
        request
            .body
            .as_object_mut()
            .expect("request object")
            .remove("tools");
    } else {
        request.body["tool_choice"] = if config.kind == provider::ProviderKind::Anthropic {
            json!({"type":"tool","name":"inspect_document"})
        } else {
            json!({"type":"function","function":{"name":"inspect_document"}})
        };
    }
    serde_json::to_string(&request).map_err(ai_error)
}

#[uniffi::export]
pub fn ai_probe_result(
    config_json: String,
    response: Vec<u8>,
    vision: bool,
) -> Result<bool, StrataError> {
    let config: ProviderConfig = serde_json::from_str(&config_json).map_err(ai_error)?;
    let mut decoder = provider::StreamDecoder::new(config.kind);
    decoder.feed(&response).map_err(ai_error)?;
    let reply = decoder.finish().map_err(ai_error)?;
    Ok(if vision {
        reply.text.trim().to_uppercase() == "MAGENTA" && reply.tool_calls.is_empty()
    } else {
        reply.tool_calls.len() == 1
            && reply.tool_calls[0].name == "inspect_document"
            && reply.tool_calls[0].arguments == json!({})
    })
}

#[uniffi::export]
impl StrataEngine {
    pub fn ai_begin(
        &self,
        config_json: String,
        options_json: String,
        prompt: String,
        share_canvas: bool,
        prior_json: String,
    ) -> Result<String, StrataError> {
        let config: ProviderConfig = serde_json::from_str(&config_json).map_err(ai_error)?;
        let options: SessionOptions = serde_json::from_str(&options_json).map_err(ai_error)?;
        let prior: Vec<Message> = serde_json::from_str(&prior_json).map_err(ai_error)?;
        let mut slot = self.ai.lock().map_err(ai_error)?;
        {
            let mut inner = self.lock()?;
            let grant =
                ContextGrant::approved(&inner.editor.snapshot(), &config, share_canvas, None)
                    .map_err(ai_error)?;
            let turn = inner.editor.begin_turn().map_err(command_error)?;
            match Session::new(
                config,
                grant,
                options,
                turn,
                &inner.editor.snapshot(),
                prompt,
                prior,
            ) {
                Ok(session) => *slot = Some(session),
                Err(error) => {
                    inner.editor.stop_turn();
                    return Err(ai_error(error));
                }
            }
            queue_state(&mut inner);
        }
        let turn = slot
            .as_ref()
            .expect("created AI session")
            .turn_id()
            .to_string();
        drop(slot);
        self.deliver_events()?;
        Ok(turn)
    }
    pub fn ai_poll(&self, turn_id: String) -> Result<String, StrataError> {
        let turn = parse_id(&turn_id)?;
        self.with_ai(|session| {
            if session.turn_id() != turn {
                return Err(AiError::Stopped);
            }
            session.poll(self)
        })
        .and_then(|step| serde_json::to_string(&step).map_err(ai_error))
    }
    /// Local diagnostic evidence contains no credential or image payload.
    pub fn ai_transcript_json(&self, turn_id: String) -> Result<String, StrataError> {
        let turn = parse_id(&turn_id)?;
        let slot = self.ai.lock().map_err(ai_error)?;
        let session = slot
            .as_ref()
            .filter(|s| s.turn_id() == turn)
            .ok_or_else(|| ai_error("AI turn no longer available"))?;
        let mut messages = session.transcript().to_vec();
        for message in &mut messages {
            message.images.clear();
        }
        serde_json::to_string(&messages).map_err(ai_error)
    }
    pub fn ai_feed(&self, request_id: String, bytes: Vec<u8>) -> Result<String, StrataError> {
        let request = parse_id(&request_id)?;
        self.with_ai(|session| {
            require_turn(self, session.turn_id(), false)?;
            session.feed(request, &bytes)
        })
    }
    pub fn ai_finish_response(&self, request_id: String) -> Result<String, StrataError> {
        let request = parse_id(&request_id)?;
        self.with_ai(|session| {
            session.finish_response(self, request)?;
            Ok(session
                .transcript()
                .last()
                .map(|m| m.text.clone())
                .unwrap_or_default())
        })
    }
    pub fn ai_request_failed(
        &self,
        request_id: String,
        retryable: bool,
    ) -> Result<(), StrataError> {
        let request = parse_id(&request_id)?;
        self.with_ai(|session| session.request_failed(self, request, retryable))
    }
    pub fn ai_respond(&self, answer_json: String) -> Result<String, StrataError> {
        let answer: HostAnswer = serde_json::from_str(&answer_json).map_err(ai_error)?;
        self.with_ai(|session| session.respond(self, answer))
            .and_then(|step| serde_json::to_string(&step).map_err(ai_error))
    }
    /// This path deliberately does not acquire the session mutex: a slow
    /// decode/composition/tool preparation cannot prevent revoking commit rights.
    pub fn ai_stop(&self, turn_id: String) -> Result<(), StrataError> {
        let turn = parse_id(&turn_id)?;
        ToolHost::stop(self, turn).map_err(ai_error)
    }
    pub fn ai_grant_read(&self, turn_id: String, path: String) -> Result<String, StrataError> {
        let turn = parse_id(&turn_id)?;
        require_turn(self, turn, true).map_err(ai_error)?;
        self.lock()?
            .editor
            .grant_read(Path::new(&path), Source::Ai { turn_id: turn })
            .map_err(command_error)
    }
    pub fn ai_grant_write(
        &self,
        turn_id: String,
        path: String,
        overwrite: bool,
    ) -> Result<String, StrataError> {
        let turn = parse_id(&turn_id)?;
        require_turn(self, turn, true).map_err(ai_error)?;
        self.lock()?
            .editor
            .grant_write(Path::new(&path), overwrite, Source::Ai { turn_id: turn })
            .map_err(command_error)
    }
}
impl StrataEngine {
    fn with_ai<T>(
        &self,
        operation: impl FnOnce(&mut Session) -> Result<T, AiError>,
    ) -> Result<T, StrataError> {
        let mut slot = self.ai.lock().map_err(ai_error)?;
        let session = slot.as_mut().ok_or_else(|| ai_error("no AI turn"))?;
        match operation(session) {
            Ok(value) => Ok(value),
            Err(error) => {
                if !matches!(error, AiError::Stopped) {
                    let _ = session.abort(self, "failed");
                }
                Err(ai_error(error))
            }
        }
    }
}
fn queue_state(inner: &mut Inner) {
    inner.events.push_back(EngineEventDto::StateChanged {
        snapshot_json: json!(inner.editor.snapshot()).to_string(),
    });
}
fn require_turn(engine: &StrataEngine, turn: Uuid, paused: bool) -> Result<(), AiError> {
    let inner = engine.lock().map_err(|e| AiError::Host(e.to_string()))?;
    let valid = match inner.editor.control() {
        Control::Running { turn_id } => *turn_id == turn,
        Control::Paused { turn_id } => paused && *turn_id == turn,
        _ => false,
    };
    if valid { Ok(()) } else { Err(AiError::Stopped) }
}
impl ToolHost for StrataEngine {
    fn snapshot(&self) -> Result<Snapshot, AiError> {
        Ok(self
            .lock()
            .map_err(|e| AiError::Host(e.to_string()))?
            .editor
            .snapshot())
    }
    fn execute(
        &self,
        turn: Uuid,
        call: Uuid,
        id: &str,
        params: Value,
        timeout: std::time::Duration,
    ) -> Result<Value, AiError> {
        let (snapshot, mut request) = {
            let inner = self.lock().map_err(|e| AiError::Host(e.to_string()))?;
            (
                inner.editor.clone(),
                inner.editor.request(Source::Ai { turn_id: turn }),
            )
        };
        request.call_id = call;
        request = request.with_timeout(timeout);
        let prepared = snapshot
            .prepare(request, id, params.clone())
            .map_err(|e| AiError::Host(e.to_string()))?;
        let result = {
            let mut inner = self.lock().map_err(|e| AiError::Host(e.to_string()))?;
            apply(&mut inner, prepared, id, &params).map_err(|e| AiError::Host(e.to_string()))?
        };
        self.deliver_events()
            .map_err(|e| AiError::Host(e.to_string()))?;
        Ok(result)
    }
    fn pause(&self, turn: Uuid) -> Result<(), AiError> {
        require_turn(self, turn, false)?;
        {
            let mut inner = self.lock().map_err(|e| AiError::Host(e.to_string()))?;
            if inner.editor.control() != &(Control::Running { turn_id: turn }) {
                return Err(AiError::Stopped);
            }
            inner
                .editor
                .pause_turn()
                .map_err(|e| AiError::Host(e.to_string()))?;
            queue_state(&mut inner);
        }
        self.deliver_events()
            .map_err(|e| AiError::Host(e.to_string()))
    }
    fn resume(&self, turn: Uuid) -> Result<(), AiError> {
        {
            let mut inner = self.lock().map_err(|e| AiError::Host(e.to_string()))?;
            if inner.editor.control() != &(Control::Paused { turn_id: turn }) {
                return Err(AiError::Stopped);
            }
            inner
                .editor
                .resume_turn()
                .map_err(|e| AiError::Host(e.to_string()))?;
            queue_state(&mut inner);
        }
        self.deliver_events()
            .map_err(|e| AiError::Host(e.to_string()))
    }
    fn stop(&self, turn: Uuid) -> Result<(), AiError> {
        {
            let mut inner = self.lock().map_err(|e| AiError::Host(e.to_string()))?;
            if matches!(inner.editor.control(), Control::Running { turn_id } | Control::Paused { turn_id } if *turn_id == turn)
            {
                inner.editor.stop_turn();
                queue_state(&mut inner);
            }
        }
        self.deliver_events()
            .map_err(|e| AiError::Host(e.to_string()))
    }
    fn canvas(&self, request: &CanvasRequest, vision: bool) -> Result<CanvasResult, AiError> {
        let doc = self
            .lock()
            .map_err(|e| AiError::Host(e.to_string()))?
            .editor
            .document()
            .cloned()
            .ok_or(AiError::Permission)?;
        make_canvas(&doc, request, vision).map_err(|e| AiError::Host(e.to_string()))
    }
}

fn make_canvas(
    doc: &Document,
    request: &CanvasRequest,
    vision: bool,
) -> Result<CanvasResult, StrataError> {
    let frame = doc.frame();
    let region = request.region.clone().unwrap_or(Region {
        x: frame.x,
        y: frame.y,
        width: frame.width,
        height: frame.height,
    });
    if !(32..=2048).contains(&request.max_size)
        || !request.layers.is_empty()
        || region.width == 0
        || region.height == 0
        || region.x < frame.x
        || region.y < frame.y
        || i128::from(region.x) + i128::from(region.width)
            > i128::from(frame.x) + i128::from(frame.width)
        || i128::from(region.y) + i128::from(region.height)
            > i128::from(frame.y) + i128::from(frame.height)
    {
        return Err(ai_error(
            "view_canvas region must be inside the authorized artboard",
        ));
    }
    let scale = (f64::from(request.max_size.min(1024))
        / f64::from(region.width.max(region.height)))
    .min(1.0);
    let width = (f64::from(region.width) * scale).round().max(1.0) as u32;
    let height = (f64::from(region.height) * scale).round().max(1.0) as u32;
    let count = (width * height) as usize;
    let _allocation = doc
        .memory_pool()
        .reserve(
            strata_pixels::memory::MemoryKind::Transient,
            count as u64 * 64,
        )
        .map_err(render_error)?;
    let mut samples = Vec::with_capacity(count);
    for y in 0..height {
        for x in 0..width {
            let sx = region.x
                + (((u64::from(x) * 2 + 1) * u64::from(region.width)) / (u64::from(width) * 2))
                    as i64;
            let sy = region.y
                + (((u64::from(y) * 2 + 1) * u64::from(region.height)) / (u64::from(height) * 2))
                    as i64;
            let (tile, [px, py]) = TileCoord::locate(sx, sy);
            samples.push((
                tile,
                usize::from(py) * 256 + usize::from(px),
                (y * width + x) as usize,
            ));
        }
    }
    samples.sort_unstable_by_key(|s| s.0);
    let mut linear = vec![[0.0f32; 4]; count];
    let mut compositor = strata_core::composite::Compositor::new(1);
    let mut last: Option<(TileCoord, strata_core::composite::LinearTile)> = None;
    for (coord, source, dest) in samples {
        if last.as_ref().is_none_or(|(c, _)| *c != coord) {
            last = Some((coord, compositor.tile(doc, coord).map_err(render_error)?));
        }
        linear[dest] = last.as_ref().expect("sample tile").1[source];
    }
    let mut histogram = vec![0u64; 256];
    let mut mean = [0.0f64; 3];
    let mut weight = 0.0f64;
    for p in &linear {
        let alpha = f64::from(p[3]);
        if alpha <= 0.0 {
            continue;
        }
        let luminance = (p[0] * 0.2289746 + p[1] * 0.6917385 + p[2] * 0.0792869).clamp(0.0, 1.0);
        histogram[(luminance * 255.0).round() as usize] += 1;
        for c in 0..3 {
            mean[c] += f64::from(p[c]) * alpha;
        }
        weight += alpha;
    }
    if weight > 0.0 {
        for c in &mut mean {
            *c /= weight;
        }
    }
    let data = json!({"width":width,"height":height,"region":region,"profile":"sRGB image; linear Display P3 statistics","image_available":vision,
        "statistics":{"luminance_histogram":histogram,"mean_linear_rgb":mean,"sampled_pixels":count,"visible_alpha_weight":weight},
        "limitation":"Bounded pixel-center samples of the visible composite; statistics do not identify objects."});
    let image = if vision {
        let srgb = strata_color::working_profile("srgb").map_err(render_error)?;
        let PixelBuffer::Rgba8(rgba) =
            strata_color::from_linear_p3(&linear, PixelFormat::Rgba8, &srgb)
                .map_err(render_error)?
        else {
            unreachable!()
        };
        let mut png = Vec::new();
        image::codecs::png::PngEncoder::new(&mut png)
            .write_image(&rgba, width, height, image::ExtendedColorType::Rgba8)
            .map_err(render_error)?;
        Some(ImageContent {
            mime: "image/png".into(),
            base64: base64::engine::general_purpose::STANDARD.encode(png),
        })
    } else {
        None
    };
    Ok(CanvasResult { data, image })
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Listener;
    impl EngineListener for Listener {
        fn on_event(&self, _: EngineEventDto) {}
    }
    fn engine() -> Arc<StrataEngine> {
        let engine = StrataEngine::new(Arc::new(Listener));
        engine
            .dispatch(
                "file.new".into(),
                json!({"width":8,"height":8,"bit_depth":16,"background":[32768,0,0,65535]})
                    .to_string(),
            )
            .unwrap();
        engine
    }
    fn begin(engine: &StrataEngine) -> String {
        engine.ai_begin(json!({"kind":"open_ai","endpoint":"https://example.test/v1","model":"test","vision":false}).to_string(),
            json!({"mode":"direct","max_steps":50,"request_timeout_seconds":60,"turn_timeout_seconds":300}).to_string(),"test".into(),false,"[]".into()).unwrap()
    }
    fn reply(engine: &StrataEngine, id: &str, command: &str, params: Value) {
        let turn = engine
            .ai
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .turn_id()
            .to_string();
        let request: Value = serde_json::from_str(&engine.ai_poll(turn).unwrap()).unwrap();
        let response = json!({"choices":[{"finish_reason":"tool_calls","message":{"tool_calls":[{"id":id,"type":"function","function":{"name":"invoke_command","arguments":json!({"id":command,"params":params}).to_string()}}]}}]}).to_string();
        engine
            .ai_feed(
                request["request_id"].as_str().unwrap().into(),
                response.into_bytes(),
            )
            .unwrap();
        engine
            .ai_finish_response(request["request_id"].as_str().unwrap().into())
            .unwrap();
    }
    #[test]
    fn native_facade_replay_uses_command_history_and_stop_invalidates_late_feed() {
        let engine = engine();
        let turn = begin(&engine);
        assert!(
            engine
                .dispatch(
                    "layer.create".into(),
                    json!({"name":"user blocked"}).to_string()
                )
                .is_err()
        );
        reply(&engine, "create", "layer.create", json!({"name":"草稿"}));
        engine.ai_poll(turn.clone()).unwrap();
        reply(
            &engine,
            "opacity",
            "layer.set_opacity",
            json!({"layer":{"name":"草稿"},"opacity":0.5}),
        );
        engine.ai_poll(turn.clone()).unwrap();
        assert_eq!(
            ToolHost::snapshot(&*engine)
                .unwrap()
                .layers
                .last()
                .unwrap()
                .opacity,
            0.5
        );
        let request: Value = serde_json::from_str(&engine.ai_poll(turn.clone()).unwrap()).unwrap();
        engine.ai_stop(turn.clone()).unwrap();
        assert!(
            engine
                .ai_feed(
                    request["request_id"].as_str().unwrap().into(),
                    b"data: [DONE]\n\n".to_vec()
                )
                .is_err()
        );
        engine
            .dispatch("edit.undo_turn".into(), json!({"turn_id":turn}).to_string())
            .unwrap();
        assert_eq!(ToolHost::snapshot(&*engine).unwrap().layers.len(), 1);
    }
    #[test]
    fn visible_canvas_is_bounded_srgb_and_leaves_16_bit_truth_unchanged() {
        let engine = engine();
        engine
            .dispatch(
                "layer.create".into(),
                json!({"name":"private","fill":[0,65535,0,65535]}).to_string(),
            )
            .unwrap();
        engine
            .dispatch(
                "layer.set_visibility".into(),
                json!({"layer":{"name":"private"},"visible":false}).to_string(),
            )
            .unwrap();
        let doc = engine.lock().unwrap().editor.document().unwrap().clone();
        let before = doc.export_pixels().unwrap();
        let request = CanvasRequest {
            region: None,
            layers: vec![],
            max_size: 32,
        };
        let preview = make_canvas(&doc, &request, true).unwrap();
        let png = base64::engine::general_purpose::STANDARD
            .decode(preview.image.unwrap().base64)
            .unwrap();
        let image = image::load_from_memory(&png).unwrap().to_rgba8();
        assert_eq!(image.dimensions(), (8, 8));
        assert!(
            image
                .pixels()
                .all(|p| p[0] > 100 && p[1] < 2 && p[2] < 2 && p[3] == 255)
        );
        assert_eq!(doc.export_pixels().unwrap(), before);
        let stats = make_canvas(&doc, &request, false).unwrap();
        assert!(stats.image.is_none());
        assert!(!stats.data["image_available"].as_bool().unwrap());
        let outside = CanvasRequest {
            region: Some(Region {
                x: 7,
                y: 0,
                width: 2,
                height: 2,
            }),
            ..request
        };
        assert!(make_canvas(&doc, &outside, true).is_err());
    }
    #[test]
    fn capability_probe_is_synthetic_and_config_validation_rejects_credentials_in_url() {
        let config=json!({"kind":"open_ai","endpoint":"https://example.test/v1","model":"test","vision":false}).to_string();
        let request = ai_probe_request(config.clone(), true).unwrap();
        assert!(request.contains("data:image/png;base64,"));
        assert!(!request.contains("MAGENTA"));
        let response =
            json!({"choices":[{"finish_reason":"stop","message":{"content":"MAGENTA"}}]})
                .to_string()
                .into_bytes();
        assert!(ai_probe_result(config, response, true).unwrap());
        let bad=json!({"kind":"open_ai","endpoint":"https://key:secret@example.test/v1","model":"test","vision":false}).to_string();
        assert!(ai_validate_config(bad).is_err());
    }
    #[test]
    fn stale_poll_and_network_callbacks_do_not_cancel_a_new_turn() {
        let engine = engine();
        let old = begin(&engine);
        let request: Value = serde_json::from_str(&engine.ai_poll(old.clone()).unwrap()).unwrap();
        let old_request = request["request_id"].as_str().unwrap().to_string();
        engine.ai_stop(old.clone()).unwrap();
        let new = begin(&engine);
        assert!(engine.ai_poll(old).is_err());
        assert!(
            engine
                .ai_feed(old_request.clone(), b"data: [DONE]\n\n".to_vec())
                .is_err()
        );
        let current: Value = serde_json::from_str(&engine.ai_poll(new.clone()).unwrap()).unwrap();
        assert_ne!(current["request_id"], old_request);
        assert!(engine.ai_request_failed(old_request, true).is_err());
        assert_eq!(
            ToolHost::snapshot(&*engine).unwrap().control,
            Control::Running {
                turn_id: Uuid::parse_str(&new).unwrap()
            }
        );
        engine.ai_stop(new).unwrap();
    }
}
