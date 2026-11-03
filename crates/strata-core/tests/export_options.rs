use serde_json::{Value, json};
use strata_core::{Editor, Source};
use strata_pixels::PixelBuffer;

fn run(e: &mut Editor, id: &str, p: Value) -> Value {
    let r = e.request(Source::User);
    e.execute(r, id, p, |_| Ok(())).unwrap()
}
fn fresh() -> Editor {
    let mut e = Editor::new();
    run(
        &mut e,
        "file.new",
        json!({"width":16,"height":16,"bit_depth":16}),
    );
    run(
        &mut e,
        "layer.create",
        json!({"name":"semi","fill":[65535,0,0,32768]}),
    );
    e
}

#[test]
fn exporting_quantized_copies_does_not_change_document_or_history() {
    for format in ["jpeg", "webp"] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(format!("out.{format}"));
        let mut e = fresh();
        let pixels = e.document().unwrap().export_pixels().unwrap();
        let icc = e.document().unwrap().working_icc().to_vec();
        let before = e.snapshot();
        let token = e.grant_write(&path, false, Source::User).unwrap();
        let mut p = json!({"resource":token,"format":format,"target_space":"display_p3"});
        if format == "jpeg" {
            p["background"] = json!([255, 255, 255]);
            p["quality"] = json!(95);
        }
        let r = e.request(Source::User);
        assert!(e.prepare(r, "file.export", p.clone()).is_err());
        assert!(!path.exists());
        p["allow_quantization"] = json!(true);
        run(&mut e, "file.export", p);
        let output = strata_io::decode_image(&path).unwrap();
        assert_eq!(output.pixels.format().bit_depth(), 8);
        assert!(
            strata_color::resolve_profile(output.icc_profile.as_deref())
                .unwrap()
                .name
                .contains("P3")
        );
        let PixelBuffer::Rgba8(v) = output.pixels else {
            panic!()
        };
        assert!(
            v.chunks_exact(4)
                .all(|p| p[3] == if format == "jpeg" { 255 } else { 128 })
        );
        assert_eq!(e.document().unwrap().export_pixels().unwrap(), pixels);
        assert_eq!(e.document().unwrap().working_icc(), icc);
        assert_eq!(e.snapshot().revision, before.revision);
        assert_eq!(e.snapshot().history.len(), before.history.len());
        assert_eq!(e.snapshot().dirty, before.dirty);
        assert_eq!(e.snapshot().executions.len(), 1);
    }
}

#[test]
fn jpeg_matte_is_linear_light_and_requires_explicit_background() {
    let mut e = fresh();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("white.jpg");
    let token = e.grant_write(&path, false, Source::User).unwrap();
    let r = e.request(Source::User);
    assert!(
        e.prepare(
            r,
            "file.export",
            json!({"resource":token,"format":"jpeg","allow_quantization":true})
        )
        .is_err()
    );
    run(
        &mut e,
        "file.export",
        json!({"resource":token,"format":"jpeg","quality":100,"background":[255,255,255],"allow_quantization":true}),
    );
    let PixelBuffer::Rgba8(v) = strata_io::decode_image(&path).unwrap().pixels else {
        panic!()
    };
    // Half-transparent red on white in linear light is near [255,188,188],
    // not the dark gamma-space [255,128,128]. JPEG adds small rounding error.
    for p in v.chunks_exact(4) {
        assert!(p[0] >= 250);
        assert!((184..=192).contains(&p[1]));
        assert!((184..=192).contains(&p[2]));
    }
}

#[test]
fn invalid_export_parameters_are_atomic_and_lossless_color_conversion_is_exact() {
    let mut e = fresh();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("converted.png");
    let token = e.grant_write(&path, false, Source::User).unwrap();
    let before = serde_json::to_value(e.snapshot()).unwrap();
    for extra in [
        json!({"quality":42}),
        json!({"background":[0,0,0]}),
        json!({"target_space":"bogus"}),
    ] {
        let mut p = json!({"resource":token,"format":"png"});
        p.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        let r = e.request(Source::User);
        assert!(e.prepare(r, "file.export", p).is_err());
        assert_eq!(serde_json::to_value(e.snapshot()).unwrap(), before);
        assert!(!path.exists());
    }
    let source = e.document().unwrap().export_pixels().unwrap();
    run(
        &mut e,
        "file.export",
        json!({"resource":token,"format":"png","target_space":"display_p3"}),
    );
    let out = strata_io::decode_image(&path).unwrap();
    let expected = strata_color::convert_to_profile(
        &source,
        16,
        16,
        e.document().unwrap().working_icc(),
        out.icc_profile.as_ref().unwrap(),
    )
    .unwrap();
    assert_eq!(out.pixels, expected);
    assert_eq!(out.pixels.format().bit_depth(), 16);
}
