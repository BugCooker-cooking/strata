use serde_json::json;
use strata_core::{
    BlendMode, DocumentCandidate, Editor, Source,
    composite::{Compositor, blend},
};
use strata_pixels::{PixelBuffer, TileCoord};
#[test]
fn blend_modes_match_independent_opaque_reference() {
    // Opaque inputs reduce Porter-Duff to each blend equation exactly.
    let b = [0.25, 0.75, 0.5, 1.0];
    let s = [0.8, 0.2, 0.6, 1.0];
    for (mode, expected) in [
        (BlendMode::Normal, [0.8, 0.2, 0.6]),
        (BlendMode::Multiply, [0.2, 0.15, 0.3]),
        (BlendMode::Screen, [0.85, 0.8, 0.8]),
        (BlendMode::Overlay, [0.4, 0.6, 0.6]),
    ] {
        let out = blend(b, s, mode, 1.0);
        for i in 0..3 {
            assert!((out[i] - expected[i]).abs() < 1e-12);
        }
        assert_eq!(blend(b, s, mode, 0.0), b);
    }
    let out = blend(
        [1.0, 0.0, 0.0, 0.5],
        [0.0, 0.0, 1.0, 0.5],
        BlendMode::Normal,
        1.0,
    );
    assert!((out[0] - 1.0 / 3.0).abs() < 1e-12);
    assert!((out[2] - 2.0 / 3.0).abs() < 1e-12);
    assert_eq!(out[3], 0.75);
}
#[test]
fn identity_export_preserves_hidden_16bit_rgb() {
    let source = PixelBuffer::Rgba16(vec![32768, 32769, 50001, 0]);
    let doc = DocumentCandidate::from_image(1, 1, source.clone(), None, "test".into())
        .unwrap()
        .into_document();
    assert_eq!(doc.export_pixels().unwrap(), source);
}

#[test]
fn edited_exports_match_independent_f64_srgb_reference() {
    // Neutral RGB is invariant under the sRGB/P3 primary matrices. This oracle
    // uses analytic transfer functions and the W3C alpha equation, without any
    // production compositor or LCMS call. Exercise both overlay branches.
    let linear = |v: f64| {
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    let encoded = |v: f64| {
        if v <= 0.0031308 {
            v * 12.92
        } else {
            1.055 * v.powf(1.0 / 2.4) - 0.055
        }
    };
    for depth in [8, 16] {
        let max = if depth == 8 { 255u16 } else { 65535u16 };
        for background in [0.25, 0.85] {
            for mode in ["normal", "multiply", "screen", "overlay"] {
                let b = (background * f64::from(max)).round() as u16;
                let s = (0.65 * f64::from(max)).round() as u16;
                let ab = (0.6 * f64::from(max)).round() as u16;
                let source_alpha = (0.8 * f64::from(max)).round() as u16;
                let mut editor = Editor::new();
                for (id, params) in [
                    ("file.new", json!({"width":1,"height":1,"bit_depth":depth})),
                    ("layer.create", json!({"name":"back","fill":[b,b,b,ab]})),
                    (
                        "layer.create",
                        json!({"name":"front","fill":[s,s,s,source_alpha]}),
                    ),
                    (
                        "layer.set_opacity",
                        json!({"layer":{"name":"front"},"opacity":0.7}),
                    ),
                    (
                        "layer.set_blend_mode",
                        json!({"layer":{"name":"front"},"blend_mode":mode}),
                    ),
                ] {
                    let request = editor.request(Source::Test);
                    editor.execute(request, id, params, |_| Ok(())).unwrap();
                }
                let cb = linear(f64::from(b) / f64::from(max));
                let cs = linear(f64::from(s) / f64::from(max));
                let back_alpha = f64::from(ab) / f64::from(max);
                let front_alpha = f64::from(source_alpha) / f64::from(max) * 0.7;
                let mix = match mode {
                    "normal" => cs,
                    "multiply" => cb * cs,
                    "screen" => 1.0 - (1.0 - cb) * (1.0 - cs),
                    _ if cb <= 0.5 => 2.0 * cb * cs,
                    _ => 1.0 - 2.0 * (1.0 - cb) * (1.0 - cs),
                };
                let alpha = 1.0 - (1.0 - back_alpha) * (1.0 - front_alpha);
                let rgb = ((1.0 - front_alpha) * back_alpha * cb
                    + (1.0 - back_alpha) * front_alpha * cs
                    + back_alpha * front_alpha * mix)
                    / alpha;
                let expected = [
                    (encoded(rgb) * f64::from(max)).round() as i32,
                    (alpha * f64::from(max)).round() as i32,
                ];
                let actual: Vec<i32> = match editor.document().unwrap().export_pixels().unwrap() {
                    PixelBuffer::Rgba8(v) => v.into_iter().map(i32::from).collect(),
                    PixelBuffer::Rgba16(v) => v.into_iter().map(i32::from).collect(),
                };
                for (channel, value) in actual.into_iter().enumerate() {
                    let target = expected[usize::from(channel == 3)];
                    let tolerance = if depth == 8 { 1 } else { 3 };
                    assert!(
                        (value - target).abs() <= tolerance,
                        "{depth}bit {mode} background={background} channel={channel}: {value} vs {target}"
                    );
                }
            }
        }
    }
}
#[test]
fn cache_reuses_unchanged_tiles_and_invalidates_visible_edits() {
    let mut e = Editor::new();
    let r = e.request(Source::User);
    e.execute(
        r,
        "file.new",
        json!({"width":256,"height":256,"bit_depth":8}),
        |_| Ok(()),
    )
    .unwrap();
    let r = e.request(Source::User);
    e.execute(
        r,
        "layer.create",
        json!({"name":"solid","fill":[255,0,0,255]}),
        |_| Ok(()),
    )
    .unwrap();
    let coord = TileCoord::new(0, 0).unwrap();
    let outside = TileCoord::new(-1, 0).unwrap();
    let mut c = Compositor::new(4);
    let first = c.tile(e.document().unwrap(), coord).unwrap();
    let blank = c.tile(e.document().unwrap(), outside).unwrap();
    assert!(std::sync::Arc::ptr_eq(
        &first,
        &c.tile(e.document().unwrap(), coord).unwrap()
    ));
    assert_eq!(c.recomputed, 2);
    let r = e.request(Source::User);
    e.execute(
        r,
        "layer.set_opacity",
        json!({"layer":{"name":"solid"},"opacity":0.5}),
        |_| Ok(()),
    )
    .unwrap();
    assert!(!std::sync::Arc::ptr_eq(
        &first,
        &c.tile(e.document().unwrap(), coord).unwrap()
    ));
    assert!(std::sync::Arc::ptr_eq(
        &blank,
        &c.tile(e.document().unwrap(), outside).unwrap()
    ));
    assert!((c.tile(e.document().unwrap(), coord).unwrap()[0][3] - 0.5).abs() < 1e-6);
}
