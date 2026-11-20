use serde::Deserialize;
use serde_json::{Value, json};
mod dataset;
mod documentation;
mod filter_benchmark;
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
                "image.duplicate" => {
                    run(
                        &mut editor,
                        "layer.create",
                        json!({"name":"Color fixture","fill":[10001,30003,50001,32769]}),
                    )?;
                }
                "view.rulers.convert" => {
                    run(
                        &mut editor,
                        "file.new",
                        json!({"width":3000,"height":1500,"bit_depth":16,"resolution_dpi":300,"discard_changes":true}),
                    )?;
                    run(
                        &mut editor,
                        "view.presentation.set",
                        json!({"ruler_unit":"millimeters","ruler_origin":[100,200]}),
                    )?;
                }
                "path.from_selection" => {
                    run(
                        &mut editor,
                        "select.marquee",
                        json!({"x":1,"y":1,"width":12,"height":12}),
                    )?;
                    run(
                        &mut editor,
                        "select.marquee",
                        json!({"x":4,"y":4,"width":4,"height":4,"mode":"subtract"}),
                    )?;
                }
                "path.split" | "layer.fill.path" | "path.get" | "path.list" | "path.update"
                | "path.delete" | "select.from_path" => {
                    run(
                        &mut editor,
                        "path.create",
                        json!({"name":"Outline","anchors":[{"position":[2,2]},{"position":[13,2],"outgoing":[0,3]},{"position":[2,13]}]}),
                    )?;
                }
                "view.guides.update" | "view.guides.delete" | "view.guides.clear" => {
                    run(
                        &mut editor,
                        "view.guides.add",
                        json!({"name":"Baseline","axis":"horizontal","position":6.5}),
                    )?;
                }
                "layer.snap_position" if args["layer"]["name"] == "SmartMoving" => {
                    run(
                        &mut editor,
                        "layer.create",
                        json!({"name":"SmartMoving","fill":[10001,30003,50001,40001]}),
                    )?;
                    run(
                        &mut editor,
                        "layer.create",
                        json!({"name":"SmartReference","fill":[50001,20003,10005,65535]}),
                    )?;
                    run(
                        &mut editor,
                        "layer.set_position",
                        json!({"layer":{"name":"SmartReference"},"x":32,"y":0}),
                    )?;
                }
                "layer.snap_position" => {
                    run(
                        &mut editor,
                        "paint.fill",
                        json!({"layer":{"name":"Background"},"color":[0.2,0.4,0.8,1]}),
                    )?;
                    run(
                        &mut editor,
                        "view.guides.add",
                        json!({"name":"X","axis":"vertical","position":5}),
                    )?;
                    run(
                        &mut editor,
                        "view.guides.add",
                        json!({"name":"Y","axis":"horizontal","position":5}),
                    )?;
                }
                "transform.apply" if args["layer"]["name"] == "GroupGeometry" => {
                    run(
                        &mut editor,
                        "layer.create_group",
                        json!({"name":"GroupGeometry"}),
                    )?;
                    run(
                        &mut editor,
                        "layer.create",
                        json!({"name":"Pixels","fill":[10001,30003,50001,40001]}),
                    )?;
                    run(
                        &mut editor,
                        "type.create",
                        json!({"name":"Group type","spec":{"text":"Group","font":"sans","font_size":12,"layout":{"type":"point"}},"color":[0.2,0.4,0.8,1],"x":1,"y":14}),
                    )?;
                }
                "transform.apply" if args["layer"]["name"] == "TypeGeometry" => {
                    run(
                        &mut editor,
                        "type.create",
                        json!({"name":"TypeGeometry","spec":{"text":"Type","font":"sans","font_size":16,"layout":{"type":"point"}},"color":[0.2,0.4,0.8,1],"x":1,"y":18}),
                    )?;
                }
                "transform.apply_layers" | "image.resize" | "image.rotate" | "image.straighten" => {
                    run(
                        &mut editor,
                        "paint.fill",
                        json!({"layer":{"name":"Background"},"color":[0.4,0.5,0.6,1]}),
                    )?;
                    run(
                        &mut editor,
                        "layer.fill.gradient",
                        json!({"name":"Transform fill","start":[10001,30003,50001,65535],"end":[50001,30003,10001,32768],"angle":29}),
                    )?;
                    run(
                        &mut editor,
                        "layer.add_adjustment",
                        json!({"name":"Exposure","type":"exposure","stops":0.5,"mask":"white"}),
                    )?;
                }
                "transform.apply" if args["layer"]["name"] == "Transform fill" => {
                    run(
                        &mut editor,
                        "layer.fill.solid",
                        json!({"name":"Transform fill","color":[10001,30003,50001,65535]}),
                    )?;
                }
                "type.update" | "type.inspect" => {
                    run(
                        &mut editor,
                        "type.create",
                        json!({"name":"Title","spec":{"text":"Original 中文","font":"sans","font_size":24,"layout":{"type":"point"}},"color":[0.2,0.4,0.8,1],"x":1,"y":26}),
                    )?;
                }
                "image.auto" | "filter.high_pass" => {
                    run(
                        &mut editor,
                        "paint.gradient",
                        json!({"layer":{"name":"Background"},"start":[0.5,0.5],"end":[15.5,15.5],"stops":[{"at":0,"color":[0.2,0.3,0.4,1]},{"at":1,"color":[0.7,0.9,0.8,1]}]}),
                    )?;
                }
                "image.convert_bit_depth" => {
                    let depth = if args["bit_depth"] == 16 { 8 } else { 16 };
                    let color = if depth == 8 {
                        json!([100, 120, 140, 200])
                    } else {
                        json!([10001, 30003, 50001, 32769])
                    };
                    run(
                        &mut editor,
                        "file.new",
                        json!({"width":16,"height":16,"bit_depth":depth,"background":color,"discard_changes":true}),
                    )?;
                }
                "image.trim" => {
                    run(
                        &mut editor,
                        "select.marquee",
                        json!({"x":2,"y":3,"width":8,"height":9}),
                    )?;
                    run(
                        &mut editor,
                        "paint.fill",
                        json!({"layer":{"name":"Background"},"color":[0.4,0.5,0.6,1]}),
                    )?;
                    run(&mut editor, "select.none", json!({}))?;
                }
                "retouch.spot_heal" | "transform.apply" => {
                    run(
                        &mut editor,
                        "paint.fill",
                        json!({"layer":{"name":"Background"},"color":[0.4,0.5,0.6,1]}),
                    )?;
                    if spec.id == "retouch.spot_heal" {
                        run(
                            &mut editor,
                            "select.marquee",
                            json!({"x":11,"y":8,"width":1,"height":1}),
                        )?;
                        run(
                            &mut editor,
                            "paint.fill",
                            json!({"layer":{"name":"Background"},"color":[1,0,0.3,1]}),
                        )?;
                        run(&mut editor, "select.none", json!({}))?;
                    } else if args["scope"] == "selection" {
                        run(
                            &mut editor,
                            "select.marquee",
                            json!({"x":2,"y":2,"width":6,"height":6}),
                        )?;
                    }
                }
                "paint.stroke" | "paint.erase" if args["target"] == "mask" => {
                    run(
                        &mut editor,
                        "layer.mask.add",
                        json!({"layer":{"name":"Background"},"source":"white"}),
                    )?;
                }
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
                "layer.rasterize" if args["layer"]["name"] == "RasterText" => {
                    run(
                        &mut editor,
                        "type.create",
                        json!({"name":"RasterText","spec":{"text":"Raster中文","font":"sans","font_size":10,"layout":{"type":"point"}},"color":[0.2,0.4,0.8,1],"x":2,"y":12}),
                    )?;
                    run(
                        &mut editor,
                        "transform.apply",
                        json!({"layer":{"name":"RasterText"},"scope":"whole_layer","geometry":{"type":"affine","values":[0,1,-1,0,16,0]},"interpolation":"nearest"}),
                    )?;
                    run(
                        &mut editor,
                        "image.crop",
                        json!({"rect":{"x":1,"y":1,"width":14,"height":14},"pixels":"delete"}),
                    )?;
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
                "file.open" if example["resource"] == "fixture-psd" => {
                    let path = dir.path().join("editable.psd");
                    std::fs::write(
                        &path,
                        include_bytes!(
                            "../../../crates/strata-core/tests/fixtures/psd/group-mask.psd"
                        ),
                    )?;
                    args["resource"] = json!(editor.grant_read(&path, Source::Test)?);
                }
                "file.open" | "file.revert" => {
                    let path = dir.path().join("input.strata");
                    let grant = editor.grant_write(&path, false, Source::Test)?;
                    run(&mut editor, "file.save_as", json!({"resource":grant}))?;
                    let grant = editor.grant_read(&path, Source::Test)?;
                    args["resource"] = json!(grant);
                    if spec.id == "file.revert" {
                        run(
                            &mut editor,
                            "layer.create",
                            json!({"name":"Discard revert fixture"}),
                        )?;
                    }
                }
                "file.recover" => {
                    let store = strata_io::recovery::RecoveryStore::new(dir.path())?;
                    let info = editor
                        .checkpoint_recovery(&store, "Recovery fixture")?
                        .ok_or("missing checkpoint")?;
                    args["resource"] =
                        json!(editor.grant_read(&store.path(&info.session_id)?, Source::Test)?);
                }
                "file.export_psd" => {
                    let input = dir.path().join("editable.psd");
                    std::fs::write(
                        &input,
                        include_bytes!(
                            "../../../crates/strata-core/tests/fixtures/psd/group-mask.psd"
                        ),
                    )?;
                    let grant = editor.grant_read(&input, Source::Test)?;
                    run(
                        &mut editor,
                        "file.open",
                        json!({"resource":grant,"discard_changes":true}),
                    )?;
                    run(
                        &mut editor,
                        "path.create",
                        json!({"name":"交接路径","open_contours":[0],"anchors":[{"position":[0,0],"outgoing":[1,1]},{"position":[2,1],"incoming":[-1,1]}]}),
                    )?;
                    let output = dir.path().join("edited.psd");
                    args["resource"] = json!(editor.grant_write(&output, false, Source::Test)?);
                }
                "file.save" | "file.save_as" | "file.export" | "file.quick_export" => {
                    let grant = editor.grant_write(
                        &dir.path().join(
                            if matches!(spec.id, "file.export" | "file.quick_export") {
                                "export.png"
                            } else {
                                "output.strata"
                            },
                        ),
                        false,
                        Source::Test,
                    )?;
                    args["resource"] = json!(grant);
                }
                _ => {}
            }
            let smart_example = spec.id == "layer.snap_position" && args["layers"] == true;
            let text_raster_example =
                spec.id == "layer.rasterize" && args["layer"]["name"] == "RasterText";
            let origin_example = (spec.id == "view.presentation.set")
                .then(|| args.get("ruler_origin").cloned())
                .flatten();
            let result = if spec.id == "image.duplicate" {
                let prepared = editor.prepare(editor.request(Source::Test), spec.id, args)?;
                let (result, copy) = editor.commit_with_document_copy(prepared)?;
                let copy = copy.ok_or("duplicate example produced no document")?;
                let document = copy
                    .editor
                    .document()
                    .ok_or("duplicate document is empty")?;
                let source = editor.document().unwrap();
                let expected_layers = if result["mode"] == "merged" { 1 } else { 2 };
                if document.id() == source.id()
                    || document.layers().len() != expected_layers
                    || document.export_pixels()? != source.export_pixels()?
                    || !copy.editor.dirty()
                {
                    return Err("duplicate example did not create the expected editable copy or artboard composite".into());
                }
                result
            } else {
                run(&mut editor, spec.id, args)?
            };
            if !result.is_object() {
                return Err(format!("{} returned non-object", spec.id).into());
            }
            if spec.id == "file.open" && example["resource"] == "fixture-psd" {
                let doc = editor.document().ok_or("PSD example did not open")?;
                if doc.bit_depth() != 16
                    || doc.layers().len() != 5
                    || doc.layers()[1].mask().is_none()
                {
                    return Err("PSD example did not preserve editable masked layers".into());
                }
            }
            if spec.id == "file.export_psd" {
                let bytes = std::fs::read(dir.path().join("edited.psd"))?;
                let output = strata_io::psd::Psd::parse(&bytes, strata_core::MAX_LAYERS)?;
                if output.layers.len() != 6
                    || output.format.bit_depth() != 16
                    || output.paths.len() != 1
                    || output.paths[0].name != "交接路径"
                    || !output.paths[0].contours[0].open
                {
                    return Err("layered PSD example lost its editable directory or depth".into());
                }
            }
            if spec.id == "path.from_selection"
                && editor.document().is_none_or(|doc| {
                    doc.paths()
                        .last()
                        .is_none_or(|path| path.subpaths.len() != 1)
                })
            {
                return Err("selection tracing example did not preserve its hole".into());
            }
            if spec.id == "file.revert"
                && (!editor.snapshot().history.is_empty()
                    || editor.document().is_none_or(|d| d.layers().len() != 1))
            {
                return Err("revert example did not reload the actual saved project and clear the unsaved layer/history".into());
            }
            if spec.id == "file.quick_export"
                && (result["format"] != "png" || result["width"] != 16 || result["height"] != 16)
            {
                return Err("quick export example did not execute default preset at the actual fixture dimensions".into());
            }
            if origin_example.is_some_and(|origin| {
                (0..2).any(|i| result["ruler_origin"][i].as_f64() != origin[i].as_f64())
            }) {
                return Err(
                    "ruler origin example did not retain exact fractional canvas pixels".into(),
                );
            }
            if smart_example
                && (result["x"] != 16
                    || result["snapped_x"]["coordinate"] != 32.
                    || result["snapped_x"]["source_anchor"] != "maximum"
                    || result["snapped_x"]["reference_anchor"] != "minimum")
            {
                return Err(
                    "smart-guide example did not execute the independent16px edge alignment".into(),
                );
            }
            if spec.id == "view.rulers.convert" {
                let expected = [25.4, 50.8];
                let canvas = [400., 800.];
                if (0..2).any(|i| {
                    result["ruler"][i]
                        .as_f64()
                        .is_none_or(|v| (v - expected[i]).abs() > 1e-10)
                        || result["canvas"][i]
                            .as_f64()
                            .is_none_or(|v| (v - canvas[i]).abs() > 1e-10)
                }) {
                    return Err(
                        "ruler example did not execute the independent300dpi millimeter reference"
                            .into(),
                    );
                }
            }
            if text_raster_example {
                let layer = editor.document().unwrap().layers().last().unwrap();
                let mut has_ink = false;
                for (_, tile) in layer.pixels().tiles() {
                    let tile = tile?;
                    has_ink |= match &*tile {
                        strata_pixels::PixelBuffer::Rgba8(pixels) => {
                            pixels.chunks_exact(4).any(|pixel| pixel[3] > 0)
                        }
                        strata_pixels::PixelBuffer::Rgba16(pixels) => {
                            pixels.chunks_exact(4).any(|pixel| pixel[3] > 0)
                        }
                    };
                }
                if !matches!(layer.info().kind, strata_core::LayerKind::Pixel)
                    || result["layer_id"] != json!(layer.id())
                    || !has_ink
                {
                    return Err(
                        "text rasterization example did not create real pixel content".into(),
                    );
                }
            }
            if spec.id == "filter.high_pass" && result["touched_tiles"].as_u64().unwrap_or(0) == 0 {
                return Err("high-pass example did not change any source tile".into());
            }
            if spec.id == "image.auto" && result["changed"] != true {
                return Err(
                    "automatic correction example did not execute a real pixel change".into(),
                );
            }
            if matches!(spec.id, "transform.apply" | "transform.apply_layers")
                && result["changed"] != true
            {
                return Err("transform example did not execute a real geometry change".into());
            }
            if spec.id == "retouch.spot_heal"
                && result["healing"]["domain_pixels"].as_u64().unwrap_or(0) == 0
            {
                return Err("spot healing example did not repair any pixels".into());
            }
            if matches!(spec.id, "type.create" | "type.update")
                && (result["changed"] != true
                    || result["metrics"]["glyph_count"].as_u64().unwrap_or(0) == 0)
            {
                return Err(
                    "text example did not shape and execute a real editable text operation".into(),
                );
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
        Some("filter-benchmark") => filter_benchmark::run(Path::new(args.get(2).ok_or("missing new report path")?))?,
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
        _ => return Err("usage: strata-devtool catalog | examples | docs NEW_MARKDOWN | run SCRIPT.json | demo NEW_DIRECTORY | capacity | selection-benchmark NEW_REPORT | tonal-benchmark NEW_REPORT | filter-benchmark NEW_REPORT | fixture-project NEW_PROJECT WIDTH HEIGHT LAYERS DEPTH | memory-stress NEW_DIRECTORY [WIDTH HEIGHT LAYERS] | verify-exports PROJECT IMAGE...".into()),
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
