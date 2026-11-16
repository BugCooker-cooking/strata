use crate::*;
use strata_pixels::{TileCoord, TiledMask};

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct SelectionEdge {
    pub x1: f32,
    pub y1: f32,
    pub x2: f32,
    pub y2: f32,
}
#[uniffi::export]
pub fn tonal_curve_preview(points_json: String) -> Result<Vec<f32>, StrataError> {
    if points_json.len() > 16384 {
        return Err(render_error("curve preview parameter limit"));
    }
    let points: Vec<[f64; 2]> = serde_json::from_str(&points_json).map_err(render_error)?;
    strata_core::TonalAdjustment::curve_samples(&points, 257).map_err(render_error)
}

#[uniffi::export]
impl StrataEngine {
    pub fn begin_tonal_brush(&self, layer_id: String, command: String) -> Result<(), StrataError> {
        let layer = serde_json::from_value(json!({"id":layer_id})).map_err(render_error)?;
        self.issue_tonal_preview()?;
        let mut inner = self.lock()?;
        crate::tonal_preview::restore(&mut inner)?;
        inner.brush = Some(
            inner
                .editor
                .begin_tonal_brush(layer, &command)
                .map_err(command_error)?,
        );
        Ok(())
    }
    pub fn begin_spot_heal(&self, layer_id: String) -> Result<(), StrataError> {
        let layer = serde_json::from_value(json!({"id":layer_id})).map_err(render_error)?;
        self.issue_tonal_preview()?;
        let mut inner = self.lock()?;
        crate::tonal_preview::restore(&mut inner)?;
        inner.brush = Some(inner.editor.begin_spot_heal(layer).map_err(command_error)?);
        Ok(())
    }
    pub fn begin_heal(&self, layer_id: String) -> Result<(), StrataError> {
        let layer = serde_json::from_value(json!({"id":layer_id})).map_err(render_error)?;
        self.issue_tonal_preview()?;
        let mut inner = self.lock()?;
        crate::tonal_preview::restore(&mut inner)?;
        inner.brush = Some(inner.editor.begin_heal(layer).map_err(command_error)?);
        Ok(())
    }
    pub fn begin_clone(&self, layer_id: String) -> Result<(), StrataError> {
        let layer = serde_json::from_value(json!({"id":layer_id})).map_err(render_error)?;
        self.issue_tonal_preview()?;
        let mut inner = self.lock()?;
        crate::tonal_preview::restore(&mut inner)?;
        inner.brush = Some(inner.editor.begin_clone(layer).map_err(command_error)?);
        Ok(())
    }
    pub fn begin_paint(&self, layer_id: String, command: String) -> Result<(), StrataError> {
        let layer = serde_json::from_value(json!({"id":layer_id})).map_err(render_error)?;
        self.issue_tonal_preview()?;
        let mut inner = self.lock()?;
        crate::tonal_preview::restore(&mut inner)?;
        inner.paint = Some(
            inner
                .editor
                .begin_paint(layer, &command)
                .map_err(command_error)?,
        );
        Ok(())
    }
    pub fn end_paint(&self, params_json: String, cancel: bool) -> Result<(), StrataError> {
        let (editor, interaction) = {
            let mut inner = self.lock()?;
            let interaction = inner
                .paint
                .take()
                .ok_or_else(|| render_error("no paint gesture"))?;
            (inner.editor.clone(), interaction)
        };
        if !cancel {
            if params_json.len() > 64 * 1024 {
                return Err(render_error("paint parameters exceed limit"));
            }
            let params = serde_json::from_str(&params_json).map_err(render_error)?;
            let prepared = editor
                .prepare_paint(&interaction, params)
                .map_err(command_error)?;
            let mut inner = self.lock()?;
            apply(&mut inner, prepared, interaction.command(), &json!({}))?;
        }
        self.deliver_events()
    }
    pub fn sample_color(&self, params_json: String, revision: u64) -> Result<String, StrataError> {
        if params_json.len() > 4096 {
            return Err(render_error("sample parameters exceed limit"));
        }
        let params = serde_json::from_str(&params_json).map_err(render_error)?;
        let (editor, request) = {
            let inner = self.lock()?;
            if inner.editor.revision() != revision {
                return Err(command_error(CommandError::Stale));
            }
            (inner.editor.clone(), inner.editor.request(Source::User))
        };
        let prepared = editor
            .prepare(request, "paint.sample_color", params)
            .map_err(command_error)?;
        self.lock()?
            .editor
            .commit(prepared)
            .map(|value| value.to_string())
            .map_err(command_error)
    }
    pub fn begin_brush(
        &self,
        layer_id: String,
        target: String,
        erase: bool,
    ) -> Result<(), StrataError> {
        let layer = serde_json::from_value(json!({"id":layer_id})).map_err(render_error)?;
        let target = serde_json::from_value(json!(target)).map_err(render_error)?;
        self.issue_tonal_preview()?;
        let mut inner = self.lock()?;
        crate::tonal_preview::restore(&mut inner)?;
        inner.brush = Some(
            inner
                .editor
                .begin_brush(layer, target, erase)
                .map_err(command_error)?,
        );
        Ok(())
    }
    pub fn end_brush(&self, params_json: String, cancel: bool) -> Result<(), StrataError> {
        let (editor, interaction) = {
            let mut inner = self.lock()?;
            let interaction = inner
                .brush
                .take()
                .ok_or_else(|| render_error("no brush gesture"))?;
            (inner.editor.clone(), interaction)
        };
        if !cancel {
            if params_json.len() > 1024 * 1024 {
                return Err(render_error("brush parameters exceed limit"));
            }
            let params: Value = serde_json::from_str(&params_json).map_err(render_error)?;
            let prepared = editor
                .prepare_brush(&interaction, params.clone())
                .map_err(command_error)?;
            let mut inner = self.lock()?;
            apply(&mut inner, prepared, interaction.command(), &params)?;
        }
        self.deliver_events()
    }
    pub fn begin_marquee(&self) -> Result<(), StrataError> {
        self.issue_tonal_preview()?;
        let mut inner = self.lock()?;
        crate::tonal_preview::restore(&mut inner)?;
        inner.marquee = Some(inner.editor.begin_marquee().map_err(command_error)?);
        Ok(())
    }
    pub fn end_marquee(&self, params_json: String, cancel: bool) -> Result<(), StrataError> {
        self.end_selection("select.marquee".into(), params_json, cancel)
    }
    /// Prepare outside the host lock so canvas scrolling and cancellation stay
    /// responsive during graph cuts. The gesture's original revision is checked
    /// again at commit, even if the worker finishes after user/AI state changes.
    pub fn end_selection(
        &self,
        command: String,
        params_json: String,
        cancel: bool,
    ) -> Result<(), StrataError> {
        let (snapshot, interaction) = {
            let mut inner = self.lock()?;
            let interaction = inner
                .marquee
                .take()
                .ok_or_else(|| render_error("no marquee gesture"))?;
            (inner.editor.clone(), interaction)
        };
        if !cancel {
            if params_json.len() > 1024 * 1024 {
                return Err(render_error("selection parameters exceed limit"));
            }
            let params: Value = serde_json::from_str(&params_json).map_err(render_error)?;
            let prepared = snapshot
                .prepare_selection(&interaction, &command, params.clone())
                .map_err(command_error)?;
            let mut inner = self.lock()?;
            apply(&mut inner, prepared, &command, &params)?;
        }
        self.deliver_events()
    }
    pub fn begin_exposure(&self, layer_id: String) -> Result<(), StrataError> {
        let layer = serde_json::from_value(json!({"id":layer_id})).map_err(render_error)?;
        self.issue_tonal_preview()?;
        let mut inner = self.lock()?;
        crate::tonal_preview::restore(&mut inner)?;
        inner.exposure = Some(inner.editor.begin_exposure(layer).map_err(command_error)?);
        Ok(())
    }
    pub fn preview_exposure(&self, stops: f64) -> Result<(), StrataError> {
        let mut inner = self.lock()?;
        let interaction = inner
            .exposure
            .as_ref()
            .ok_or_else(|| render_error("no exposure gesture"))?;
        let candidate = inner
            .editor
            .preview_exposure(interaction, stops)
            .map_err(command_error)?;
        if let Some(renderer) = &mut inner.renderer {
            let scene = renderer
                .prepare_document(candidate.document().expect("exposure candidate"), false)
                .map_err(render_error)?;
            renderer.commit_document(scene).map_err(render_error)?;
        }
        Ok(())
    }
    pub fn end_exposure(&self, stops: f64, cancel: bool) -> Result<(), StrataError> {
        {
            let mut inner = self.lock()?;
            let interaction = inner
                .exposure
                .take()
                .ok_or_else(|| render_error("no exposure gesture"))?;
            if cancel {
                restore_scene(&mut inner)?;
            } else {
                let candidate = inner
                    .editor
                    .preview_exposure(&interaction, stops)
                    .map_err(command_error)?;
                if let Err(error) = apply(&mut inner, candidate, "adjust.exposure.set", &json!({}))
                {
                    restore_scene(&mut inner)?;
                    return Err(error);
                }
            }
        }
        self.deliver_events()
    }
    /// Bounded display-only selection contour. Revision checked before and after
    /// work; sampled coverage never replaces the document's u16 mask plane.
    pub fn selection_outline(&self, revision: u64) -> Result<Vec<SelectionEdge>, StrataError> {
        let doc = {
            let inner = self.lock()?;
            if inner.editor.revision() != revision {
                return Ok(vec![]);
            }
            inner.editor.document().cloned()
        };
        let Some(doc) = doc else { return Ok(vec![]) };
        let Some(mask) = doc.selection() else {
            return Ok(vec![]);
        };
        let _scratch = doc
            .memory_pool()
            .reserve(
                strata_pixels::memory::MemoryKind::Transient,
                12 * 1024 * 1024,
            )
            .map_err(render_error)?;
        let result = outline(mask).map_err(render_error)?;
        if self.lock()?.editor.revision() != revision {
            return Ok(vec![]);
        }
        Ok(result)
    }
    /// Display-only candidate coverage. Document/AI selection queries remain
    /// anchored to editor truth until Apply commits the prepared transform.
    pub fn display_selection_outline(
        &self,
        revision: u64,
    ) -> Result<Vec<SelectionEdge>, StrataError> {
        use std::sync::atomic::Ordering;
        let sequence = self.tonal_preview_sequence.load(Ordering::Acquire);
        let doc = {
            let inner = self.lock()?;
            if inner.editor.revision() != revision {
                return Ok(vec![]);
            }
            inner
                .tonal_preview
                .as_ref()
                .and_then(|p| p.candidate.as_ref())
                .and_then(|p| p.selection_document())
                .cloned()
                .or_else(|| inner.editor.document().cloned())
        };
        let Some(doc) = doc else {
            return Ok(vec![]);
        };
        let Some(mask) = doc.selection() else {
            return Ok(vec![]);
        };
        let _scratch = doc
            .memory_pool()
            .reserve(
                strata_pixels::memory::MemoryKind::Transient,
                12 * 1024 * 1024,
            )
            .map_err(render_error)?;
        let result = outline(mask).map_err(render_error)?;
        if self.tonal_preview_sequence.load(Ordering::Acquire) != sequence
            || self.lock()?.editor.revision() != revision
        {
            return Ok(vec![]);
        }
        Ok(result)
    }
}
fn restore_scene(inner: &mut Inner) -> Result<(), StrataError> {
    let doc = inner.editor.document().cloned();
    if let (Some(doc), Some(renderer)) = (doc, &mut inner.renderer) {
        let scene = renderer
            .prepare_document(&doc, false)
            .map_err(render_error)?;
        renderer.commit_document(scene).map_err(render_error)?;
    }
    Ok(())
}
fn outline(mask: &TiledMask) -> Result<Vec<SelectionEdge>, strata_pixels::TileError> {
    let frame = mask.layout().bounds;
    let Some(bounds) = mask.content_bounds()? else {
        return Ok(vec![]);
    };
    let width = bounds.width.min(256) as usize;
    let height = bounds.height.min(256) as usize;
    let mut samples = Vec::with_capacity(width * height);
    for y in 0..height {
        for x in 0..width {
            let sx = bounds.x
                + (((x as u64 * 2 + 1) * u64::from(bounds.width)) / (width as u64 * 2)) as i64;
            let sy = bounds.y
                + (((y as u64 * 2 + 1) * u64::from(bounds.height)) / (height as u64 * 2)) as i64;
            let (coord, [px, py]) = TileCoord::locate(sx, sy);
            samples.push((
                coord,
                usize::from(py) * 256 + usize::from(px),
                y * width + x,
            ));
        }
    }
    samples.sort_unstable_by_key(|s| s.0);
    let mut values = vec![false; width * height];
    let mut last = None;
    let layout = mask.layout();
    let baseline = layout.default_coverage ^ if layout.inverted { 65535 } else { 0 };
    for (coord, source, dest) in samples {
        if last.as_ref().is_none_or(|(c, _)| *c != coord) {
            last = Some((coord, mask.encoded_tile(coord)?));
        }
        let encoded = last
            .as_ref()
            .and_then(|(_, tile)| tile.as_ref())
            .map_or(0, |tile| tile[source]);
        values[dest] = (encoded ^ baseline) >= 32768;
    }
    let mut edges = Vec::new();
    let x = |i: usize| (bounds.x - frame.x) as f32 + i as f32 * bounds.width as f32 / width as f32;
    let y =
        |i: usize| (bounds.y - frame.y) as f32 + i as f32 * bounds.height as f32 / height as f32;
    for row in 0..=height {
        for col in 0..width {
            let above = row > 0 && values[(row - 1) * width + col];
            let below = row < height && values[row * width + col];
            if above != below {
                edges.push(SelectionEdge {
                    x1: x(col),
                    y1: y(row),
                    x2: x(col + 1),
                    y2: y(row),
                });
            }
        }
    }
    for col in 0..=width {
        for row in 0..height {
            let left = col > 0 && values[row * width + col - 1];
            let right = col < width && values[row * width + col];
            if left != right {
                edges.push(SelectionEdge {
                    x1: x(col),
                    y1: y(row),
                    x2: x(col),
                    y2: y(row + 1),
                });
            }
        }
    }
    Ok(edges)
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Listener;
    impl EngineListener for Listener {
        fn on_event(&self, _: EngineEventDto) {}
    }
    #[test]
    fn fill_gesture_and_sampler_are_revision_bound_and_sampler_never_edits_history() {
        let engine = StrataEngine::new(Arc::new(Listener));
        engine.dispatch("file.new".into(),json!({"width":16,"height":16,"bit_depth":16,"background":[32768,32769,50001,65535]}).to_string()).unwrap();
        let state = engine.lock().unwrap().editor.snapshot();
        let id = state.active_layer.unwrap().to_string();
        let color: Value = serde_json::from_str(
            &engine
                .sample_color(
                    json!({"x":1,"y":1,"sample":"current_layer"}).to_string(),
                    state.revision,
                )
                .unwrap(),
        )
        .unwrap();
        assert_eq!(color["color"][1], json!(32769.0 / 65535.0));
        assert_eq!(engine.lock().unwrap().editor.snapshot().history.len(), 0);
        engine.begin_paint(id.clone(), "paint.fill".into()).unwrap();
        engine
            .end_paint(json!({"color":[1,0,0,1]}).to_string(), false)
            .unwrap();
        assert_eq!(engine.lock().unwrap().editor.snapshot().history.len(), 1);
        assert!(
            engine
                .sample_color(json!({"x":1,"y":1}).to_string(), state.revision)
                .is_err()
        );
        engine
            .begin_paint(id.clone(), "paint.gradient".into())
            .unwrap();
        engine.end_paint("{}".into(), true).unwrap();
        assert_eq!(engine.lock().unwrap().editor.snapshot().history.len(), 1);
        engine.begin_paint(id, "paint.bucket".into()).unwrap();
        engine.dispatch("select.all".into(), "{}".into()).unwrap();
        let history = engine.lock().unwrap().editor.snapshot().history.len();
        assert!(
            engine
                .end_paint(json!({"x":1,"y":1,"color":[0,1,0,1]}).to_string(), false)
                .is_err()
        );
        assert_eq!(
            engine.lock().unwrap().editor.snapshot().history.len(),
            history
        );
    }
    #[test]
    fn tonal_brush_commands_are_whitelisted_and_nested_gestures_are_revision_bound() {
        for command in ["retouch.dodge", "retouch.burn", "retouch.sponge"] {
            let engine = StrataEngine::new(Arc::new(Listener));
            engine.dispatch("file.new".into(),json!({"width":16,"height":16,"bit_depth":16,"background":[20000,30000,40000,32768]}).to_string()).unwrap();
            let id = engine
                .lock()
                .unwrap()
                .editor
                .snapshot()
                .active_layer
                .unwrap()
                .to_string();
            assert!(
                engine
                    .begin_tonal_brush(id.clone(), "paint.stroke".into())
                    .is_err()
            );
            engine
                .begin_tonal_brush(id.clone(), command.into())
                .unwrap();
            let mut payload = json!({"stroke":{"points":[{"x":8.5,"y":8.5}],"size":4}});
            engine.end_brush(payload.to_string(), false).unwrap();
            assert_eq!(
                engine
                    .lock()
                    .unwrap()
                    .editor
                    .snapshot()
                    .history
                    .last()
                    .unwrap()
                    .command,
                command
            );
            engine
                .begin_tonal_brush(id.clone(), command.into())
                .unwrap();
            payload["stroke"]["target"] = json!("mask");
            assert!(engine.end_brush(payload.to_string(), false).is_err());
            payload["stroke"].as_object_mut().unwrap().remove("target");
            engine.begin_tonal_brush(id, command.into()).unwrap();
            engine.dispatch("select.all".into(), "{}".into()).unwrap();
            let history = engine.lock().unwrap().editor.snapshot().history.len();
            assert!(engine.end_brush(payload.to_string(), false).is_err());
            assert_eq!(
                engine.lock().unwrap().editor.snapshot().history.len(),
                history
            );
        }
    }
    #[test]
    fn automatic_spot_gesture_anchors_nested_target_and_rejects_stale_input() {
        let engine = StrataEngine::new(Arc::new(Listener));
        engine.dispatch("file.new".into(),json!({"width":16,"height":16,"bit_depth":16,"background":[20000,30000,40000,65535]}).to_string()).unwrap();
        let id = engine
            .lock()
            .unwrap()
            .editor
            .snapshot()
            .active_layer
            .unwrap()
            .to_string();
        let payload = json!({"stroke":{"points":[{"x":10.5,"y":8.5}],"size":2,"hardness":1},"search_radius":8});
        engine.begin_spot_heal(id.clone()).unwrap();
        engine.end_brush("{}".into(), true).unwrap();
        assert_eq!(engine.lock().unwrap().editor.snapshot().history.len(), 0);
        for mode in ["proximity_match", "create_texture", "content_aware"] {
            let mut input = payload.clone();
            input["mode"] = json!(mode);
            engine.begin_spot_heal(id.clone()).unwrap();
            engine.end_brush(input.to_string(), false).unwrap();
            assert_eq!(
                engine
                    .lock()
                    .unwrap()
                    .editor
                    .snapshot()
                    .history
                    .last()
                    .unwrap()
                    .command,
                "retouch.spot_heal"
            );
        }
        let mut bad = payload.clone();
        bad["stroke"]["target"] = json!("mask");
        engine.begin_spot_heal(id.clone()).unwrap();
        assert!(engine.end_brush(bad.to_string(), false).is_err());
        engine.begin_spot_heal(id).unwrap();
        engine.dispatch("select.all".into(), "{}".into()).unwrap();
        let history = engine.lock().unwrap().editor.snapshot().history.len();
        assert!(engine.end_brush(payload.to_string(), false).is_err());
        assert_eq!(
            engine.lock().unwrap().editor.snapshot().history.len(),
            history
        );
    }
    #[test]
    fn healing_nested_gesture_cannot_retarget_or_commit_after_revision_change() {
        let engine = StrataEngine::new(Arc::new(Listener));
        engine
            .dispatch(
                "file.new".into(),
                json!({"width":16,"height":16,"bit_depth":16}).to_string(),
            )
            .unwrap();
        let id = engine
            .lock()
            .unwrap()
            .editor
            .snapshot()
            .active_layer
            .unwrap()
            .to_string();
        let stroke =
            json!({"points":[{"x":10.5,"y":8.5}],"size":4,"clone_source":{"source":[2.5,8.5]}});
        engine.begin_heal(id.clone()).unwrap();
        engine.end_brush("{}".into(), true).unwrap();
        assert_eq!(engine.lock().unwrap().editor.snapshot().history.len(), 0);
        engine.begin_heal(id.clone()).unwrap();
        let mut invalid = stroke.clone();
        invalid["layer"] = json!({"name":"Background"});
        assert!(
            engine
                .end_brush(json!({"stroke":invalid}).to_string(), false)
                .is_err()
        );
        assert_eq!(engine.lock().unwrap().editor.snapshot().history.len(), 0);
        engine.begin_heal(id.clone()).unwrap();
        engine.dispatch("select.all".into(), "{}".into()).unwrap();
        let history = engine.lock().unwrap().editor.snapshot().history.len();
        assert!(
            engine
                .end_brush(json!({"stroke":stroke}).to_string(), false)
                .is_err()
        );
        assert_eq!(
            engine.lock().unwrap().editor.snapshot().history.len(),
            history
        );
        engine.begin_heal(id).unwrap();
        assert!(engine.end_brush(stroke.to_string(), false).is_err());
    }
    #[test]
    fn brush_gestures_anchor_target_and_reject_late_or_retargeted_commits() {
        let engine = StrataEngine::new(Arc::new(Listener));
        engine
            .dispatch(
                "file.new".into(),
                json!({"width":16,"height":16,"bit_depth":16}).to_string(),
            )
            .unwrap();
        let id = engine
            .lock()
            .unwrap()
            .editor
            .snapshot()
            .active_layer
            .unwrap()
            .to_string();
        let parameters =
            json!({"points":[{"x":4.5,"y":4.5},{"x":10.5,"y":10.5}],"size":4,"color":[1,0,0,1]})
                .to_string();
        engine
            .begin_brush(id.clone(), "pixels".into(), false)
            .unwrap();
        engine.end_brush("{}".into(), true).unwrap();
        assert_eq!(engine.lock().unwrap().editor.snapshot().history.len(), 0);
        engine
            .begin_brush(id.clone(), "pixels".into(), false)
            .unwrap();
        engine.end_brush(parameters.clone(), false).unwrap();
        assert_eq!(engine.lock().unwrap().editor.snapshot().history.len(), 1);
        engine
            .begin_brush(id.clone(), "pixels".into(), true)
            .unwrap();
        engine.end_brush(parameters.clone(), false).unwrap();
        assert_eq!(
            engine
                .lock()
                .unwrap()
                .editor
                .snapshot()
                .history
                .last()
                .unwrap()
                .command,
            "paint.erase"
        );
        engine
            .begin_brush(id.clone(), "pixels".into(), false)
            .unwrap();
        engine
            .dispatch(
                "layer.rename".into(),
                json!({"layer":{"id":id},"name":"renamed"}).to_string(),
            )
            .unwrap();
        let history = engine.lock().unwrap().editor.snapshot().history.len();
        assert!(engine.end_brush(parameters, false).is_err());
        assert_eq!(
            engine.lock().unwrap().editor.snapshot().history.len(),
            history
        );
        engine.begin_brush(id, "pixels".into(), false).unwrap();
        assert!(
            engine
                .end_brush(
                    json!({"target":"mask","points":[{"x":4,"y":4}]}).to_string(),
                    false
                )
                .is_err()
        );
        assert_eq!(
            engine.lock().unwrap().editor.snapshot().history.len(),
            history
        );
    }
    #[test]
    fn cancelling_never_waits_for_the_editor_mutex_and_late_commit_stays_rejected() {
        let engine = StrataEngine::new(Arc::new(Listener));
        engine
            .dispatch(
                "file.new".into(),
                json!({"width":8,"height":8,"bit_depth":16}).to_string(),
            )
            .unwrap();
        let candidate = {
            let inner = engine.lock().unwrap();
            inner
                .editor
                .prepare(
                    inner.editor.request(Source::User),
                    "layer.create",
                    json!({"name":"late"}),
                )
                .unwrap()
        };
        let locked = engine.lock().unwrap();
        let (sender, receiver) = std::sync::mpsc::channel();
        let worker = engine.clone();
        let thread = std::thread::spawn(move || {
            sender.send(worker.cancel_pending_work()).unwrap();
        });
        let signal = receiver.recv_timeout(std::time::Duration::from_secs(1));
        drop(locked);
        thread.join().unwrap();
        assert!(
            signal.is_ok(),
            "cancellation blocked on the editor/render mutex"
        );
        signal.unwrap().unwrap();
        assert!(matches!(
            engine.lock().unwrap().editor.commit(candidate),
            Err(CommandError::Expired)
        ));
        engine
            .dispatch(
                "layer.create".into(),
                json!({"name":"new request"}).to_string(),
            )
            .unwrap();
        let state = engine.lock().unwrap().editor.snapshot();
        assert_eq!(state.layers.len(), 2);
        assert_eq!(state.history.len(), 1);
    }
    #[test]
    fn selection_gestures_reject_late_commits_wrong_commands_and_cancellation() {
        let engine = StrataEngine::new(Arc::new(Listener));
        engine
            .dispatch(
                "file.new".into(),
                json!({"width":16,"height":16,"bit_depth":16}).to_string(),
            )
            .unwrap();
        let params = json!({"x":2,"y":2,"width":8,"height":8}).to_string();
        engine.begin_marquee().unwrap();
        engine.dispatch("select.all".into(), "{}".into()).unwrap();
        let history = engine.lock().unwrap().editor.snapshot().history.len();
        assert!(
            engine
                .end_selection("select.ellipse".into(), params.clone(), false)
                .is_err()
        );
        assert_eq!(
            engine.lock().unwrap().editor.snapshot().history.len(),
            history
        );
        engine.begin_marquee().unwrap();
        assert!(
            engine
                .end_selection("layer.delete".into(), "{}".into(), false)
                .is_err()
        );
        engine.begin_marquee().unwrap();
        engine.cancel_pending_work().unwrap();
        assert!(
            engine
                .end_selection("select.ellipse".into(), params.clone(), false)
                .is_err()
        );
        engine.begin_marquee().unwrap();
        engine
            .end_selection("select.ellipse".into(), params, false)
            .unwrap();
        assert_eq!(
            engine.lock().unwrap().editor.snapshot().history.len(),
            history + 1
        );
    }
    #[test]
    fn marquee_and_exposure_gestures_commit_once_and_cancel_without_history() {
        let engine = StrataEngine::new(Arc::new(Listener));
        engine
            .dispatch(
                "file.new".into(),
                json!({"width":8,"height":8,"bit_depth":16}).to_string(),
            )
            .unwrap();
        engine.begin_marquee().unwrap();
        engine
            .end_marquee(json!({"x":1,"y":2,"width":3,"height":4}).to_string(), false)
            .unwrap();
        let state = engine.lock().unwrap().editor.snapshot();
        assert_eq!(state.history.len(), 1);
        let edges = engine.selection_outline(state.revision).unwrap();
        assert!(
            edges
                .iter()
                .all(|e| e.x1 >= 1.0 && e.y1 >= 2.0 && e.x2 <= 4.0 && e.y2 <= 6.0)
        );
        assert_eq!(edges.len(), 14);
        let layer: Value = serde_json::from_str(
            &engine
                .dispatch(
                    "layer.add_adjustment".into(),
                    json!({"name":"exposure","type":"exposure","mask":"from_selection"})
                        .to_string(),
                )
                .unwrap(),
        )
        .unwrap();
        let id = layer["layer_id"].as_str().unwrap().to_string();
        engine.begin_exposure(id.clone()).unwrap();
        engine.preview_exposure(0.5).unwrap();
        engine.preview_exposure(1.0).unwrap();
        engine.end_exposure(1.0, true).unwrap();
        assert_eq!(engine.lock().unwrap().editor.snapshot().history.len(), 2);
        engine.begin_exposure(id).unwrap();
        engine.preview_exposure(0.7).unwrap();
        engine.end_exposure(0.7, false).unwrap();
        assert_eq!(engine.lock().unwrap().editor.snapshot().history.len(), 3);
        engine.begin_marquee().unwrap();
        engine.end_marquee("{}".into(), true).unwrap();
        assert_eq!(engine.lock().unwrap().editor.snapshot().history.len(), 3);
    }
}
