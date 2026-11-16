use serde_json::{Value, json};
use strata_core::{Document, Editor, Source};
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

#[test]
fn scaled_gradient_uses_original_gradient_space_and_remains_editable_after_reopen() {
    let mut e = Editor::new();
    run(
        &mut e,
        "file.new",
        json!({"width":8,"height":6,"bit_depth":16}),
    );
    let layer = run(
        &mut e,
        "layer.fill.gradient",
        json!({"name":"Ramp","start":[0,0,0,65535],"end":[65535,65535,65535,65535],"angle":0}),
    )["layer_id"]
        .clone();
    run(
        &mut e,
        "image.resize",
        json!({"width":16,"height":3,"interpolation":"area"}),
    );
    let document = e.document().unwrap();
    let saved = document.to_archive().unwrap();
    assert_eq!(saved.metadata["version"], 10);
    let reopened = Document::from_archive(saved.clone()).unwrap();
    assert_eq!(reopened.to_archive().unwrap(), saved);
    let PixelBuffer::Rgba16(codes) = reopened.export_pixels().unwrap() else {
        panic!()
    };
    for (x, pixel) in codes.chunks_exact(4).take(16).enumerate() {
        let t = (x as f64 + 0.5) / 16.0;
        let expected = ((1.055 * t.powf(1.0 / 2.4) - 0.055) * 65535.0).round() as u16;
        assert!(
            pixel[0].abs_diff(expected) < 32,
            "{x}: {} vs {expected}",
            pixel[0]
        );
        assert_eq!(pixel[3], 65535);
    }
    let mut forged = saved;
    forged.metadata["version"] = json!(9);
    assert!(Document::from_archive(forged).is_err());
    run(
        &mut e,
        "layer.fill.gradient.set",
        json!({"layer":{"id":layer},"start":[10000,10000,10000,65535],"end":[10000,10000,10000,65535],"angle":0,"scale":1}),
    );
    let document = e.document().unwrap();
    assert_eq!(document.layers()[1].pixels().tile_count(), 0);
    let PixelBuffer::Rgba16(codes) = document.export_pixels().unwrap() else {
        panic!()
    };
    assert!(
        codes
            .chunks_exact(4)
            .all(|px| px[0].abs_diff(10000) <= 3 && px[3] == 65535)
    );
}

#[test]
fn projective_fill_does_not_restore_deleted_coverage_after_reframe_or_orientation() {
    let mut e = Editor::new();
    run(
        &mut e,
        "file.new",
        json!({"width":16,"height":16,"bit_depth":16}),
    );
    let layer = run(
        &mut e,
        "layer.fill.solid",
        json!({"name":"Square","color":[20001,30001,50001,65535]}),
    )["layer_id"]
        .clone();
    run(
        &mut e,
        "transform.apply",
        json!({
            "layer":{"id":layer},"scope":"whole_layer","geometry":{"type":"corners",
                "source":[[0,0],[16,0],[16,16],[0,16]],"destination":[[8,0],[16,8],[8,16],[0,8]]}
        }),
    );
    let original = e.document().unwrap().to_archive().unwrap();
    run(
        &mut e,
        "image.crop",
        json!({"rect":{"x":4,"y":0,"width":8,"height":8},"pixels":"delete"}),
    );
    run(
        &mut e,
        "image.crop",
        json!({"rect":{"x":0,"y":0,"width":16,"height":16},"pixels":"preserve"}),
    );
    let PixelBuffer::Rgba16(codes) = e.document().unwrap().export_pixels().unwrap() else {
        panic!()
    };
    let alpha = |x: usize, y: usize| codes[(y * 16 + x) * 4 + 3];
    assert_eq!(
        [alpha(8, 4), alpha(4, 0), alpha(13, 7), alpha(8, 10)],
        [65535, 0, 0, 0]
    );
    run(&mut e, "image.orient", json!({"operation":"rotate_90_cw"}));
    let PixelBuffer::Rgba16(rotated) = e.document().unwrap().export_pixels().unwrap() else {
        panic!()
    };
    for y in 0..16 {
        for x in 0..16 {
            assert_eq!(rotated[(x * 16 + (15 - y)) * 4 + 3], alpha(x, y));
        }
    }
    for _ in 0..3 {
        run(&mut e, "edit.undo", json!({}));
    }
    assert_eq!(e.document().unwrap().to_archive().unwrap(), original);
}
