use serde_json::{Value, json};
use strata_core::{Editor, Source};
use strata_pixels::PixelBuffer;
fn run(e: &mut Editor, id: &str, p: Value) -> Value {
    e.execute(e.request(Source::User), id, p, |_| Ok(()))
        .unwrap()
}
fn editor() -> Editor {
    let mut e = Editor::new();
    run(
        &mut e,
        "file.new",
        json!({"width":16,"height":16,"bit_depth":16}),
    );
    run(
        &mut e,
        "layer.create",
        json!({"name":"shape","fill":[65535,65535,65535,65535]}),
    );
    run(
        &mut e,
        "select.marquee",
        json!({"x":4,"y":4,"width":4,"height":4}),
    );
    run(
        &mut e,
        "layer.mask.add",
        json!({"layer":{"name":"shape"},"source":"from_selection"}),
    );
    e
}
fn pixels(e: &Editor) -> Vec<u16> {
    let PixelBuffer::Rgba16(v) = e.document().unwrap().export_pixels().unwrap() else {
        panic!()
    };
    v
}
fn pixel(v: &[u16], x: usize, y: usize) -> &[u16] {
    &v[(y * 16 + x) * 4..(y * 16 + x + 1) * 4]
}
#[test]
fn shadow_and_stroke_have_fixed_geometry_and_fill_opacity_does_not_fade_effects() {
    let mut e = editor();
    run(
        &mut e,
        "layer.set_styles",
        json!({"layer":{"name":"shape"},"styles":{
        "drop_shadow":{"color":[0,0,65535,65535],"offset_x":4,"offset_y":0,"blur":0},
        "stroke":{"color":[65535,0,0,65535],"size":1,"position":"outside"}}}),
    );
    let v = pixels(&e);
    assert!(pixel(&v, 5, 5)[0] >= 65532 && pixel(&v, 5, 5)[1] >= 65532);
    let shadow = pixel(&v, 10, 5);
    assert!(shadow[2] > 65532 && shadow[0] < 3 && shadow[1] < 3 && shadow[3] == 65535);
    let stroke = pixel(&v, 3, 5);
    assert!(stroke[0] > 65532 && stroke[1] < 3 && stroke[2] < 3 && stroke[3] == 65535);
    run(
        &mut e,
        "layer.set_fill_opacity",
        json!({"layer":{"name":"shape"},"fill_opacity":0}),
    );
    let faded = pixels(&e);
    assert_eq!(pixel(&faded, 5, 5)[3], 0);
    assert_eq!(pixel(&faded, 3, 5), stroke);
    assert_eq!(pixel(&faded, 10, 5), shadow);
}
#[test]
fn outer_glow_is_symmetric_and_overlays_preserve_original_alpha_and_source() {
    let mut e = editor();
    let source = e.document().unwrap().layers()[1].pixels().clone();
    run(
        &mut e,
        "layer.set_styles",
        json!({"layer":{"name":"shape"},"styles":{"outer_glow":{"color":[0,65535,0,65535],"size":3},"color_overlay":{"color":[65535,0,0,65535],"opacity":1,"blend_mode":"normal"}}}),
    );
    let v = pixels(&e);
    assert_eq!(pixel(&v, 3, 5)[3], pixel(&v, 8, 5)[3]);
    assert!(pixel(&v, 3, 5)[3] > 0);
    assert_eq!(pixel(&v, 5, 5)[3], 65535);
    assert!(pixel(&v, 5, 5)[0] > 65532 && pixel(&v, 5, 5)[1] < 3);
    assert_eq!(
        source.pixel(0, 0).unwrap(),
        e.document().unwrap().layers()[1]
            .pixels()
            .pixel(0, 0)
            .unwrap()
    );
}
#[test]
fn gradient_overlay_uses_linear_light_and_styles_survive_project_and_undo() {
    let mut e = editor();
    let before = e.document().unwrap().to_archive().unwrap();
    run(
        &mut e,
        "layer.set_styles",
        json!({"layer":{"name":"shape"},"styles":{"gradient_overlay":{"start":[0,0,0,65535],"end":[65535,65535,65535,65535],"angle":0,"scale":1,"opacity":1}}}),
    );
    let values = pixels(&e);
    let expected = (1.055 * (5.5f64 / 16.0).powf(1.0 / 2.4) - 0.055) * 65535.0;
    assert!((f64::from(pixel(&values, 5, 5)[0]) - expected).abs() <= 3.0);
    assert_eq!(pixel(&values, 0, 0)[3], 0);
    let archive = e.document().unwrap().to_archive().unwrap();
    let reopened = strata_core::Document::from_archive(archive.clone()).unwrap();
    assert_eq!(reopened.to_archive().unwrap(), archive);
    assert_eq!(
        reopened.export_pixels().unwrap(),
        PixelBuffer::Rgba16(values)
    );
    run(&mut e, "edit.undo", json!({}));
    assert_eq!(e.document().unwrap().to_archive().unwrap(), before);
}
#[test]
fn invalid_style_parameters_never_change_history() {
    let mut e = editor();
    let revision = e.revision();
    for styles in [
        json!({"stroke":{"color":[1,2,3,65535],"size":1000}}),
        json!({"drop_shadow":{"color":[0,0,0,65535],"blur":1,"offset_x":-2147483648i64,"offset_y":0}}),
    ] {
        assert!(
            e.execute(
                e.request(Source::User),
                "layer.set_styles",
                json!({"layer":{"name":"shape"},"styles":styles}),
                |_| Ok(())
            )
            .is_err()
        );
        assert_eq!(e.revision(), revision);
    }
}

#[test]
fn all_five_style_types_agree_between_eight_and_sixteen_bit_reference_samples() {
    let mut images = Vec::new();
    for depth in [8, 16] {
        let max = if depth == 8 { 255u16 } else { 65535u16 };
        let mut e = Editor::new();
        run(
            &mut e,
            "file.new",
            json!({"width":16,"height":16,"bit_depth":depth}),
        );
        run(
            &mut e,
            "layer.create",
            json!({"name":"shape","fill":[max,max,max,max]}),
        );
        run(
            &mut e,
            "select.marquee",
            json!({"x":4,"y":4,"width":4,"height":4}),
        );
        run(
            &mut e,
            "layer.mask.add",
            json!({"layer":{"name":"shape"},"source":"from_selection"}),
        );
        run(
            &mut e,
            "layer.set_styles",
            json!({"layer":{"name":"shape"},"styles":{
            "drop_shadow":{"color":[0,0,0,max],"offset_x":4,"offset_y":2,"blur":1},
            "stroke":{"color":[max,0,0,max],"size":1},"outer_glow":{"color":[0,max,0,max],"size":3},
            "color_overlay":{"color":[max,0,0,max],"opacity":0.25},
            "gradient_overlay":{"start":[0,0,0,max],"end":[max,max,max,max],"angle":0,"scale":1,"opacity":0.5}}}),
        );
        let values = match e.document().unwrap().export_pixels().unwrap() {
            PixelBuffer::Rgba8(v) => v.into_iter().map(u16::from).collect::<Vec<_>>(),
            PixelBuffer::Rgba16(v) => v,
        };
        images.push(
            values
                .into_iter()
                .map(|v| f64::from(v) / f64::from(max))
                .collect::<Vec<_>>(),
        );
    }
    assert!(
        images[0]
            .iter()
            .zip(&images[1])
            .all(|(a, b)| (a - b).abs() <= 2.0 / 255.0)
    );
}
