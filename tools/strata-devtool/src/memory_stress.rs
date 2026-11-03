//! Fixed-seed random, independent source blocks; real pixel edits, undo and
//! recovery all pass through the command executor. Large inputs are streamed.
use crate::dataset::Dataset;
use serde_json::{Value, json};
use std::{collections::HashSet, error::Error, path::Path, time::Instant};
use strata_core::{Document, Editor, Source};
use strata_pixels::{
    PixelBuffer, TileCoord,
    memory::{MemoryLimits, MemoryPool},
};

fn verify(doc: &Document, dataset: &Dataset, alpha_only: bool) -> Result<u64, Box<dyn Error>> {
    if doc.layers().len() != dataset.layer_ids.len() {
        return Err("wrong layer count".into());
    }
    let mut versions = HashSet::new();
    let mut samples = 0;
    for (layer, index) in doc.layers().iter().zip(0usize..) {
        for (coord, tile) in layer.pixels().tiles() {
            if !versions.insert(
                layer
                    .pixels()
                    .tile_version(*coord)
                    .ok_or("missing block version")?,
            ) {
                return Err("shared source block in independent dataset".into());
            }
            let tile = tile?;
            let [ox, oy] = coord.origin();
            for pixel in 0..256 * 256usize {
                let expected = dataset.pixel(
                    index,
                    ox as u32 + (pixel % 256) as u32,
                    oy as u32 + (pixel / 256) as u32,
                );
                let sample = pixel * 4;
                let actual = match &*tile {
                    PixelBuffer::Rgba8(v) => [
                        u16::from(v[sample]),
                        u16::from(v[sample + 1]),
                        u16::from(v[sample + 2]),
                        u16::from(v[sample + 3]),
                    ],
                    PixelBuffer::Rgba16(v) => {
                        [v[sample], v[sample + 1], v[sample + 2], v[sample + 3]]
                    }
                };
                if if alpha_only {
                    actual[3] != expected[3]
                } else {
                    actual != expected
                } {
                    return Err(format!(
                        "mismatch layer {index} tile ({},{}) pixel {pixel}",
                        coord.x(),
                        coord.y()
                    )
                    .into());
                }
                samples += if alpha_only { 1 } else { 4 };
            }
        }
    }
    let expected = u64::from(dataset.width.div_ceil(256))
        * u64::from(dataset.height.div_ceil(256))
        * dataset.layer_ids.len() as u64;
    if versions.len() as u64 != expected {
        return Err("wrong independent block count".into());
    }
    Ok(samples)
}
fn versions(doc: &Document) -> HashSet<u64> {
    doc.layers()
        .iter()
        .flat_map(|l| {
            l.pixels()
                .tile_coords()
                .filter_map(|c| l.pixels().tile_version(*c))
        })
        .collect()
}
fn probes(doc: &Document) -> Result<u64, Box<dyn Error>> {
    let coord = TileCoord::new(0, 0)?;
    let mut sum = 0u64;
    for layer in doc.layers() {
        let tile = layer.pixels().tile(coord)?.ok_or("missing probe")?;
        sum += match &*tile {
            PixelBuffer::Rgba8(v) => u64::from(v[17]),
            PixelBuffer::Rgba16(v) => u64::from(v[17]),
        };
    }
    Ok(sum)
}
fn event(events: &mut Vec<Value>, stage: &str, started: Instant, pool: &MemoryPool) {
    events.push(json!({"stage":stage,"elapsed_ms":started.elapsed().as_millis(),"process":strata_platform::process_metrics(),"managed":pool.stats()}));
    eprintln!("STRATA_MEMORY_STRESS {stage}");
}

