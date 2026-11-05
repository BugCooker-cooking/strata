//! A fixed, CPU-only trend report. No native windows or model/network calls.
use serde_json::json;
use std::{error::Error, io::Write, path::Path, time::Instant};
use strata_core::{Editor, Source};
use strata_pixels::memory::{MemoryLimits, MemoryPool};
pub fn run(path: &Path) -> Result<(), Box<dyn Error>> {
    if path.exists() {
        return Err("report already exists; choose a new output path".into());
    }
    let mut results = Vec::new();
    for (command, settings) in [
        ("filter.gaussian_blur", json!({"sigma":1.5})),
        (
            "filter.unsharp_mask",
            json!({"sigma":1.0,"amount":1.2,"threshold":0.01}),
        ),
        (
            "filter.add_noise",
            json!({"amount":0.02,"distribution":"gaussian","monochrome":true,"seed":42}),
        ),
    ] {
        let pool = MemoryPool::new(MemoryLimits {
            cpu_bytes: 512 * 1024 * 1024,
            gpu_bytes: 0,
            disk_bytes: 2 * 1024 * 1024 * 1024,
        });
        let mut editor = Editor::with_memory_pool(pool.clone());
        editor.execute(
            editor.request(Source::Test),
            "file.new",
            json!({"width":6000,"height":4000,"bit_depth":16,"working_space":"display_p3"}),
            |_| Ok(()),
        )?;
        editor.execute(editor.request(Source::Test),"paint.gradient",json!({"layer":{"name":"Background"},"start":[0.5,0.5],"end":[5999.5,3999.5],"stops":[{"at":0,"color":[0.15,0.2,0.25,0.7]},{"at":1,"color":[0.8,0.7,0.6,1]}]}),|_|Ok(()))?;
        editor.execute(editor.request(Source::Test),"paint.stroke",json!({"layer":{"name":"Background"},"points":[{"x":3000.5,"y":2000.5}],"size":1200,"hardness":1,"color":[0.7,0.3,0.1,1]}),|_|Ok(()))?;
        let initial_state = editor.document().unwrap().state_id();
        let history = editor.snapshot().history_cursor;
        let mut params = settings;
        params["layer"] = json!({"id":editor.snapshot().active_layer});
        let start = Instant::now();
        let result = editor.execute(
            editor.request(Source::Test),
            command,
            params.clone(),
            |_| Ok(()),
        )?;
        let elapsed = start.elapsed().as_secs_f64();
        let committed = editor.snapshot().history_cursor == history + 1;
        let stats = pool.stats();
        let start = Instant::now();
        editor.execute(editor.request(Source::Test), "edit.undo", json!({}), |_| {
            Ok(())
        })?;
        let undo_seconds = start.elapsed().as_secs_f64();
        let undo_exact = editor.document().unwrap().state_id() == initial_state;
        if !committed || !undo_exact || result["touched_tiles"].as_u64().unwrap_or(0) == 0 {
            return Err(format!("{command}: invalid history/undo benchmark boundary").into());
        }
        results.push(json!({"command":command,"params":params,"command_seconds":elapsed,"undo_seconds":undo_seconds,"one_history_item":committed,"undo_restores_original_state":undo_exact,"result":result,"memory_pool_after_command":stats}));
    }
    let report = json!({"fixture":"fixed encoded Display P3 diagonal gradient with alpha .7..1 and a central hard round mark; sparse new document, one raw pixel layer","width":6000,"height":4000,"pixels":24000000,"bit_depth":16,"working_space":"display_p3","profile":if cfg!(debug_assertions){"debug"}else{"release"},"cpu_budget_bytes":512*1024*1024u64,"gpu_budget_bytes":0,"runs_per_case":1,"scope":"single-run CPU command and tracked-pool memory trend; not process RSS, photo quality, native interactive latency, or displayed frame rate","os":std::env::consts::OS,"arch":std::env::consts::ARCH,"results":results});
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut output = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    output.write_all(serde_json::to_string_pretty(&report)?.as_bytes())?;
    output.write_all(b"\n")?;
    println!(
        "{}",
        json!({"report":path,"cases":3,"pixels":24000000,"profile":report["profile"]})
    );
    Ok(())
}
