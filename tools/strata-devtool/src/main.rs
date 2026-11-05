use serde::Deserialize;
use serde_json::{Value, json};
mod dataset;
mod documentation;
mod memory_stress;
mod selection_benchmark;
mod tonal_benchmark;
mod validation;
use std::{collections::HashMap, error::Error, path::Path};
use strata_core::{Editor, Source, commands::catalog};
fn native_editor() -> Editor {
    Editor::with_decoder(std::sync::Arc::new(strata_platform::NativeDecoder))
}

fn run(editor: &mut Editor, id: &str, args: Value) -> Result<Value, Box<dyn Error>> {
    let request = editor.request(Source::Test);
    Ok(editor.execute(request, id, args, |_| Ok(()))?)
}
fn fresh() -> Result<Editor, Box<dyn Error>> {
    let mut e = native_editor();
    // Deterministic command fixture; never substitute this for ML acceptance.
    e.set_subject_selector(std::sync::Arc::new(ExampleSubject));
    run(
        &mut e,
        "file.new",
        json!({"width":16,"height":16,"bit_depth":16}),
    )?;
    Ok(e)
}
struct ExampleSubject;
impl strata_core::SubjectSelector for ExampleSubject {
    fn select(
        &self,
        doc: &strata_core::Document,
        _: &strata_core::SubjectParams,
        _: &(dyn Fn() -> bool + Sync),
    ) -> Result<strata_core::SubjectSelection, String> {
        let frame = doc.frame();
        let mask = strata_pixels::TiledMask::new(
            strata_pixels::MaskLayout {
                bounds: strata_pixels::CoverageRect {
                    x: frame.x,
                    y: frame.y,
                    width: frame.width,
                    height: frame.height,
                },
                default_coverage: 65535,
                inverted: false,
            },
            1,
            doc.memory_pool(),
        )
        .map_err(|e| e.to_string())?;
        Ok(strata_core::SubjectSelection {
            mask,
            diagnostics: json!({"fixture":"deterministic command example; not ML quality evidence"}),
            needs_confirmation: false,
        })
    }
}
fn examples() -> Result<(), Box<dyn Error>> {
    let mut ids = std::collections::HashSet::new();
    for spec in catalog() {
        if !ids.insert(spec.id)
            || spec.description.is_empty()
            || spec.title_zh.is_empty()
            || spec.preconditions.is_empty()
            || spec.examples.is_empty()
        {
            return Err(format!("incomplete command {}", spec.id).into());
        }
        for example in &spec.examples {
            let dir = tempfile::tempdir()?;
            let mut editor = if spec.id == "file.new" {
                native_editor()
            } else {
                fresh()?
            };
            let mut args = example.clone();
            match spec.id {
                "adjust.tonal.set" => {
                    run(
                        &mut editor,
                        "layer.add_tonal_adjustment",
                        json!({"name":args["layer"]["name"],"adjustment":{"type":args["adjustment"]["type"]}}),
                    )?;
                }
                "select.reselect" => {
                    run(
                        &mut editor,
                        "select.marquee",
                        json!({"x":2,"y":2,"width":8,"height":8}),
                    )?;
                    run(&mut editor, "select.none", json!({}))?;
                }
                "select.feather" | "select.expand" | "select.contract" | "select.smooth"
                | "select.border" | "select.transform" => {
                    run(
                        &mut editor,
                        "select.marquee",
                        json!({"x":2,"y":2,"width":8,"height":8}),
                    )?;
                }
                "layer.set_pass_through" => {
                    run(&mut editor, "layer.create_group", json!({"name":"Group"}))?;
                }
                "edit.cut" | "edit.clipboard.write_status" => {
                    let copied = run(&mut editor, "edit.copy", json!({}))?;
                    let token = copied["clipboard_resource"].as_str().unwrap();
                    editor.confirm_clipboard_write(token, &Source::Test)?;
                    args["resource"] = json!(token);
                }
                "edit.paste" | "edit.paste_in_place" => {
                    let icc = strata_color::working_profile("srgb")?;
                    let image = strata_core::ClipboardImage::new(
                        strata_pixels::PixelBuffer::Rgba16(
                            [32768, 32769, 50001, 65535].repeat(16 * 16),
                        ),
                        16,
                        16,
                        -1,
                        2,
                        icc,
                        &strata_pixels::memory::MemoryPool::global(),
                    )?;
                    args["resource"] = json!(editor.grant_clipboard_image(image, Source::Test)?);
                }
                "layer.distribute" => {
                    run(&mut editor, "layer.create", json!({"name":"Second"}))?;
                    run(&mut editor, "layer.create", json!({"name":"Third"}))?;
                }
                "layer.rasterize" => {
                    run(
                        &mut editor,
                        "layer.fill.solid",
                        json!({"name":"Solid","color":[32768,10000,20000,65535]}),
                    )?;
                }
                "layer.mask.apply" => {
                    run(
                        &mut editor,
                        "layer.mask.add",
                        json!({"layer":{"name":"Background"},"source":"white"}),
                    )?;
                }
                "layer.merge_down" => {
                    run(
                        &mut editor,
                        "layer.create",
                        json!({"name":"Upper","fill":[32768,10000,20000,65535]}),
                    )?;
                }
                "layer.set_clipping" => {
                    run(&mut editor, "layer.create", json!({"name":"Clip"}))?;
                }
                "layer.fill.solid.set" => {
                    run(
                        &mut editor,
                        "layer.fill.solid",
                        json!({"name":"Solid","color":[65535,65535,65535,65535]}),
                    )?;
                }
                "layer.fill.gradient.set" => {
                    run(
                        &mut editor,
                        "layer.fill.gradient",
                        json!({"name":"Gradient","start":[0,0,0,65535],"end":[65535,65535,65535,65535]}),
                    )?;
                }
                "adjust.hue_saturation.set" => {
                    run(
                        &mut editor,
                        "layer.add_hue_saturation",
                        json!({"name":"Hue"}),
                    )?;
                }
                "layer.ungroup" => {
                    run(
                        &mut editor,
                        "layer.group",
                        json!({"layers":[{"name":"Background"}],"name":"Group"}),
                    )?;
                }
                "layer.mask.set_enabled" | "layer.mask.invert" | "layer.mask.remove" => {
                    run(
                        &mut editor,
                        "layer.mask.add",
                        json!({"layer":{"name":"Background"},"source":"white"}),
                    )?;
                }
                "layer.add_adjustment" => {
                    run(
                        &mut editor,
                        "select.marquee",
                        json!({"x":2,"y":2,"width":8,"height":8}),
                    )?;
                }
                "adjust.exposure.set" => {
                    run(
                        &mut editor,
                        "layer.add_adjustment",
                        json!({"name":"人物提亮","type":"exposure","mask":"white"}),
                    )?;
                }
                "edit.undo" | "edit.redo" => {
                    run(&mut editor, "layer.create", json!({"name":"Undo fixture"}))?;
                    if spec.id == "edit.redo" {
                        run(&mut editor, "edit.undo", json!({}))?;
                    }
                }
                "edit.undo_turn" => {
                    let turn = editor.begin_turn()?;
                    let request = editor.request(Source::Ai { turn_id: turn });
                    editor.execute(
                        request,
                        "layer.create",
                        json!({"name":"AI fixture"}),
                        |_| Ok(()),
                    )?;
                    editor.stop_turn();
                    args = json!({"turn_id":turn});
                }
                "file.place" => {
                    let path = dir.path().join("input.png");
                    let grant = editor.grant_write(&path, false, Source::Test)?;
                    run(
                        &mut editor,
                        "file.export",
                        json!({"resource":grant,"format":"png"}),
                    )?;
                    args["resource"] = json!(editor.grant_read(&path, Source::Test)?);
                }
                "file.open" => {
                    let path = dir.path().join("input.strata");
                    let grant = editor.grant_write(&path, false, Source::Test)?;
                    run(&mut editor, "file.save_as", json!({"resource":grant}))?;
                    let grant = editor.grant_read(&path, Source::Test)?;
                    args["resource"] = json!(grant);
                }
                "file.recover" => {
                    let store = strata_io::recovery::RecoveryStore::new(dir.path())?;
                    let info = editor
                        .checkpoint_recovery(&store, "Recovery fixture")?
                        .ok_or("missing checkpoint")?;
                    args["resource"] =
                        json!(editor.grant_read(&store.path(&info.session_id)?, Source::Test)?);
                }
                "file.save" | "file.save_as" | "file.export" => {
                    let grant = editor.grant_write(
                        &dir.path().join(if spec.id == "file.export" {
                            "export.png"
                        } else {
                            "output.strata"
                        }),
                        false,
                        Source::Test,
                    )?;
                    args["resource"] = json!(grant);
                }
                _ => {}
            }
            let result = run(&mut editor, spec.id, args)?;
            if !result.is_object() {
                return Err(format!("{} returned non-object", spec.id).into());
            }
        }
    }
    println!(
        "PASS: {} documented commands and isolated examples",
        ids.len()
    );
    Ok(())
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Script {
    #[serde(default)]
    resources: Vec<Resource>,
    commands: Vec<Step>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Resource {
    name: String,
    path: String,
    #[serde(default)]
    write: bool,
    #[serde(default)]
    overwrite: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Step {
    command: String,
    params: Value,
}
fn script(path: &Path) -> Result<(), Box<dyn Error>> {
    let metadata = std::fs::metadata(path)?;
    if metadata.len() > 1024 * 1024 {
        return Err("script exceeds 1 MiB".into());
    }
    let script: Script = serde_json::from_slice(&std::fs::read(path)?)?;
    if script.commands.len() > 4096 {
        return Err("too many commands".into());
    }
    let mut editor = native_editor();
    let mut resources = HashMap::new();
    for r in script.resources {
        let grant = if r.write {
            editor.grant_write(Path::new(&r.path), r.overwrite, Source::Test)?
        } else {
            editor.grant_read(Path::new(&r.path), Source::Test)?
        };
        if resources.insert(r.name, grant).is_some() {
            return Err("duplicate resource name".into());
        }
    }
    for mut step in script.commands {
        if let Some(alias) = step.params.get("resource").and_then(Value::as_str) {
            let grant = resources.get(alias).ok_or("undeclared resource")?;
            step.params["resource"] = json!(grant);
        }
        println!(
            "{}",
            json!({"command":step.command,"result":run(&mut editor,&step.command,step.params)?})
        );
    }
    println!("{}", json!({"snapshot":editor.snapshot()}));
    Ok(())
}
fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("catalog") => println!("{}", serde_json::to_string_pretty(catalog())?),
        Some("examples") => examples()?,
        Some("docs") => documentation::write(Path::new(args.get(2).ok_or("missing Markdown output path")?))?,
        Some("run") => script(Path::new(args.get(2).ok_or("missing script path")?))?,
        Some("demo") => println!("{}", serde_json::to_string_pretty(&validation::demo(Path::new(args.get(2).ok_or("missing new output directory")?))?)?),
        Some("capacity") => println!("{}", serde_json::to_string_pretty(&validation::capacity()?)?),
        Some("selection-benchmark") => selection_benchmark::run(Path::new(args.get(2).ok_or("missing new report path")?))?,
        Some("tonal-benchmark") => tonal_benchmark::run(Path::new(args.get(2).ok_or("missing new report path")?),args.get(3).is_some_and(|mode|mode=="local"))?,
        Some("memory-stress") => {
            let width = args.get(3).map(|s| s.parse()).transpose()?.unwrap_or(8000);
            let height = args.get(4).map(|s| s.parse()).transpose()?.unwrap_or(6250);
            let layers = args.get(5).map(|s| s.parse()).transpose()?.unwrap_or(30);
            memory_stress::run(Path::new(args.get(2).ok_or("missing new output directory")?), width, height, layers)?;
        }
        Some("fixture-project") => {
            let path=Path::new(args.get(2).ok_or("missing output project")?);
            let width=args.get(3).ok_or("missing width")?.parse()?;let height=args.get(4).ok_or("missing height")?.parse()?;
            let layers=args.get(5).ok_or("missing layers")?.parse()?;let depth=args.get(6).ok_or("missing depth")?.parse()?;
            dataset::Dataset::new(width,height,layers,depth)?.write(path)?;
            println!("{}",json!({"path":path,"width":width,"height":height,"layers":layers,"bit_depth":depth,"seed":dataset::SEED}));
        }
        Some("verify-exports") => println!("{}", serde_json::to_string_pretty(&validation::verify_exports(Path::new(args.get(2).ok_or("missing project")?), &args[3..])?)?),
        _ => return Err("usage: strata-devtool catalog | examples | docs NEW_MARKDOWN | run SCRIPT.json | demo NEW_DIRECTORY | capacity | selection-benchmark NEW_REPORT | tonal-benchmark NEW_REPORT | fixture-project NEW_PROJECT WIDTH HEIGHT LAYERS DEPTH | memory-stress NEW_DIRECTORY [WIDTH HEIGHT LAYERS] | verify-exports PROJECT IMAGE...".into()),
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    #[test]
    fn every_catalog_example_runs() {
        super::examples().unwrap();
    }
}
