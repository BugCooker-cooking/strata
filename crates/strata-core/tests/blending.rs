use serde_json::json;
use strata_core::{BlendMode, Editor, Source, composite::blend};
use strata_pixels::PixelBuffer;

#[test]
fn twenty_seven_modes_match_fixed_opaque_neutral_references() {
    use BlendMode::*;
    let expected = [
        (Normal, 0.75),
        (Dissolve, 0.75),
        (Darken, 0.25),
        (Multiply, 0.1875),
        (ColorBurn, 0.0),
        (LinearBurn, 0.0),
        (DarkerColor, 0.25),
        (Lighten, 0.75),
        (Screen, 0.8125),
        (ColorDodge, 1.0),
        (LinearDodge, 1.0),
        (LighterColor, 0.75),
        (Overlay, 0.375),
        (SoftLight, 0.375),
        (HardLight, 0.625),
        (VividLight, 0.5),
        (LinearLight, 0.75),
        (PinLight, 0.5),
        (HardMix, 1.0),
        (Difference, 0.5),
        (Exclusion, 0.625),
        (Subtract, 0.0),
        (Divide, 1.0 / 3.0),
        (Hue, 0.25),
        (Saturation, 0.25),
        (Color, 0.25),
        (Luminosity, 0.75),
    ];
    assert_eq!(BlendMode::ALL.len(), 27);
    for (mode, value) in expected {
        let result = blend([0.25, 0.25, 0.25, 1.0], [0.75, 0.75, 0.75, 1.0], mode, 1.0);
        for c in &result[..3] {
            assert!((c - value).abs() < 1e-12, "{mode:?}: {result:?}");
        }
        assert_eq!(result[3], 1.0);
    }
}
#[test]
fn all_modes_handle_alpha_black_white_and_colored_extrema_without_nan() {
    for mode in BlendMode::ALL {
        for alpha in [0.0, 0.25, 1.0] {
            for back in [
                [0.0, 0.0, 0.0, alpha],
                [1.0, 1.0, 1.0, alpha],
                [0.9, 0.3, 0.1, alpha],
            ] {
                for front in [
                    [0.0, 1.0, 0.0, alpha],
                    [1.0, 0.0, 1.0, alpha],
                    [0.0, 0.0, 0.0, alpha],
                    [1.0, 1.0, 1.0, alpha],
                ] {
                    let result = blend(back, front, mode, 0.5);
                    assert!(
                        result
                            .iter()
                            .all(|v| v.is_finite() && *v >= -1e-12 && *v <= 1.0 + 1e-12),
                        "{mode:?}: {result:?}"
                    );
                    assert_eq!(
                        blend(back, front, mode, 0.0),
                        if back[3] == 0.0 { [0.0; 4] } else { back }
                    );
                }
            }
        }
    }
}
fn execute(editor: &mut Editor, id: &str, params: serde_json::Value) -> serde_json::Value {
    editor
        .execute(editor.request(Source::User), id, params, |_| Ok(()))
        .unwrap()
}
#[test]
fn all_modes_export_both_depths_and_roundtrip_without_changing_source() {
    for depth in [8, 16] {
        let maximum = if depth == 8 { 255u16 } else { 65535u16 };
        let mut editor = Editor::new();
        execute(
            &mut editor,
            "file.new",
            json!({"width":2,"height":2,"bit_depth":depth,"background":[maximum/4,maximum/4,maximum/4,maximum]}),
        );
        let layer = execute(
            &mut editor,
            "layer.create",
            json!({"name":"front","fill":[maximum/4*3,maximum/4*3,maximum/4*3,maximum]}),
        )["layer_id"]
            .clone();
        let original = editor.document().unwrap().layers()[1].pixels().clone();
        for mode in BlendMode::ALL {
            execute(
                &mut editor,
                "layer.set_blend_mode",
                json!({"layer":{"id":layer},"blend_mode":mode}),
            );
            let output = editor.document().unwrap().export_pixels().unwrap();
            assert_eq!(output.format().bit_depth(), depth);
            let doc = strata_core::Document::from_archive(
                editor.document().unwrap().to_archive().unwrap(),
            )
            .unwrap();
            assert_eq!(doc.export_pixels().unwrap(), output);
            assert_eq!(
                original.pixel(0, 0).unwrap(),
                doc.layers()[1].pixels().pixel(0, 0).unwrap()
            );
        }
    }
}
#[test]
fn dissolve_uses_stable_canvas_samples_and_preserves_source_alpha() {
    let mut editor = Editor::new();
    execute(
        &mut editor,
        "file.new",
        json!({"width":256,"height":256,"bit_depth":8}),
    );
    let layer = execute(
        &mut editor,
        "layer.create",
        json!({"name":"noise","fill":[100,50,200,128]}),
    )["layer_id"]
        .clone();
    execute(
        &mut editor,
        "layer.set_blend_mode",
        json!({"layer":{"id":layer},"blend_mode":"dissolve"}),
    );
    let first = editor.document().unwrap().export_pixels().unwrap();
    assert_eq!(editor.document().unwrap().export_pixels().unwrap(), first);
    let PixelBuffer::Rgba8(values) = first else {
        panic!()
    };
    let count = values.chunks_exact(4).filter(|p| p[3] == 255).count();
    assert!((31000..34500).contains(&count));
    assert!(values.chunks_exact(4).all(|p| p[3] == 0 || p[3] == 255));
    assert_eq!(
        editor.document().unwrap().layers()[1]
            .pixels()
            .pixel(0, 0)
            .unwrap(),
        strata_pixels::PixelValue::Rgba8([100, 50, 200, 128])
    );
}
#[test]
fn fill_opacity_labels_and_locks_survive_undo_and_block_edits_atomically() {
    let mut editor = Editor::new();
    execute(
        &mut editor,
        "file.new",
        json!({"width":1,"height":1,"bit_depth":16,"background":[100,200,300,65535]}),
    );
    execute(
        &mut editor,
        "layer.set_fill_opacity",
        json!({"layer":{"name":"Background"},"fill_opacity":0.5}),
    );
    let PixelBuffer::Rgba16(values) = editor.document().unwrap().export_pixels().unwrap() else {
        panic!()
    };
    assert!(values[3].abs_diff(32768) <= 1);
    execute(
        &mut editor,
        "layer.set_color_label",
        json!({"layer":{"name":"Background"},"color_label":"violet"}),
    );
    execute(
        &mut editor,
        "layer.set_locks",
        json!({"layer":{"name":"Background"},"locks":{"position":true,"pixels":true}}),
    );
    let before = editor.revision();
    for (id, params) in [
        (
            "layer.set_position",
            json!({"layer":{"name":"Background"},"x":5,"y":0}),
        ),
        ("color.convert", json!({"space":"display_p3"})),
    ] {
        assert!(
            editor
                .execute(editor.request(Source::User), id, params, |_| Ok(()))
                .is_err()
        );
        assert_eq!(editor.revision(), before);
    }
    execute(
        &mut editor,
        "layer.set_locks",
        json!({"layer":{"name":"Background"},"locks":{"all":true}}),
    );
    assert!(
        editor
            .execute(
                editor.request(Source::User),
                "layer.delete",
                json!({"layer":{"name":"Background"}}),
                |_| Ok(())
            )
            .is_err()
    );
    execute(
        &mut editor,
        "layer.set_visibility",
        json!({"layer":{"name":"Background"},"visible":false}),
    );
    let before = editor.document().unwrap().to_archive().unwrap();
    let doc = strata_core::Document::from_archive(before.clone()).unwrap();
    assert_eq!(doc.to_archive().unwrap(), before);
    assert_eq!(
        doc.layers()[0].info().color_label,
        Some(strata_core::ColorLabel::Violet)
    );
    execute(
        &mut editor,
        "layer.set_locks",
        json!({"layer":{"name":"Background"},"locks":{}}),
    );
    execute(
        &mut editor,
        "layer.set_position",
        json!({"layer":{"name":"Background"},"x":5,"y":0}),
    );
}
