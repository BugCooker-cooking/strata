use serde_json::{Value, json};
use strata_core::{Editor, Source};
use strata_pixels::PixelBuffer;
fn run(e: &mut Editor, id: &str, p: Value) -> Value {
    e.execute(e.request(Source::User), id, p, |_| Ok(()))
        .unwrap()
}
#[test]
fn editable_fills_are_sparse_depth_aware_and_gradient_is_linear_light_premultiplied() {
    for depth in [8, 16] {
        let max = if depth == 8 { 255u16 } else { 65535u16 };
        let mut e = Editor::new();
        run(
            &mut e,
            "file.new",
            json!({"width":3,"height":1,"bit_depth":depth}),
        );
        let layer = run(
            &mut e,
            "layer.fill.gradient",
            json!({"name":"gradient","start":[0,0,0,max],"end":[max,max,max,max]}),
        )["layer_id"]
            .clone();
        assert_eq!(
            e.document()
                .unwrap()
                .layers()
                .last()
                .unwrap()
                .pixels()
                .tile_count(),
            0
        );
        let pixel = match e.document().unwrap().export_pixels().unwrap() {
            PixelBuffer::Rgba8(v) => v.into_iter().map(u16::from).collect::<Vec<_>>(),
            PixelBuffer::Rgba16(v) => v,
        };
        let midpoint = f64::from(pixel[4]) / f64::from(max);
        assert!((midpoint - 0.73535698).abs() < 0.005);
        run(
            &mut e,
            "layer.fill.gradient.set",
            json!({"layer":{"id":layer},"start":[max,0,0,0],"end":[0,0,max,max],"angle":0,"scale":1}),
        );
        let pixel = match e.document().unwrap().export_pixels().unwrap() {
            PixelBuffer::Rgba8(v) => v.into_iter().map(u16::from).collect::<Vec<_>>(),
            PixelBuffer::Rgba16(v) => v,
        };
        assert!(pixel[4] <= 3);
        assert!(pixel[6] >= max - 3);
        assert!(pixel[7].abs_diff(max / 2) <= 1);
        let archive = e.document().unwrap().to_archive().unwrap();
        let reopened = strata_core::Document::from_archive(archive.clone()).unwrap();
        assert_eq!(reopened.to_archive().unwrap(), archive);
        assert_eq!(
            reopened.export_pixels().unwrap(),
            e.document().unwrap().export_pixels().unwrap()
        );
    }
}
#[test]
fn hue_saturation_adjustment_masks_its_effect_and_does_not_rewrite_source() {
    let mut e = Editor::new();
    run(
        &mut e,
        "file.new",
        json!({"width":2,"height":1,"bit_depth":16,"background":[40000,10000,5000,65535],"working_space":"display_p3"}),
    );
    let source = e.document().unwrap().layers()[0].pixels().clone();
    run(
        &mut e,
        "select.marquee",
        json!({"x":0,"y":0,"width":1,"height":1}),
    );
    let layer = run(&mut e, "layer.add_hue_saturation", json!({"name":"hue"}))["layer_id"].clone();
    run(
        &mut e,
        "adjust.hue_saturation.set",
        json!({"layer":{"id":layer},"hue":120,"saturation":0,"lightness":0}),
    );
    let PixelBuffer::Rgba16(pixels) = e.document().unwrap().export_pixels().unwrap() else {
        panic!()
    };
    assert!(pixels[1] > pixels[0]);
    assert!(pixels[4].abs_diff(40000) <= 3);
    assert!(pixels[5].abs_diff(10000) <= 3);
    assert!(pixels[6].abs_diff(5000) <= 3);
    assert_eq!(
        e.document().unwrap().layers()[0]
            .pixels()
            .pixel(0, 0)
            .unwrap(),
        source.pixel(0, 0).unwrap()
    );
    let before = e.revision();
    assert!(
        e.execute(
            e.request(Source::User),
            "adjust.hue_saturation.set",
            json!({"layer":{"id":layer},"hue":181,"saturation":0,"lightness":0}),
            |_| Ok(())
        )
        .is_err()
    );
    assert_eq!(e.revision(), before);
}
