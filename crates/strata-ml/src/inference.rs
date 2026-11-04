use crate::{MlError, assets};
use ort::{
    execution_providers::{CoreMLExecutionProvider, ExecutionProvider},
    session::{RunOptions, Session},
    value::Tensor,
};
use serde::Serialize;
use std::{
    path::Path,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use strata_pixels::memory::{MemoryKind, MemoryPool};

const SIDE: usize = 320;
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionBackend {
    Cpu,
    CoreMl,
}
#[derive(Serialize)]
pub struct Segmentation {
    pub width: u32,
    pub height: u32,
    #[serde(skip)]
    pub coverage: Vec<u16>,
    pub confident_fraction: f64,
    pub foreground_fraction: f64,
    pub components: usize,
    pub needs_confirmation: bool,
    pub backend: ExecutionBackend,
}
pub struct SubjectModel {
    session: Mutex<Session>,
    backend: ExecutionBackend,
    pool: MemoryPool,
    _session_budget: strata_pixels::memory::Reservation,
}
impl SubjectModel {
    /// Host paths only. The pinned runtime/model are checked before loading code.
    /// Alternate platform packages must provide their own verified runtime hash.
    pub fn load(
        runtime: &Path,
        runtime_sha256: &str,
        model: &Path,
        backend: ExecutionBackend,
        pool: MemoryPool,
    ) -> Result<Self, MlError> {
        assets::verify(runtime, runtime_sha256, 128 * 1024 * 1024)?;
        assets::verify(
            model,
            assets::SUBJECT_MODEL.sha256,
            assets::SUBJECT_MODEL.max_bytes,
        )?;
        static INITIALIZED: OnceLock<std::path::PathBuf> = OnceLock::new();
        static INIT: Mutex<()> = Mutex::new(());
        let _guard = INIT
            .lock()
            .map_err(|_| MlError::Runtime("initialization lock poisoned".into()))?;
        let canonical = std::fs::canonicalize(runtime)?;
        if let Some(previous) = INITIALIZED.get() {
            if previous != &canonical {
                return Err(MlError::Invalid(
                    "ONNX Runtime already initialized from a different package".into(),
                ));
            }
        } else {
            ort::init_from(canonical.to_string_lossy())
                .with_name("Strata subject selection")
                .with_telemetry(false)
                .commit()
                .map_err(runtime_error)?;
            let _ = INITIALIZED.set(canonical);
        }
        let budget = pool
            .reserve(MemoryKind::Transient, 128 * 1024 * 1024)
            .map_err(|e| MlError::Runtime(e.to_string()))?;
        let mut builder = Session::builder()
            .map_err(runtime_error)?
            .with_intra_threads(2)
            .map_err(runtime_error)?
            .with_inter_threads(1)
            .map_err(runtime_error)?;
        if matches!(backend, ExecutionBackend::CoreMl) {
            let ep = CoreMLExecutionProvider::default().with_static_input_shapes(true);
            if !ep.is_available().map_err(runtime_error)? {
                return Err(MlError::Runtime(
                    "Core ML provider is unavailable in this runtime package; use CPU".into(),
                ));
            }
            builder = builder
                .with_execution_providers([ep.build().error_on_failure()])
                .map_err(runtime_error)?;
        }
        let session = builder.commit_from_file(model).map_err(runtime_error)?;
        if session.inputs.len() != 1 || session.outputs.is_empty() {
            return Err(MlError::Invalid("unexpected model IO".into()));
        }
        Ok(Self {
            session: Mutex::new(session),
            backend,
            pool,
            _session_budget: budget,
        })
    }
    pub fn infer(
        &self,
        rgba: &[u8],
        width: u32,
        height: u32,
        cancel: &(dyn Fn() -> bool + Sync),
    ) -> Result<Segmentation, MlError> {
        if width == 0
            || height == 0
            || width > 2048
            || height > 2048
            || rgba.len() != width as usize * height as usize * 4
        {
            return Err(MlError::Invalid("bounded RGBA8 preview required".into()));
        }
        if cancel() {
            return Err(MlError::Cancelled);
        }
        let _budget = self
            .pool
            .reserve(MemoryKind::Transient, 192 * 1024 * 1024)
            .map_err(|e| MlError::Runtime(e.to_string()))?;
        let input = preprocess(rgba, width, height);
        let tensor = Tensor::from_array(([1, 3, SIDE, SIDE], input)).map_err(runtime_error)?;
        let run = Arc::new(RunOptions::new().map_err(runtime_error)?);
        let done = AtomicBool::new(false);
        let result = std::thread::scope(|scope| {
            let cancel_run = run.clone();
            let done = &done;
            scope.spawn(move || {
                while !done.load(Ordering::Acquire) {
                    if cancel() {
                        let _ = cancel_run.terminate();
                        break;
                    }
                    std::thread::park_timeout(Duration::from_millis(15));
                }
            });
            let output = (|| {
                let mut session = self
                    .session
                    .lock()
                    .map_err(|_| MlError::Runtime("model session poisoned".into()))?;
                if cancel() {
                    return Err(MlError::Cancelled);
                }
                let output = session
                    .run_with_options(ort::inputs![tensor], &run)
                    .map_err(runtime_error)?;
                let (shape, data) = output[0]
                    .try_extract_tensor::<f32>()
                    .map_err(runtime_error)?;
                if shape.as_ref() != [1, 1, SIDE as i64, SIDE as i64]
                    || data.len() != SIDE * SIDE
                    || data.iter().any(|v| !v.is_finite())
                {
                    return Err(MlError::Invalid(
                        "unexpected model output shape/values".into(),
                    ));
                }
                Ok(data.to_vec())
            })();
            done.store(true, Ordering::Release);
            output
        });
        if cancel() {
            return Err(MlError::Cancelled);
        }
        postprocess(&result?, self.backend)
    }
}
fn runtime_error(e: impl std::fmt::Display) -> MlError {
    MlError::Runtime(e.to_string())
}
fn preprocess(rgba: &[u8], width: u32, height: u32) -> Vec<f32> {
    let mut pixels = vec![[0.0f32; 3]; SIDE * SIDE];
    let mut maximum = 0.0f32;
    for y in 0..SIDE {
        for x in 0..SIDE {
            let sx = ((x as f32 + 0.5) * width as f32 / SIDE as f32 - 0.5)
                .clamp(0.0, width as f32 - 1.0);
            let sy = ((y as f32 + 0.5) * height as f32 / SIDE as f32 - 0.5)
                .clamp(0.0, height as f32 - 1.0);
            let x0 = sx.floor() as u32;
            let y0 = sy.floor() as u32;
            let fx = sx - x0 as f32;
            let fy = sy - y0 as f32;
            for (px, wx) in [(x0, 1.0 - fx), ((x0 + 1).min(width - 1), fx)] {
                for (py, wy) in [(y0, 1.0 - fy), ((y0 + 1).min(height - 1), fy)] {
                    let index = (py * width + px) as usize * 4;
                    let alpha = f32::from(rgba[index + 3]) / 255.0;
                    for c in 0..3 {
                        pixels[y * SIDE + x][c] += f32::from(rgba[index + c]) * alpha * wx * wy;
                    }
                }
            }
            maximum = maximum.max(pixels[y * SIDE + x].iter().copied().fold(0.0, f32::max));
        }
    }
    let mut out = vec![0.0; SIDE * SIDE * 3];
    for (i, pixel) in pixels.into_iter().enumerate() {
        for c in 0..3 {
            out[c * SIDE * SIDE + i] =
                (pixel[c] / maximum.max(1.0) - [0.485, 0.456, 0.406][c]) / [0.229, 0.224, 0.225][c];
        }
    }
    out
}
fn postprocess(values: &[f32], backend: ExecutionBackend) -> Result<Segmentation, MlError> {
    let min = values.iter().copied().fold(f32::INFINITY, f32::min);
    let max = values.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    if max - min < 1e-6 {
        return Err(MlError::Invalid(
            "model did not find a distinguishable subject; ask for a rectangle".into(),
        ));
    }
    let normalized: Vec<_> = values
        .iter()
        .map(|v| ((v - min) / (max - min)).clamp(0.0, 1.0))
        .collect();
    let confident_fraction = normalized
        .iter()
        .filter(|v| **v <= 0.1 || **v >= 0.9)
        .count() as f64
        / values.len() as f64;
    let foreground_fraction =
        normalized.iter().filter(|v| **v >= 0.5).count() as f64 / values.len() as f64;
    let mut seen = vec![false; SIDE * SIDE];
    let mut components = 0;
    for start in 0..SIDE * SIDE {
        if seen[start] || normalized[start] < 0.5 {
            continue;
        }
        let mut queue = vec![start];
        seen[start] = true;
        let mut area = 0;
        while let Some(i) = queue.pop() {
            area += 1;
            let x = i % SIDE;
            let y = i / SIDE;
            for next in [
                if x > 0 { Some(i - 1) } else { None },
                if x + 1 < SIDE { Some(i + 1) } else { None },
                if y > 0 { Some(i - SIDE) } else { None },
                if y + 1 < SIDE { Some(i + SIDE) } else { None },
            ]
            .into_iter()
            .flatten()
            {
                if !seen[next] && normalized[next] >= 0.5 {
                    seen[next] = true;
                    queue.push(next);
                }
            }
        }
        if area >= SIDE * SIDE / 100 {
            components += 1
        }
    }
    Ok(Segmentation {
        width: SIDE as u32,
        height: SIDE as u32,
        coverage: normalized
            .into_iter()
            .map(|v| (v * 65535.0).round() as u16)
            .collect(),
        confident_fraction,
        foreground_fraction,
        components,
        needs_confirmation: components != 1
            || confident_fraction < 0.70
            || !(0.01..=0.95).contains(&foreground_fraction),
        backend,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_preprocessing_and_subject_ambiguity_do_not_claim_semantics() {
        let input = preprocess(&[255, 0, 0, 255].repeat(4), 2, 2);
        assert_eq!(input.len(), 3 * SIDE * SIDE);
        assert!(input.iter().all(|v| v.is_finite()));
        let mut output = vec![0.0; SIDE * SIDE];
        for y in 20..80 {
            for x in 20..80 {
                output[y * SIDE + x] = 1.0
            }
        }
        let one = postprocess(&output, ExecutionBackend::Cpu).unwrap();
        assert_eq!(one.components, 1);
        assert!(!one.needs_confirmation);
        for y in 200..280 {
            for x in 200..280 {
                output[y * SIDE + x] = 1.0
            }
        }
        let two = postprocess(&output, ExecutionBackend::Cpu).unwrap();
        assert_eq!(two.components, 2);
        assert!(two.needs_confirmation);
        assert!(postprocess(&vec![0.5; SIDE * SIDE], ExecutionBackend::Cpu).is_err());
    }
}
