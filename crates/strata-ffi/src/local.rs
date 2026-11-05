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
impl StrataEngine {
    pub fn begin_marquee(&self) -> Result<(), StrataError> {
        let mut inner = self.lock()?;
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
        let mut inner = self.lock()?;
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
