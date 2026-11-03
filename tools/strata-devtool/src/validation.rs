use crate::run;
use serde_json::{Value, json};
use std::{error::Error, path::Path, time::Instant};
use strata_core::{Editor, Source};
use strata_io::{
    decode_image,
    export::{ExportFormat, export_image},
};
use strata_pixels::PixelBuffer;

/// Read-only comparison of native exports against a reopened project.
pub fn verify_exports(project: &Path, outputs: &[String]) -> Result<Value, Box<dyn Error>> {
    if outputs.is_empty() {
        return Err("at least one export is required".into());
    }
    let mut editor = Editor::new();
    let resource = editor.grant_read(project, Source::Test)?;
    run(&mut editor, "file.open", json!({"resource":resource}))?;
    let doc = editor.document().ok_or("missing document")?;
    let expected = doc.export_pixels()?;
    let mut cases = Vec::new();
    for output in outputs {
        let image = decode_image(Path::new(output))?;
        if image.width != doc.frame().width
            || image.height != doc.frame().height
            || image.pixels != expected
            || image.icc_profile.as_deref() != Some(doc.working_icc())
        {
            return Err(format!("export mismatch: {output}").into());
        }
        cases.push(json!({"file":Path::new(output).file_name().unwrap().to_string_lossy(),"pixels_icc_dimensions_exact":true}));
    }
    Ok(
        json!({"passed":true,"bit_depth":doc.bit_depth(),"layers":doc.layers().iter().map(|l|l.info()).collect::<Vec<_>>(),"outputs":cases}),
    )
}

/// Reproducible file/command pipeline, refusing to overwrite previous evidence.
pub fn demo(directory: &Path) -> Result<Value, Box<dyn Error>> {
    std::fs::create_dir(directory)?;
    let icc = strata_color::working_profile("srgb")?;
    let mut cases = Vec::new();
    for depth in [8, 16] {
        let source = if depth == 8 {
            PixelBuffer::Rgba8(
                (0..32 * 24)
                    .flat_map(|i| [i as u8, (i * 3) as u8, (i * 7) as u8, (i * 11) as u8])
                    .collect(),
            )
        } else {
            PixelBuffer::Rgba16(
                (0..32 * 24)
                    .flat_map(|i| {
                        [
                            32768 + (i % 2) as u16,
                            (i * 73) as u16,
                            (i * 37) as u16,
                            (i * 91) as u16,
                        ]
                    })
                    .collect(),
            )
        };
        for (extension, format) in [("png", ExportFormat::Png), ("tiff", ExportFormat::Tiff)] {
            let input = directory.join(format!("source-{depth}.{extension}"));
            export_image(&input, format, 32, 24, &source, &icc, false)?;
            let mut editor = Editor::new();
            let resource = editor.grant_read(&input, Source::Test)?;
            run(&mut editor, "file.open", json!({"resource":resource}))?;
            assert_eq!(editor.document().unwrap().export_pixels()?, source);
            let layer = run(
                &mut editor,
                "layer.duplicate",
                json!({"layer":{"name":format!("source-{depth}")}}),
            )?;
            let target = json!({"id":layer["layer_id"]});
            for (command, params) in [
                ("layer.rename", json!({"layer":target,"name":"Edited"})),
                ("layer.set_position", json!({"layer":target,"x":-3,"y":2})),
                ("layer.set_opacity", json!({"layer":target,"opacity":0.6})),
                (
                    "layer.set_blend_mode",
                    json!({"layer":target,"blend_mode":"screen"}),
                ),
                (
                    "layer.set_visibility",
                    json!({"layer":target,"visible":false}),
                ),
                ("edit.undo", json!({})),
                ("edit.redo", json!({})),
                ("edit.undo", json!({})),
            ] {
                run(&mut editor, command, params)?;
            }
            let expected = editor.document().unwrap().export_pixels()?;
            let archive = editor.document().unwrap().to_archive()?;
            let project = directory.join(format!("edited-{depth}-{extension}.strata"));
            let resource = editor.grant_write(&project, false, Source::Test)?;
            run(&mut editor, "file.save_as", json!({"resource":resource}))?;
            assert!(!editor.dirty());
            let resource = editor.grant_read(&project, Source::Test)?;
            run(&mut editor, "file.open", json!({"resource":resource}))?;
            assert_eq!(editor.document().unwrap().to_archive()?, archive);
            for (output_ext, _) in [("png", ExportFormat::Png), ("tiff", ExportFormat::Tiff)] {
                let output = directory.join(format!("export-{depth}-{extension}.{output_ext}"));
                let resource = editor.grant_write(&output, false, Source::Test)?;
                run(
                    &mut editor,
                    "file.export",
                    json!({"resource":resource,"format":output_ext}),
                )?;
                let decoded = decode_image(&output)?;
                assert_eq!(decoded.pixels, expected);
                assert_eq!(decoded.icc_profile.as_deref(), Some(icc.as_slice()));
            }
            cases.push(json!({"depth":depth,"input":extension,"outputs":["png","tiff"],"ids_metadata_tiles_icc_exact":true,"export_samples_exact":true}));
        }
    }
    Ok(json!({"cases":cases,"passed":true}))
}

/// Each fill allocates new source blocks; no shared duplicate masquerades as
/// an independent layer. Stop at the documented M1a admission limit.
pub fn capacity() -> Result<Value, Box<dyn Error>> {
    let mut reports = Vec::new();
    for depth in [8, 16] {
        let mut editor = Editor::new();
        run(
            &mut editor,
            "file.new",
            json!({"width":6000,"height":4000,"bit_depth":depth}),
        )?;
        let mut steps = Vec::new();
        for n in 1..=20 {
            let start = Instant::now();
            let max = if depth == 8 { 255 } else { 65535 };
            let result = run(
                &mut editor,
                "layer.create",
                json!({"name":format!("Independent {n}"),"fill":[n*7,n*11,n*13,max]}),
            );
            let bytes: u64 = editor
                .document()
                .unwrap()
                .layers()
                .iter()
                .map(|l| l.pixels().referenced_bytes())
                .sum();
            steps.push(json!({"attempted_layer":n,"milliseconds":start.elapsed().as_secs_f64()*1000.0,"admitted":result.is_ok(),"referenced_source_bytes":bytes,"error":result.as_ref().err().map(ToString::to_string)}));
            if result.is_err() {
                break;
            }
        }
        reports.push(json!({"depth":depth,"width":6000,"height":4000,"target_independent_layers":20,"steps":steps}));
    }
    Ok(
        json!({"stage":"M1a bounded admission; full 20-layer residency is M1b","document_limit_bytes":strata_core::MAX_DOCUMENT_BYTES,"cases":reports}),
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn all_depths_and_formats_roundtrip() {
        let directory = tempfile::tempdir().unwrap();
        assert!(
            super::demo(&directory.path().join("demo")).unwrap()["passed"]
                .as_bool()
                .unwrap()
        );
    }
}
