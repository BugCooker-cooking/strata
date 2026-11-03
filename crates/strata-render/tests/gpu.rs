use strata_pixels::PixelBuffer;
use strata_render::{CANVAS_BG_SRGB, RenderError, Renderer, ViewTransform, Viewport};

const VP: Viewport = Viewport {
    width: 64,
    height: 64,
    scale: 1.0,
};

fn renderer() -> Option<Renderer> {
    match Renderer::new_offscreen(VP) {
        Ok(r) => Some(r),
        Err(RenderError::NoAdapter) if std::env::var_os("STRATA_REQUIRE_GPU").is_none() => {
            eprintln!(
                "SKIPPED GPU TEST: no adapter (use STRATA_REQUIRE_GPU=1 to require hardware)"
            );
            None
        }
        Err(e) => panic!("GPU setup failed: {e}"),
    }
}

#[test]
fn tiled_scene_preserves_source_and_has_no_mip_seam() {
    let Some(mut r) = renderer() else {
        return;
    };
    let icc = strata_color::display_p3_profile().unwrap().icc().unwrap();
    let source = PixelBuffer::Rgba16([32768, 32769, 50001, 65535].repeat(512 * 256));
    let doc = strata_core::DocumentCandidate::from_image(
        512,
        256,
        source.clone(),
        Some(icc),
        "test".into(),
    )
    .unwrap()
    .into_document();
    let prepared = r.prepare_document(&doc, true).unwrap();
    r.commit_document(prepared).unwrap();
    for zoom in [1.0, 0.5, 0.125] {
        r.set_view(ViewTransform {
            zoom,
            center_x: 256.0,
            center_y: 128.0,
        })
        .unwrap();
        r.render().unwrap();
        let frame = r.read_offscreen_rgba().unwrap();
        for x in 4..60 {
            close(pixel(&frame, x, 32), &[128, 128, 195, 255]);
        }
    }
    assert_eq!(doc.export_pixels().unwrap(), source);
}

#[test]
fn tiled_scene_negative_layers_and_foreign_candidate_are_safe() {
    let Some(mut r) = renderer() else {
        return;
    };
    let Some(mut other) = renderer() else {
        return;
    };
    let mut e = strata_core::Editor::new();
    for (command, args) in [
        (
            "file.new",
            serde_json::json!({"width":16,"height":16,"bit_depth":8}),
        ),
        (
            "layer.create",
            serde_json::json!({"name":"white","fill":[255,255,255,255]}),
        ),
        (
            "layer.set_position",
            serde_json::json!({"layer":{"name":"white"},"x":-8,"y":-8}),
        ),
    ] {
        let request = e.request(strata_core::Source::Test);
        e.execute(request, command, args, |_| Ok(())).unwrap();
    }
    let doc = e.document().unwrap();
    let scene = r.prepare_document(doc, true).unwrap();
    r.commit_document(scene).unwrap();
    r.set_view(ViewTransform {
        zoom: 2.0,
        center_x: 0.0,
        center_y: 0.0,
    })
    .unwrap();
    r.render().unwrap();
    let before = r.read_offscreen_rgba().unwrap();
    close(pixel(&before, 32, 32), &[255; 4]);
    let foreign = other.prepare_document(doc, true).unwrap();
    assert!(r.commit_document(foreign).is_err());
    r.render().unwrap();
    assert_eq!(r.read_offscreen_rgba().unwrap(), before);
}