pub fn run(directory: &Path, width: u32, height: u32, layers: usize) -> Result<(), Box<dyn Error>> {
    std::fs::create_dir(directory)?;
    let scratch = tempfile::tempdir_in(directory)?;
    let input = scratch.path().join("input.strata");
    let output = scratch.path().join("saved.strata");
    let dataset = Dataset::new(width, height, layers, 16)?;
    let limits = MemoryLimits {
        cpu_bytes: 256 * 1024 * 1024,
        gpu_bytes: 64 * 1024 * 1024,
        disk_bytes: 32 * 1024 * 1024 * 1024,
    };
    let pool = MemoryPool::new(limits);
    let start = Instant::now();
    let mut events = Vec::new();
    event(&mut events, "start", start, &pool);
    dataset.write(&input)?;
    event(&mut events, "streamed_random_fixture", start, &pool);
    let mut editor = Editor::with_memory_pool(pool.clone());
    let resource = editor.grant_read(&input, Source::Test)?;
    crate::run(&mut editor, "file.open", json!({"resource":resource}))?;
    event(&mut events, "open", start, &pool);
    let source_bytes: u64 = editor
        .document()
        .ok_or("no document")?
        .layers()
        .iter()
        .map(|l| l.pixels().referenced_bytes())
        .sum();
    pool.trim()?;
    let begin = Instant::now();
    let cold = probes(editor.document().ok_or("no document")?)?;
    let cold_ms = begin.elapsed().as_secs_f64() * 1000.0;
    let begin = Instant::now();
    let hot = probes(editor.document().ok_or("no document")?)?;
    let hot_ms = begin.elapsed().as_secs_f64() * 1000.0;
    if cold != hot {
        return Err("cold/hot probe mismatch".into());
    }
    let original = versions(editor.document().ok_or("no document")?);
    let source_samples = verify(editor.document().ok_or("no document")?, &dataset, false)?;
    event(&mut events, "all_source_samples_exact", start, &pool);
    crate::run(&mut editor, "color.convert", json!({"space":"display_p3"}))?;
    let converted = editor.document().ok_or("no document")?;
    let next = versions(converted);
    if original.len() != next.len() || !original.is_disjoint(&next) {
        return Err("pixel edit did not replace every independent source block".into());
    }
    let alpha_samples = verify(converted, &dataset, true)?;
    event(&mut events, "pixel_edit_alpha_exact", start, &pool);
    crate::run(&mut editor, "edit.undo", json!({}))?;
    verify(editor.document().ok_or("no document")?, &dataset, false)?;
    crate::run(&mut editor, "edit.redo", json!({}))?;
    if !editor
        .document()
        .ok_or("no document")?
        .profile_name()
        .contains("P3")
    {
        return Err("redo did not restore pixel conversion".into());
    }
    crate::run(&mut editor, "edit.undo", json!({}))?;
    let top = editor
        .document()
        .ok_or("no document")?
        .layers()
        .last()
        .ok_or("no layer")?
        .id();
    for x in 1..=8 {
        crate::run(
            &mut editor,
            "layer.set_position",
            json!({"layer":{"id":top},"x":x,"y":-x}),
        )?;
        crate::run(&mut editor, "edit.undo", json!({}))?;
    }
    crate::run(
        &mut editor,
        "layer.rename",
        json!({"layer":{"id":top},"name":"Verified random layer"}),
    )?;
    event(&mut events, "undo_redo_and_continuous_edits", start, &pool);
    let store = strata_io::recovery::RecoveryStore::new(&scratch.path().join("recovery"))?;
    let checkpoint = editor
        .checkpoint_recovery(&store, "Random pressure document")?
        .ok_or("missing checkpoint")?;
    let mut restored = Editor::with_memory_pool(pool.clone());
    let read = restored.grant_read(&store.path(&checkpoint.session_id)?, Source::Test)?;
    crate::run(&mut restored, "file.recover", json!({"resource":read}))?;
    if !restored.dirty() || restored.project_path().is_some() {
        return Err("recovery gained saved state/path".into());
    }
    let recovered_samples = verify(restored.document().ok_or("no recovery")?, &dataset, false)?;
    drop(restored);
    store.remove(&checkpoint.session_id)?;
    event(&mut events, "recovery_samples_exact", start, &pool);
    let write = editor.grant_write(&output, false, Source::Test)?;
    crate::run(&mut editor, "file.save_as", json!({"resource":write}))?;
    let project_bytes = std::fs::metadata(&output)?.len();
    drop(editor);
    event(&mut events, "saved_and_released", start, &pool);
    let mut reopened = Editor::with_memory_pool(pool.clone());
    let read = reopened.grant_read(&output, Source::Test)?;
    crate::run(&mut reopened, "file.open", json!({"resource":read}))?;
    let reopened_samples = verify(
        reopened.document().ok_or("no reopened document")?,
        &dataset,
        false,
    )?;
    drop(reopened);
    event(&mut events, "reopened_samples_exact", start, &pool);
    let usage = pool.stats();
    if source_samples != reopened_samples
        || source_samples != recovered_samples
        || usage.peak_cpu_bytes > limits.cpu_bytes
        || usage.peak_disk_bytes > limits.disk_bytes
        || usage.cpu_bytes != 0
        || usage.disk_bytes != 0
    {
        return Err("roundtrip, budget or lifecycle mismatch".into());
    }
    let process = strata_platform::process_metrics();
    if process
        .peak_rss_bytes
        .is_some_and(|bytes| bytes > 4 * 1024 * 1024 * 1024)
    {
        return Err("process RSS exceeds 4 GiB".into());
    }
    let report = json!({"passed":true,"algorithm":"splitmix64-coordinate-v1","seed":crate::dataset::SEED,"width":width,"height":height,"independent_layers":layers,"bit_depth":16,"source_bytes":source_bytes,"source_samples_verified":source_samples,"reopened_samples_verified":reopened_samples,"recovery_samples_verified":recovered_samples,"converted_alpha_samples_verified":alpha_samples,"edited_independent_blocks":next.len(),"project_bytes":project_bytes,"cold_hot_identical_probes":{"cold_ms":cold_ms,"hot_ms":hot_ms,"layers":layers,"source_cache_trimmed_before_cold":true},"limits":limits,"usage":usage,"process":process,"events":events,"total_ms":start.elapsed().as_millis(),"pixel_edit_undo_redo_exact":true,"temporary_projects_removed":true});
    drop(scratch);
    std::fs::write(
        directory.join("report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}
