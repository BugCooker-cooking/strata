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
