use serde_json::{Value, json};
use std::{error::Error, fs::OpenOptions, io::Write, path::Path, time::Instant};
use strata_core::Editor;
use strata_pixels::memory::{MemoryLimits, MemoryPool};

/// Fixed 24 MP / 16-bit generated-color fixture. This measures command latency
/// and shared budget accounting, never display frame rate or semantic quality.
pub fn run(path: &Path) -> Result<(), Box<dyn Error>> {
    if cfg!(debug_assertions) {
        return Err("selection benchmark requires --release".into());
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
        "layer.fill.solid",
        json!({"name":"Blue backdrop","color":[0,0,65535,65535]}),
    )?;
    super::run(
        &mut e,
        "select.marquee",
        json!({"x":2000,"y":1000,"width":1500,"height":2000}),
    )?;
    super::run(
        &mut e,
        "layer.fill.solid",
        json!({"name":"Red target","color":[65535,0,0,65535]}),
    )?;
    super::run(
        &mut e,
        "layer.mask.add",
        json!({"layer":{"name":"Red target"},"source":"from_selection"}),
    )?;
    super::run(&mut e, "select.none", json!({}))?;
    let layer_state = |e: &Editor| {
        serde_json::to_value(
            e.document()
                .unwrap()
                .layers()
                .iter()
                .map(|l| l.info())
                .collect::<Vec<_>>(),
        )
    };
    let baseline = layer_state(&e)?;
    let cases = [
        (
            "select.ellipse",
            json!({"x":2000,"y":1000,"width":1500,"height":2000}),
        ),
        ("select.feather", json!({"radius":2})),
        ("select.expand", json!({"radius":4})),
        ("select.contract", json!({"radius":2})),
        ("select.smooth", json!({"radius":2})),
        ("select.border", json!({"radius":3})),
        ("select.transform", json!({"matrix":[1,0,0,1,20,10]})),
        (
            "select.wand",
            json!({"x":200,"y":200,"tolerance":4,"contiguous":true}),
        ),
        (
            "select.quick",
            json!({"foreground":[[2600,2000]],"background":[[200,200]],"radius":8}),
        ),
    ];
    let mut results: Vec<Value> = Vec::new();
    for (command, params) in cases {
        let start = Instant::now();
        super::run(&mut e, command, params)?;
        let ms = start.elapsed().as_secs_f64() * 1000.0;
        if layer_state(&e)? != baseline {
            return Err("selection command changed source layer metadata".into());
        }
        let selection = e
            .document()
            .unwrap()
            .selection()
            .ok_or("selection missing")?;
        let foreground = selection.sample(2600, 2000)?;
        let background = selection.sample(200, 200)?;
        if command == "select.wand" && (foreground != 0 || background != 65535)
            || command == "select.quick" && (foreground != 65535 || background != 0)
        {
            return Err(format!("fixed color oracle failed: {command}").into());
        }
        results.push(json!({"command":command,"single_run_ms":ms,"target_coverage":foreground,
            "protected_coverage":background,"mask_tiles":selection.tile_count(),"budget":pool.stats()}));
    }
    let report = json!({"fixture":"24 MP generated red rectangle on blue; 6000x4000; RGBA16",
        "profile":"release","method":"one sequential run per operation; not frame rate or segmentation photo-quality evidence",
        "source_layer_metadata_unchanged":true,"limits":pool.limits(),"cases":results});
    let mut output = OpenOptions::new().create_new(true).write(true).open(path)?;
    output.write_all(serde_json::to_string_pretty(&report)?.as_bytes())?;
    output.write_all(b"\n")?;
    output.sync_all()?;
    println!(
        "{}",
        json!({"report":path,"cases":results.len(),"peak_reserved_cpu_bytes":pool.stats().peak_cpu_bytes})
    );
    Ok(())
}
