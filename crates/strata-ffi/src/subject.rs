use crate::*;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{OnceLock, Weak},
};
use strata_core::{SubjectParams, SubjectSelection, SubjectSelector};
use strata_ml::{ExecutionBackend, SubjectModel};
use strata_pixels::{CoverageRect, MaskLayout, TileCoord, TiledMask, memory::MemoryKind};

struct NativeSubject {
    runtime: PathBuf,
    runtime_hash: String,
    model_path: PathBuf,
    backend: ExecutionBackend,
    model: Mutex<Option<SubjectModel>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Listener;
    impl EngineListener for Listener {
        fn on_event(&self, _: EngineEventDto) {}
    }
    #[test]
    #[ignore = "requires verified ONNX assets and explicit local inference acceptance"]
    fn real_subject_command_exposure_undo_and_reopen_keep_original_pixels() {
        let runtime = std::env::var("STRATA_TEST_SUBJECT_RUNTIME").expect("runtime path");
        let hash = std::env::var("STRATA_TEST_SUBJECT_RUNTIME_SHA256").expect("runtime hash");
        let model = std::env::var("STRATA_TEST_SUBJECT_MODEL").expect("model path");
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap();
        let engine = StrataEngine::new(Arc::new(Listener));
        engine
            .open_image(
                root.join("docs/design/assets/sample-photo.jpg")
                    .to_string_lossy()
                    .into_owned(),
            )
            .unwrap();
        engine
            .configure_subject_model(runtime, hash, model, "cpu".into())
            .unwrap();
        let before = engine.lock().unwrap().editor.document().unwrap().layers()[0]
            .pixels()
            .clone();
        let selection: Value = serde_json::from_str(
            &engine
                .dispatch("select.subject".into(), "{}".into())
                .unwrap(),
        )
        .unwrap();
        assert_eq!(selection["diagnostics"]["components"], 1);
        let inspected = engine.lock().unwrap().editor.snapshot();
        assert!(inspected.selection_extent.is_some());
        engine
            .dispatch(
                "layer.add_adjustment".into(),
                json!({"name":"人物提亮","type":"exposure","stops":0.5,"mask":"from_selection"})
                    .to_string(),
            )
            .unwrap();
        let expected = engine
            .lock()
            .unwrap()
            .editor
            .document()
            .unwrap()
            .to_archive()
            .unwrap();
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("subject.strata");
        let resource = engine
            .grant_write(path.to_string_lossy().into_owned(), false)
            .unwrap();
        engine
            .dispatch(
                "file.save_as".into(),
                json!({"resource":resource}).to_string(),
            )
            .unwrap();
        engine.dispatch("edit.undo".into(), "{}".into()).unwrap();
        let resource = engine
            .grant_read(path.to_string_lossy().into_owned())
            .unwrap();
        engine
            .dispatch(
                "file.open".into(),
                json!({"resource":resource,"discard_changes":true}).to_string(),
            )
            .unwrap();
        let inner = engine.lock().unwrap();
        let doc = inner.editor.document().unwrap();
        let actual = doc.to_archive().unwrap();
        assert_eq!(
            actual.metadata, expected.metadata,
            "project metadata must survive"
        );
        assert!(
            actual.tiles == expected.tiles,
            "project pixel and coverage tiles must survive"
        );
        for coord in before.tile_coords() {
            assert_eq!(
                *before.tile(*coord).unwrap().unwrap(),
                *doc.layers()[0].pixels().tile(*coord).unwrap().unwrap()
            );
        }
    }
}
#[uniffi::export]
pub fn subject_model_asset_json() -> String {
    serde_json::to_string(&strata_ml::assets::SUBJECT_MODEL).expect("fixed asset")
}
#[uniffi::export]
pub fn verify_subject_model(path: String) -> Result<(), StrataError> {
    strata_ml::assets::verify(
        Path::new(&path),
        strata_ml::assets::SUBJECT_MODEL.sha256,
        strata_ml::assets::SUBJECT_MODEL.max_bytes,
    )
    .map_err(render_error)
}
#[uniffi::export]
impl StrataEngine {
    /// Trusted native bundle runtime and app-owned model location, never model
    /// tool parameters. Loading/inference remains lazy and cancellable.
    pub fn configure_subject_model(
        &self,
        runtime_path: String,
        runtime_hash: String,
        model_path: String,
        backend: String,
    ) -> Result<(), StrataError> {
        if runtime_hash.len() != 64 || !runtime_hash.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(render_error("invalid bundled runtime fingerprint"));
        }
        let execution = match backend.as_str() {
            "cpu" => ExecutionBackend::Cpu,
            "core_ml" => ExecutionBackend::CoreMl,
            _ => return Err(render_error("unknown model backend")),
        };
        static SERVICES: OnceLock<Mutex<HashMap<String, Weak<NativeSubject>>>> = OnceLock::new();
        let services = SERVICES.get_or_init(|| Mutex::new(HashMap::new()));
        let key = format!("{runtime_path}|{runtime_hash}|{model_path}|{backend}");
        let mut services = services.lock().map_err(render_error)?;
        services.retain(|_, service| service.strong_count() > 0);
        let service = services
            .get(&key)
            .and_then(Weak::upgrade)
            .unwrap_or_else(|| {
                let service = Arc::new(NativeSubject {
                    runtime: PathBuf::from(runtime_path),
                    runtime_hash,
                    model_path: PathBuf::from(model_path),
                    backend: execution,
                    model: Mutex::new(None),
                });
                services.insert(key, Arc::downgrade(&service));
                service
            });
        self.lock()?.editor.set_subject_selector(service);
        Ok(())
    }
    pub fn cancel_pending_work(&self) -> Result<(), StrataError> {
        self.work_cancellation.cancel();
        Ok(())
    }
    pub fn clear_subject_model(&self) -> Result<(), StrataError> {
        self.lock()?.editor.clear_subject_selector();
        Ok(())
    }
}
impl SubjectSelector for NativeSubject {
    fn select(
        &self,
        doc: &Document,
        params: &SubjectParams,
        cancel: &(dyn Fn() -> bool + Sync),
    ) -> Result<SubjectSelection, String> {
        self.select_inner(doc, params, cancel)
            .map_err(|e| e.to_string())
    }
}
impl NativeSubject {
    fn select_inner(
        &self,
        doc: &Document,
        params: &SubjectParams,
        cancel: &(dyn Fn() -> bool + Sync),
    ) -> Result<SubjectSelection, StrataError> {
        let preview = doc
            .preview_srgb(params.region, 1024, &cancel)
            .map_err(render_error)?;
        let mut model = self.model.lock().map_err(render_error)?;
        if model.is_none() {
            if cancel() {
                return Err(render_error("subject selection cancelled"));
            }
            *model = Some(
                SubjectModel::load(
                    &self.runtime,
                    &self.runtime_hash,
                    &self.model_path,
                    self.backend,
                    doc.memory_pool(),
                )
                .map_err(render_error)?,
            );
        }
        let result = model
            .as_ref()
            .expect("model loaded")
            .infer(&preview.rgba, preview.width, preview.height, cancel)
            .map_err(render_error)?;
        drop(model);
        drop(preview);
        let frame = doc.frame();
        let bounds = CoverageRect {
            x: frame.x,
            y: frame.y,
            width: frame.width,
            height: frame.height,
        };
        let region = params.region.unwrap_or(bounds);
        let mut mask = TiledMask::new(
            MaskLayout {
                bounds,
                default_coverage: 0,
                inverted: false,
            },
            (strata_core::MAX_DOCUMENT_BYTES / (256 * 256 * 2)) as usize,
            doc.memory_pool(),
        )
        .map_err(render_error)?;
        let mut diagnostics = serde_json::to_value(&result).map_err(render_error)?;
        diagnostics["model"] = json!("u2netp");
        diagnostics["scope"] = json!(region);
        diagnostics["interpretation"] = json!(
            "Salient foreground mask; not person identification. Multiple components and uncertain coverage require user clarification."
        );
        if result.needs_confirmation && !params.accept_ambiguous {
            return Ok(SubjectSelection {
                mask,
                diagnostics,
                needs_confirmation: true,
            });
        }
        let right = region.x + i64::from(region.width);
        let bottom = region.y + i64::from(region.height);
        let (first, _) = TileCoord::locate(region.x, region.y);
        let (last, _) = TileCoord::locate(right - 1, bottom - 1);
        for tx in first.x()..=last.x() {
            for ty in first.y()..=last.y() {
                if cancel() {
                    return Err(render_error("subject selection cancelled"));
                }
                let _scratch = doc
                    .memory_pool()
                    .reserve(MemoryKind::Transient, 256 * 256 * 2)
                    .map_err(render_error)?;
                let coord = TileCoord::new(tx, ty).map_err(render_error)?;
                let [ox, oy] = coord.origin();
                let mut values = vec![0; 256 * 256];
                for y in region.y.max(oy)..bottom.min(oy.saturating_add(256)) {
                    for x in region.x.max(ox)..right.min(ox.saturating_add(256)) {
                        let sx = (((x - region.x) as f64 + 0.5) * f64::from(result.width)
                            / f64::from(region.width)
                            - 0.5)
                            .clamp(0.0, f64::from(result.width - 1));
                        let sy = (((y - region.y) as f64 + 0.5) * f64::from(result.height)
                            / f64::from(region.height)
                            - 0.5)
                            .clamp(0.0, f64::from(result.height - 1));
                        let x0 = sx.floor() as u32;
                        let y0 = sy.floor() as u32;
                        let fx = sx - f64::from(x0);
                        let fy = sy - f64::from(y0);
                        let mut value = 0.0;
                        for (px, wx) in [(x0, 1.0 - fx), ((x0 + 1).min(result.width - 1), fx)] {
                            for (py, wy) in [(y0, 1.0 - fy), ((y0 + 1).min(result.height - 1), fy)]
                            {
                                value +=
                                    f64::from(result.coverage[(py * result.width + px) as usize])
                                        * wx
                                        * wy;
                            }
                        }
                        values[(y - oy) as usize * 256 + (x - ox) as usize] = value.round() as u16;
                    }
                }
                mask.insert_tile(coord, values).map_err(render_error)?;
            }
        }
        Ok(SubjectSelection {
            mask,
            diagnostics,
            needs_confirmation: result.needs_confirmation,
        })
    }
}
