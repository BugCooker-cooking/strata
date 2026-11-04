use serde_json::json;
use std::{path::Path, time::Instant};
use strata_core::{Editor, Source};
use strata_render::{Renderer, Viewport};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = std::env::args()
        .nth(1)
        .ok_or("usage: exposure_preview_benchmark NEW_REPORT")?;
    let mut editor = Editor::new();
    for (id, params) in [
        (
            "file.new",
            json!({"width":2048,"height":1365,"bit_depth":16,"background":[16384,32768,8192,65535]}),
        ),
        (
            "select.marquee",
            json!({"x":512,"y":256,"width":1024,"height":1024}),
        ),
        (
            "layer.add_adjustment",
            json!({"name":"exposure","type":"exposure","stops":0.0,"mask":"from_selection"}),
        ),
    ] {
        editor.execute(editor.request(Source::Test), id, params, |_| Ok(()))?;
    }
    let layer = editor.document().unwrap().layers().last().unwrap().id();
    let gesture = editor.begin_exposure(strata_core::LayerRef::Id(layer))?;
    let mut renderer = Renderer::new_offscreen(Viewport {
        width: 1280,
        height: 800,
        scale: 1.0,
    })?;
    let initial = renderer.prepare_document(editor.document().unwrap(), true)?;
    renderer.commit_document(initial)?;
    renderer.render()?;
    let mut frames = Vec::new();
    for i in 0..12 {
        let started = Instant::now();
        let candidate = editor.preview_exposure(&gesture, -0.5 + f64::from(i) / 11.0)?;
        let scene = renderer.prepare_document(candidate.document().unwrap(), false)?;
        let prepare_ms = started.elapsed().as_secs_f64() * 1000.0;
        renderer.commit_document(scene)?;
        let frame = renderer.render()?;
        frames.push(json!({"step":i,"prepare_ms":prepare_ms,"submission_cpu_ms":frame.cpu_ms,"total_ms":started.elapsed().as_secs_f64()*1000.0}));
    }
    let mut times: Vec<_> = frames
        .iter()
        .map(|f| f["total_ms"].as_f64().unwrap())
        .collect();
    times.sort_by(f64::total_cmp);
    let p95 = times[(times.len() * 95 / 100).min(times.len() - 1)];
    let report = json!({"scene":{"width":2048,"height":1365,"bit_depth":16,"viewport":[1280,800],"masked_exposure":true},"p95_ms":p95,"target_ms":1000.0/30.0,"cpu_preparation_target_met":p95<=1000.0/30.0,"frames":frames,
        "limitation":"Offscreen preparation/submission reference. Native frame pacing is a separate gate.","source_history_unchanged":editor.snapshot().history.len()==2});
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(Path::new(&output))?;
    serde_json::to_writer_pretty(file, &report)?;
    println!("exposure preview p95 {p95:.2} ms");
    Ok(())
}
