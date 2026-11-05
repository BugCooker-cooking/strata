use serde_json::json;
use std::{error::Error, fs::OpenOptions, io::Write, path::Path, time::Instant};
use strata_core::Editor;
use strata_pixels::{
    PixelBuffer,
    memory::{MemoryKind, MemoryLimits, MemoryPool},
};

pub fn run(path: &Path, local: bool) -> Result<(), Box<dyn Error>> {
    if cfg!(debug_assertions) {
        return Err("tonal benchmark requires --release".into());
    }
    if path.exists() {
        return Err("report already exists; choose a new path".into());
    }
    let pool = MemoryPool::new(MemoryLimits::default());
    let mut e = Editor::with_memory_pool(pool.clone());
    super::run(
        &mut e,
        "file.new",
        json!({"width":6000,"height":4000,"bit_depth":16}),
    )?;
    super::run(
        &mut e,
        "layer.fill.gradient",
        json!({"name":"Gray ramp","start":[8000,8000,8000,65535],"end":[50000,50000,50000,65535]}),
    )?;
    let source = e
        .document()
        .unwrap()
        .layers()
        .iter()
        .map(|l| l.info().clone())
        .collect::<Vec<_>>();
    let mut cases = Vec::new();
    let sequence = if local {
        vec![
            None,
            Some(json!({"type":"shadows_highlights","shadows":35,"highlights":25,"radius":30})),
        ]
    } else {
        vec![
            None,
            Some(json!({"type":"brightness_contrast","brightness":5,"contrast":10})),
            Some(json!({"type":"levels","master":{"gamma":1.2}})),
            Some(json!({"type":"curves","master":[[0,0],[0.5,0.65],[1,1]]})),
        ]
    };
    for settings in sequence {
        let name = settings
            .as_ref()
            .and_then(|v| v["type"].as_str())
            .unwrap_or("baseline")
            .to_owned();
        if let Some(settings) = settings {
            super::run(
                &mut e,
                "layer.add_tonal_adjustment",
                json!({"name":name,"adjustment":settings}),
            )?;
        }
        let _output = pool.reserve(MemoryKind::Transient, 6000 * 4000 * 8)?;
        let start = Instant::now();
        let PixelBuffer::Rgba16(pixels) = e.document().unwrap().export_pixels()? else {
            return Err("wrong export depth".into());
        };
        let elapsed = start.elapsed().as_secs_f64() * 1000.0;
        if pixels.chunks_exact(4).any(|p| p[3] != 65535) {
            return Err("tonal stack changed opaque alpha".into());
        }
        let remaining = e
            .document()
            .unwrap()
            .layers()
            .iter()
            .filter(|l| !l.info().kind.is_adjustment())
            .map(|l| l.info().clone())
            .collect::<Vec<_>>();
        if remaining != source {
            return Err("tonal stack changed source layers".into());
        }
        let i = (2000 * 6000 + 3000) * 4;
        cases.push(json!({"added_type":name,"stack_depth":e.document().unwrap().layers().len()-source.len(),
            "full_cpu_export_ms":elapsed,"center_rgba16":&pixels[i..i+4],"all_alpha_unchanged":true,"budget":pool.stats()}));
    }
    let report = json!({"fixture":"24 MP RGBA16 generated gray gradient; 6000x4000; sRGB", "date":"2026-11-05",
        "profile":"release","method":"single sequential complete CPU export after each new adjustment; not display frame rate or interactive preview latency",
        "source_layers_unchanged":true,"limits":pool.limits(),"cases":cases});
    let mut output = OpenOptions::new().write(true).create_new(true).open(path)?;
    output.write_all(serde_json::to_string_pretty(&report)?.as_bytes())?;
    output.write_all(b"\n")?;
    output.sync_all()?;
    println!(
        "{}",
        json!({"report":path,"cases":cases.len(),"peak_reserved_cpu_bytes":pool.stats().peak_cpu_bytes})
    );
    Ok(())
}
