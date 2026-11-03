use serde::Deserialize;
use serde_json::{Value, json};
mod validation;
use std::{collections::HashMap, error::Error, path::Path};
use strata_core::{Editor, Source, commands::catalog};

fn run(editor: &mut Editor, id: &str, args: Value) -> Result<Value, Box<dyn Error>> {
    let request = editor.request(Source::Test);
    Ok(editor.execute(request, id, args, |_| Ok(()))?)
}
fn fresh() -> Result<Editor, Box<dyn Error>> {
    let mut e = Editor::new();
    run(
        &mut e,
        "file.new",
        json!({"width":16,"height":16,"bit_depth":16}),
    )?;
    Ok(e)
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
                Editor::new()
            } else {
                fresh()?
            };
            let mut args = example.clone();
            match spec.id {
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
    let mut editor = Editor::new();
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
        Some("run") => script(Path::new(args.get(2).ok_or("missing script path")?))?,
        Some("demo") => println!("{}", serde_json::to_string_pretty(&validation::demo(Path::new(args.get(2).ok_or("missing new output directory")?))?)?),
        Some("capacity") => println!("{}", serde_json::to_string_pretty(&validation::capacity()?)?),
        Some("verify-exports") => println!("{}", serde_json::to_string_pretty(&validation::verify_exports(Path::new(args.get(2).ok_or("missing project")?), &args[3..])?)?),
        _ => return Err("usage: strata-devtool catalog | examples | run SCRIPT.json | demo NEW_DIRECTORY | capacity | verify-exports PROJECT IMAGE...".into()),
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
