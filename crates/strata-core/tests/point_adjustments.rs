use serde_json::{Value, json};
use strata_core::{Editor, Source};
use strata_pixels::PixelBuffer;
fn run(e: &mut Editor, id: &str, p: Value) -> Value {
    e.execute(e.request(Source::User), id, p, |_| Ok(()))
        .unwrap()
}
fn samples(e: &Editor) -> Vec<u16> {
    match e.document().unwrap().export_pixels().unwrap() {
        PixelBuffer::Rgba16(v) => v,
        PixelBuffer::Rgba8(v) => v.into_iter().map(u16::from).collect(),
    }
}
#[test]
fn every_point_adjustment_preserves_masked_protection_alpha_source_and_project_history() {
    for depth in [8, 16] {
        for space in ["srgb", "display_p3"] {
            for kind in [
                "vibrance",
                "white_balance",
                "color_balance",
                "black_white",
                "photo_filter",
                "channel_mixer",
                "invert",
                "posterize",
                "threshold",
                "gradient_map",
                "selective_color",
                "desaturate",
            ] {
                let mut e = Editor::new();
                let maximum = if depth == 16 { 65535 } else { 255 };
                run(
                    &mut e,
                    "file.new",
                    json!({"width":4,"height":2,"bit_depth":depth,"working_space":space,"background":[maximum/4,maximum/2,maximum*3/4,maximum/2]}),
                );
                let before = samples(&e);
                let source = e.document().unwrap().layers()[0].pixels().clone();
                run(
                    &mut e,
                    "select.marquee",
                    json!({"x":0,"y":0,"width":2,"height":2}),
                );
                let settings = match kind {
                    "white_balance" => json!({"type":kind,"temperature":25,"tint":-10}),
                    "vibrance" => json!({"type":kind,"vibrance":30}),
                    "color_balance" => json!({"type":kind,"midtones":[10,-5,5]}),
                    "selective_color" => {
                        json!({"type":kind,"relative":false,"ranges":[[0,0,0,0],[0,0,0,0],[0,0,0,0],[0,0,0,0],[0,0,0,0],[0,0,0,0],[0,0,0,0],[10,0,0,0],[0,0,0,0]]})
                    }
                    _ => json!({"type":kind}),
                };
                let layer = run(
                    &mut e,
                    "layer.add_tonal_adjustment",
                    json!({"name":kind,"adjustment":settings}),
                )["layer_id"]
                    .clone();
                let after = samples(&e);
                for y in 0..2 {
                    for x in 0..4 {
                        let index = (y * 4 + x) * 4;
                        assert_eq!(after[index + 3], before[index + 3]);
                        if x >= 2 {
                            for c in 0..3 {
                                assert!(
                                    after[index + c].abs_diff(before[index + c])
                                        <= if depth == 16 { 4 } else { 1 },
                                    "{kind} {space} depth {depth} protected channel changed"
                                );
                            }
                        }
                    }
                }
                for coord in source.tile_coords() {
                    assert_eq!(
                        source.tile_version(*coord),
                        e.document().unwrap().layers()[0]
                            .pixels()
                            .tile_version(*coord)
                    );
                }
                let archive = e.document().unwrap().to_archive().unwrap();
                let reopened = strata_core::Document::from_archive(archive).unwrap();
                assert_eq!(
                    reopened.layers().last().unwrap().info().kind,
                    e.document().unwrap().layers().last().unwrap().info().kind
                );
                assert_eq!(
                    reopened.export_pixels().unwrap(),
                    e.document().unwrap().export_pixels().unwrap()
                );
                run(&mut e, "edit.undo", json!({}));
                assert_eq!(samples(&e), before);
                run(&mut e, "edit.redo", json!({}));
                assert_eq!(samples(&e), after);
                run(
                    &mut e,
                    "adjust.tonal.set",
                    json!({"layer":{"id":layer},"adjustment":settings}),
                );
            }
        }
    }
}
#[test]
fn neutral_color_layers_preserve_exact_source_without_quantization() {
    for depth in [8, 16] {
        for kind in [
            "vibrance",
            "white_balance",
            "color_balance",
            "channel_mixer",
            "selective_color",
        ] {
            let mut e = Editor::new();
            let max = if depth == 16 { 65535 } else { 255 };
            run(
                &mut e,
                "file.new",
                json!({"width":2,"height":2,"bit_depth":depth,"background":[max/2,max/2+1,max*3/4,0]}),
            );
            let before = samples(&e);
            run(
                &mut e,
                "layer.add_tonal_adjustment",
                json!({"name":kind,"adjustment":{"type":kind}}),
            );
            assert_eq!(samples(&e), before);
        }
    }
}