#[test]
fn tiled_four_blends_match_analytic_reference_at_both_source_depths() {
    let Some(mut r) = renderer() else { return };
    let linear = |v: f64| {
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    let encode = |v: f64| {
        if v <= 0.0031308 {
            v * 12.92
        } else {
            1.055 * v.powf(1.0 / 2.4) - 0.055
        }
    };
    // Neutral samples avoid relying on production ICC transforms in the oracle.
    // Include partial alpha, layer opacity and both overlay branches.
    for depth in [8, 16] {
        let maximum: f64 = if depth == 8 { 255.0 } else { 65535.0 };
        for background in [0.25, 0.85] {
            for mode in ["normal", "multiply", "screen", "overlay"] {
                let b = (background * maximum).round() as u16;
                let s = (0.65 * maximum).round() as u16;
                let ab = (0.6 * maximum).round() as u16;
                let asrc = (0.8 * maximum).round() as u16;
                let mut editor = strata_core::Editor::new();
                for (id, params) in [
                    (
                        "file.new",
                        serde_json::json!({"width":16,"height":16,"bit_depth":depth}),
                    ),
                    (
                        "layer.create",
                        serde_json::json!({"name":"back","fill":[b,b,b,ab]}),
                    ),
                    (
                        "layer.create",
                        serde_json::json!({"name":"front","fill":[s,s,s,asrc]}),
                    ),
                    (
                        "layer.set_opacity",
                        serde_json::json!({"layer":{"name":"front"},"opacity":0.7}),
                    ),
                    (
                        "layer.set_blend_mode",
                        serde_json::json!({"layer":{"name":"front"},"blend_mode":mode}),
                    ),
                ] {
                    let request = editor.request(strata_core::Source::Test);
                    editor.execute(request, id, params, |_| Ok(())).unwrap();
                }
                let cb = linear(f64::from(b) / maximum);
                let cs = linear(f64::from(s) / maximum);
                let ba = f64::from(ab) / maximum;
                let sa = f64::from(asrc) / maximum * 0.7;
                let mix = match mode {
                    "normal" => cs,
                    "multiply" => cb * cs,
                    "screen" => 1.0 - (1.0 - cb) * (1.0 - cs),
                    _ if cb <= 0.5 => 2.0 * cb * cs,
                    _ => 1.0 - 2.0 * (1.0 - cb) * (1.0 - cs),
                };
                let premul = (1.0 - sa) * ba * cb + (1.0 - ba) * sa * cs + ba * sa * mix;
                let mut expected = [255u8; 4];
                for channel in 0..3 {
                    expected[channel] = (encode(
                        premul
                            + (1.0 - ba)
                                * (1.0 - sa)
                                * linear(f64::from(CANVAS_BG_SRGB[channel]) / 255.0),
                    ) * 255.0)
                        .round() as u8;
                }
                let scene = r
                    .prepare_document(editor.document().unwrap(), true)
                    .unwrap();
                r.commit_document(scene).unwrap();
                r.render().unwrap();
                close(pixel(&r.read_offscreen_rgba().unwrap(), 32, 32), &expected);
            }
        }
    }
}

fn put(r: &mut Renderer, pixels: PixelBuffer, w: u32, h: u32) {
    let prepared = r.prepare_image(w, h, &pixels).unwrap();
    r.commit_image(prepared).unwrap();
    r.set_view(ViewTransform {
        zoom: 16.0,
        center_x: w as f32 / 2.0,
        center_y: h as f32 / 2.0,
    })
    .unwrap();
}

fn pixel(bytes: &[u8], x: usize, y: usize) -> &[u8] {
    &bytes[(y * 64 + x) * 4..(y * 64 + x + 1) * 4]
}
fn close(actual: &[u8], expected: &[u8]) {
    assert_eq!(actual.len(), expected.len());
    for (a, b) in actual.iter().zip(expected) {
        assert!(a.abs_diff(*b) <= 2, "{actual:?} != {expected:?}");
    }
}

#[test]
fn renders_image_and_background() {
    let Some(mut r) = renderer() else {
        return;
    };
    put(
        &mut r,
        PixelBuffer::Rgba8([200, 100, 50, 255].repeat(4)),
        2,
        2,
    );
    assert!(!r.render().unwrap().skipped);
    let frame = r.read_offscreen_rgba().unwrap();
    close(pixel(&frame, 32, 32), &[200, 100, 50, 255]);
    close(pixel(&frame, 0, 0), &[20, 20, 21, 255]);
}

#[test]
fn transparent_image_shows_background() {
    let Some(mut r) = renderer() else {
        return;
    };
    put(&mut r, PixelBuffer::Rgba8([255, 0, 0, 0].repeat(4)), 2, 2);
    r.render().unwrap();
    close(
        pixel(&r.read_offscreen_rgba().unwrap(), 32, 32),
        &[20, 20, 21, 255],
    );
}

#[test]
fn changing_workspace_background_preserves_image_samples() {
    let Some(mut r) = renderer() else {
        return;
    };
    put(
        &mut r,
        PixelBuffer::Rgba8([200, 100, 50, 255].repeat(4)),
        2,
        2,
    );
    r.set_background([214, 214, 216]);
    r.render().unwrap();
    let frame = r.read_offscreen_rgba().unwrap();
    close(pixel(&frame, 0, 0), &[214, 214, 216, 255]);
    close(pixel(&frame, 32, 32), &[200, 100, 50, 255]);
    r.set_background(CANVAS_BG_SRGB);
    r.render().unwrap();
    let frame = r.read_offscreen_rgba().unwrap();
    close(pixel(&frame, 0, 0), &[20, 20, 21, 255]);
    close(pixel(&frame, 32, 32), &[200, 100, 50, 255]);
}

#[test]
fn empty_scene_and_zero_size_are_safe() {
    let Some(mut r) = renderer() else {
        return;
    };
    r.render().unwrap();
    close(
        pixel(&r.read_offscreen_rgba().unwrap(), 10, 10),
        &[CANVAS_BG_SRGB[0], CANVAS_BG_SRGB[1], CANVAS_BG_SRGB[2], 255],
    );
    r.resize(Viewport { width: 0, ..VP }).unwrap();
    assert!(r.render().unwrap().skipped);
    r.resize(VP).unwrap();
    assert!(!r.render().unwrap().skipped);
}

#[test]
fn failed_prepare_preserves_image_and_view() {
    let Some(mut r) = renderer() else {
        return;
    };
    put(
        &mut r,
        PixelBuffer::Rgba8([200, 100, 50, 255].repeat(4)),
        2,
        2,
    );
    let view = r.view();
    r.render().unwrap();
    let before = r.read_offscreen_rgba().unwrap();
    assert!(matches!(
        r.prepare_image(
            r.max_texture_dimension() + 1,
            1,
            &PixelBuffer::Rgba8(vec![])
        ),
        Err(RenderError::ImageTooLarge { .. })
    ));
    assert!(
        r.prepare_image(2, 2, &PixelBuffer::Rgba8(vec![0; 3]))
            .is_err()
    );
    assert_eq!(r.view(), view);
    r.render().unwrap();
    assert_eq!(r.read_offscreen_rgba().unwrap(), before);
}

#[test]
fn new_scale_rescales_view() {
    let Some(mut r) = renderer() else {
        return;
    };
    r.set_view(ViewTransform {
        zoom: 1.0,
        center_x: 5.0,
        center_y: 5.0,
    })
    .unwrap();
    r.resize(Viewport {
        width: 128,
        height: 128,
        scale: 2.0,
    })
    .unwrap();
    assert_eq!(
        r.view(),
        ViewTransform {
            zoom: 2.0,
            center_x: 5.0,
            center_y: 5.0
        }
    );
}

#[test]
fn rgba16_preview_does_not_mutate_source() {
    let Some(mut r) = renderer() else {
        return;
    };
    let source = PixelBuffer::Rgba16([32768, 32769, 50001, 65535].repeat(4));
    let before = source.clone();
    let prepared = r.prepare_image(2, 2, &source).unwrap();
    r.commit_image(prepared).unwrap();
    r.render().unwrap();
    assert_eq!(source, before);
    close(
        pixel(&r.read_offscreen_rgba().unwrap(), 32, 32),
        &[128, 128, 195, 255],
    );
}

#[test]
fn transparent_color_does_not_bleed_when_filtered() {
    let Some(mut r) = renderer() else {
        return;
    };
    put(
        &mut r,
        PixelBuffer::Rgba8(vec![255, 0, 0, 0, 255, 255, 255, 255]),
        2,
        1,
    );
    r.render().unwrap();
    let frame = r.read_offscreen_rgba().unwrap();
    let p = pixel(&frame, 32, 32);
    assert!(p[0].abs_diff(p[1]) <= 2, "red fringe: {p:?}");
    assert!(p[0] > 100 && p[0] < 240, "expected blended edge: {p:?}");
}

#[test]
fn prepared_image_cannot_cross_renderers() {
    let Some(a) = renderer() else {
        return;
    };
    let Some(mut b) = renderer() else {
        return;
    };
    let candidate = a
        .prepare_image(1, 1, &PixelBuffer::Rgba8(vec![255; 4]))
        .unwrap();
    assert!(b.commit_image(candidate).is_err());
}
