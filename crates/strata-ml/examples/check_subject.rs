use std::{path::Path, time::Instant};
use strata_ml::{ExecutionBackend, SubjectModel, assets};
use strata_pixels::{PixelBuffer, memory::MemoryPool};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 6 {
        return Err("usage: check_subject RUNTIME MODEL IMAGE cpu|core_ml NEW_REPORT".into());
    }
    let backend = match args[4].as_str() {
        "cpu" => ExecutionBackend::Cpu,
        "core_ml" => ExecutionBackend::CoreMl,
        _ => return Err("unknown backend".into()),
    };
    let image = strata_io::decode_image(Path::new(&args[3]))?;
    let srgb = strata_color::working_profile("srgb")?;
    let pixels = strata_color::convert_to_profile(
        &image.pixels,
        image.width,
        image.height,
        image.icc_profile.as_deref().unwrap_or(&srgb),
        &srgb,
    )?;
    let rgba = match pixels {
        PixelBuffer::Rgba8(v) => v,
        PixelBuffer::Rgba16(v) => v
            .into_iter()
            .map(|v| ((u32::from(v) + 128) / 257) as u8)
            .collect(),
    };
    let pool = MemoryPool::global();
    let started = Instant::now();
    let runtime_hash = std::env::var("STRATA_VERIFIED_RUNTIME_SHA256")
        .map_err(|_|"Set STRATA_VERIFIED_RUNTIME_SHA256 from the verified packaged runtime; unpatched Microsoft 1.22.0 is not supported.")?;
    let model = SubjectModel::load(
        Path::new(&args[1]),
        &runtime_hash,
        Path::new(&args[2]),
        backend,
        pool.clone(),
    )?;
    let load_ms = started.elapsed().as_millis();
    let started = Instant::now();
    let result = model.infer(&rgba, image.width, image.height, &|| false)?;
    let inference_ms = started.elapsed().as_millis();
    let mut report = serde_json::to_value(&result)?;
    report["load_ms"] = serde_json::json!(load_ms);
    report["inference_ms"] = serde_json::json!(inference_ms);
    report["memory"] = serde_json::json!(pool.stats());
    report["model_sha256"] = serde_json::json!(assets::SUBJECT_MODEL.sha256);
    let path = Path::new(&args[5]);
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    serde_json::to_writer_pretty(file, &report)?;
    let mask = path.with_extension("png");
    let rgba: Vec<u8> = result
        .coverage
        .iter()
        .flat_map(|v| {
            let v = (u32::from(*v) / 257) as u8;
            [v, v, v, 255]
        })
        .collect();
    image::save_buffer_with_format(
        mask,
        &rgba,
        result.width,
        result.height,
        image::ColorType::Rgba8,
        image::ImageFormat::Png,
    )?;
    println!("subject inference completed; report {}", path.display());
    Ok(())
}
