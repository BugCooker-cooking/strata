use serde_json::{Value, json};
use strata_core::{Editor, Source};
fn run(e: &mut Editor, id: &str, p: Value) -> Value {
    e.execute(e.request(Source::User), id, p, |_| Ok(()))
        .unwrap()
}
fn set(e: &mut Editor, target: &str, color: Value) -> Value {
    run(
        e,
        "paint.colors.set_components",
        json!({"target":target,"color":color}),
    )
}
fn rgba(value: &Value) -> [f64; 4] {
    std::array::from_fn(|i| value[i].as_f64().unwrap())
}
fn near(actual: [f64; 4], expected: [f64; 4]) {
    assert!(
        (0..4).all(|i| (actual[i] - expected[i]).abs() < 1e-12),
        "{actual:?} != {expected:?}"
    );
}
#[test]
fn all_five_color_models_share_exact_working_rgb_state_with_or_without_a_document() {
    for id in [
        "paint.colors.get",
        "paint.colors.describe",
        "paint.colors.set",
        "paint.colors.set_components",
        "paint.colors.swap",
        "paint.colors.reset",
    ] {
        let spec = strata_core::commands::catalog()
            .iter()
            .find(|spec| spec.id == id)
            .unwrap();
        assert_eq!(
            serde_json::to_value(&spec.returns).unwrap()["additionalProperties"],
            false,
            "loose palette result: {id}"
        );
    }
    for doc in [None, Some((8, "srgb")), Some((16, "display_p3"))] {
        for (color, expected) in [
            (
                json!({"model":"rgb","red":0.123456789012,"green":0.3,"blue":0.8,"alpha":0.333333333333}),
                [0.123456789012, 0.3, 0.8, 0.333333333333],
            ),
            (
                json!({"model":"rgb8","red":51,"green":102,"blue":153,"alpha":0.5}),
                [0.2, 0.4, 0.6, 0.5],
            ),
            (
                json!({"model":"rgb16","red":10001,"green":30003,"blue":50001}),
                [10001. / 65535., 30003. / 65535., 50001. / 65535., 1.],
            ),
            (
                json!({"model":"hsb","hue":210,"saturation":50,"brightness":80,"alpha":0.5}),
                [0.4, 0.6, 0.8, 0.5],
            ),
            (
                json!({"model":"hex","value":"#33669980"}),
                [0.2, 0.4, 0.6, 128. / 255.],
            ),
        ] {
            let mut e = Editor::new();
            if let Some((depth, space)) = doc {
                run(
                    &mut e,
                    "file.new",
                    json!({"width":4,"height":3,"bit_depth":depth,"working_space":space}),
                );
            }
            let before = e.snapshot();
            let archive = e.document().map(|d| d.to_archive().unwrap());
            let result = set(&mut e, "foreground", color);
            near(rgba(&result["foreground"]), expected);
            assert_eq!(result["background"], json!([1., 1., 1., 1.]));
            assert_eq!(result["version"], before.paint_colors.version + 1);
            let described = run(&mut e, "paint.colors.describe", json!({}));
            near(rgba(&described["foreground"]["rgba"]), expected);
            assert_eq!(described["version"], result["version"]);
            let after = e.snapshot();
            assert_eq!(after.revision, before.revision);
            assert_eq!(
                serde_json::to_value(after.history).unwrap(),
                serde_json::to_value(before.history).unwrap()
            );
            assert_eq!(after.dirty, before.dirty);
            assert_eq!(after.active_layer, before.active_layer);
            assert_eq!(e.document().map(|d| d.to_archive().unwrap()), archive);
        }
    }
}
#[test]
fn hue_sectors_wrap_gray_and_alpha_have_fixed_independent_reference_values() {
    let mut e = Editor::new();
    for (h, base) in [
        (0., [1., 0., 0.]),
        (30., [1., 0.5, 0.]),
        (60., [1., 1., 0.]),
        (90., [0.5, 1., 0.]),
        (120., [0., 1., 0.]),
        (180., [0., 1., 1.]),
        (240., [0., 0., 1.]),
        (300., [1., 0., 1.]),
        (360., [1., 0., 0.]),
    ] {
        let result = set(
            &mut e,
            "background",
            json!({"model":"hsb","hue":h,"saturation":50,"brightness":80,"alpha":0.25}),
        );
        near(
            rgba(&result["background"]),
            [
                0.4 + base[0] * 0.4,
                0.4 + base[1] * 0.4,
                0.4 + base[2] * 0.4,
                0.25,
            ],
        );
        let d = run(&mut e, "paint.colors.describe", json!({}));
        let actual = d["background"]["hsb"].as_array().unwrap();
        assert!((actual[0].as_f64().unwrap() - if h == 360. { 0. } else { h }).abs() < 1e-10);
        assert!((actual[1].as_f64().unwrap() - 50.).abs() < 1e-10);
        assert!((actual[2].as_f64().unwrap() - 80.).abs() < 1e-10);
    }
    for (h, s, v) in [(271., 0., 50.), (123., 80., 0.)] {
        set(
            &mut e,
            "foreground",
            json!({"model":"hsb","hue":h,"saturation":s,"brightness":v}),
        );
        let d = run(&mut e, "paint.colors.describe", json!({}));
        assert_eq!(d["foreground"]["hsb"][0].as_f64(), Some(0.));
        assert_eq!(d["foreground"]["hsb"][1].as_f64(), Some(0.));
        assert_eq!(d["foreground"]["hsb"][2], v);
    }
}
#[test]
fn description_rounds_display_codes_only_and_is_readonly_even_after_layer_selection() {
    let mut e = Editor::new();
    run(
        &mut e,
        "file.new",
        json!({"width":4,"height":3,"bit_depth":16}),
    );
    run(&mut e, "layer.create", json!({"name":"second"}));
    run(
        &mut e,
        "paint.colors.set",
        json!({"foreground":[0.123456789,0.5,0.8,0.3333333333]}),
    );
    let before = e.snapshot();
    let request = e.request(Source::User);
    let prepared = e
        .prepare(request, "paint.colors.describe", json!({}))
        .unwrap();
    run(
        &mut e,
        "layer.select",
        json!({"layer":{"name":"Background"}}),
    );
    let active = e.snapshot().active_layer;
    let d = e.commit(prepared).unwrap();
    near(
        rgba(&d["foreground"]["rgba"]),
        [0.123456789, 0.5, 0.8, 0.3333333333],
    );
    assert_eq!(d["foreground"]["rgb8"], json!([31, 128, 204, 85]));
    assert_eq!(d["foreground"]["rgb16"], json!([8091, 32768, 52428, 21845]));
    assert_eq!(d["foreground"]["hex"], "#1F80CC55");
    assert_eq!(e.snapshot().active_layer, active);
    assert_eq!(e.revision(), before.revision);
    assert_eq!(e.snapshot().paint_colors, before.paint_colors);
    let before = serde_json::to_value(e.snapshot()).unwrap();
    run(&mut e, "paint.colors.describe", json!({}));
    assert_eq!(serde_json::to_value(e.snapshot()).unwrap(), before);
}
#[test]
fn hex_exact_syntax_and_every_invalid_numeric_or_unknown_field_leave_palette_and_document_unchanged()
 {
    let mut e = Editor::new();
    for (text, expected) in [
        ("ff8800", [1., 136. / 255., 0., 1.]),
        ("#Ff880000", [1., 136. / 255., 0., 0.]),
        ("000000", [0., 0., 0., 1.]),
        ("FFFFFFFF", [1., 1., 1., 1.]),
    ] {
        near(
            rgba(&set(&mut e, "foreground", json!({"model":"hex","value":text}))["foreground"]),
            expected,
        );
    }
    let before = serde_json::to_value(e.snapshot()).unwrap();
    for color in [
        json!({"model":"rgb","red":1.1,"green":0,"blue":0}),
        json!({"model":"rgb","red":0,"green":0,"blue":0,"alpha":-0.1}),
        json!({"model":"rgb8","red":256,"green":0,"blue":0}),
        json!({"model":"rgb8","red":255.0,"green":0,"blue":0}),
        json!({"model":"rgb16","red":65536,"green":0,"blue":0}),
        json!({"model":"hsb","hue":361,"saturation":0,"brightness":0}),
        json!({"model":"hsb","hue":-1,"saturation":0,"brightness":0}),
        json!({"model":"hsb","hue":1,"saturation":101,"brightness":50}),
        json!({"model":"hsb","hue":1,"saturation":50,"brightness":101}),
        json!({"model":"hsb","hue":1,"saturation":50,"brightness":50,"alpha":2}),
        json!({"model":"cmyk","cyan":0}),
        json!({"model":"hex","value":"#12345"}),
        json!({"model":"hex","value":" #123456"}),
        json!({"model":"hex","value":"##123456"}),
        json!({"model":"hex","value":"#123456789"}),
        json!({"model":"hex","value":"#gggggg"}),
        json!({"model":"hex","value":"#123456","alpha":0.5}),
    ] {
        assert!(
            e.execute(
                e.request(Source::User),
                "paint.colors.set_components",
                json!({"target":"foreground","color":color}),
                |_| Ok(())
            )
            .is_err()
        );
        assert_eq!(serde_json::to_value(e.snapshot()).unwrap(), before);
    }
    for params in [
        json!({"target":"both","color":{"model":"hex","value":"#123456"}}),
        json!({"target":"foreground","color":{"model":"hex","value":"#123456"},"source":"user"}),
    ] {
        assert!(
            e.execute(
                e.request(Source::User),
                "paint.colors.set_components",
                params,
                |_| Ok(())
            )
            .is_err()
        );
        assert_eq!(serde_json::to_value(e.snapshot()).unwrap(), before);
    }
}
#[test]
fn formatted_writes_reject_stale_versions_and_control_changes_while_descriptions_remain_readable() {
    let mut e = Editor::new();
    run(
        &mut e,
        "file.new",
        json!({"width":4,"height":3,"bit_depth":8}),
    );
    let prepared=e.prepare(e.request(Source::User),"paint.colors.set_components",json!({"target":"background","color":{"model":"hex","value":"#123456"},"expected_version":0})).unwrap();
    run(
        &mut e,
        "paint.colors.set",
        json!({"foreground":[0.2,0.4,0.6,1]}),
    );
    let before = e.snapshot().paint_colors;
    assert!(e.commit(prepared).is_err());
    assert_eq!(e.snapshot().paint_colors, before);
    assert!(e.execute(e.request(Source::User),"paint.colors.set_components",json!({"target":"background","color":{"model":"hex","value":"#123456"},"expected_version":0}),|_|Ok(())).is_err());
    let original_dirty = e.snapshot().dirty;
    let turn = e.begin_turn().unwrap();
    for paused in [false, true] {
        if paused {
            e.pause_turn().unwrap();
        }
        assert!(
            e.execute(
                e.request(Source::User),
                "paint.colors.set_components",
                json!({"target":"foreground","color":{"model":"hex","value":"#123456"}}),
                |_| Ok(())
            )
            .is_err()
        );
        run(&mut e, "paint.colors.describe", json!({}));
    }
    e.resume_turn().unwrap();
    e.execute(e.request(Source::Ai{turn_id:turn}),"paint.colors.set_components",json!({"target":"foreground","color":{"model":"hsb","hue":120,"saturation":100,"brightness":100}}),|_|Ok(())).unwrap();
    e.stop_turn();
    assert_eq!(e.snapshot().paint_colors.foreground, [0., 1., 0., 1.]);
    assert!(e.snapshot().history.is_empty());
    assert_eq!(e.snapshot().dirty, original_dirty);
}
#[test]
fn formatted_palette_can_drive_real_fill_and_document_undo_does_not_revert_tool_colors() {
    let mut e = Editor::new();
    run(
        &mut e,
        "file.new",
        json!({"width":4,"height":3,"bit_depth":8}),
    );
    let before = e.document().unwrap().to_archive().unwrap();
    let colors = set(
        &mut e,
        "foreground",
        json!({"model":"hex","value":"#33669980"}),
    );
    run(
        &mut e,
        "paint.fill",
        json!({"layer":{"name":"Background"},"color":colors["foreground"]}),
    );
    let strata_pixels::PixelValue::Rgba8(actual) = e.document().unwrap().layers()[0]
        .pixels()
        .pixel(0, 0)
        .unwrap()
    else {
        panic!()
    };
    assert!((0..3).all(|c| actual[c].abs_diff([51, 102, 153][c]) <= 1));
    assert_eq!(actual[3], 128);
    run(&mut e, "edit.undo", json!({}));
    assert_eq!(e.document().unwrap().to_archive().unwrap(), before);
    assert_eq!(run(&mut e, "paint.colors.get", json!({})), colors);
}
