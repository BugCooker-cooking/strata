use super::*;

const VP: Viewport = Viewport {
    width: 800,
    height: 600,
    scale: 1.0,
};
fn near(a: f32, b: f32) {
    assert!((a - b).abs() < 1e-3, "{a} != {b}");
}

#[test]
fn fit_centers_and_leaves_margin() {
    let t = ViewTransform::fit(
        1000,
        2000,
        Viewport {
            width: 1000,
            height: 1000,
            scale: 1.0,
        },
    );
    near(t.zoom, 0.45);
    near(t.center_x, 500.0);
    near(t.center_y, 1000.0);
}

#[test]
fn screen_image_roundtrip() {
    let t = ViewTransform {
        zoom: 2.5,
        center_x: 123.0,
        center_y: 456.0,
    };
    let (x, y) = t.screen_to_image(VP, 37.0, 81.0);
    let (x, y) = t.image_to_screen(VP, x, y);
    near(x, 37.0);
    near(y, 81.0);
}

#[test]
fn zoom_keeps_cursor_fixed() {
    let mut t = ViewTransform {
        zoom: 1.0,
        center_x: 400.0,
        center_y: 300.0,
    };
    t.zoom_at(VP, 100.0, 100.0, 2.0);
    near(t.zoom, 2.0);
    let (x, y) = t.screen_to_image(VP, 100.0, 100.0);
    near(x, 100.0);
    near(y, 100.0);
}

#[test]
fn zoom_is_clamped() {
    let mut t = ViewTransform::default();
    t.zoom_at(VP, 0.0, 0.0, 1e6);
    near(t.zoom, MAX_ZOOM);
    t.zoom_at(VP, 0.0, 0.0, 1e-9);
    near(t.zoom, MIN_ZOOM);
}

#[test]
fn pan_moves_content_with_fingers() {
    let mut t = ViewTransform {
        zoom: 2.0,
        center_x: 400.0,
        center_y: 300.0,
    };
    t.pan_by(20.0, 10.0);
    let (x, y) = t.image_to_screen(VP, 400.0, 300.0);
    near(x, 420.0);
    near(y, 310.0);
}

#[test]
fn pan_is_unbounded() {
    let mut t = ViewTransform {
        zoom: 1.0,
        center_x: 400.0,
        center_y: 300.0,
    };
    t.pan_by(1e6, -1e6);
    near(t.center_x, 400.0 - 1e6);
    near(t.center_y, 300.0 + 1e6);
}

#[test]
fn backing_scale_changes_preserve_logical_size() {
    let t = ViewTransform {
        zoom: 0.5,
        center_x: 10.0,
        center_y: 20.0,
    }
    .rescaled(1.0, 2.0);
    assert_eq!(
        t,
        ViewTransform {
            zoom: 1.0,
            center_x: 10.0,
            center_y: 20.0
        }
    );
}

#[test]
fn ndc_maps_center_to_origin_and_flips_y() {
    let t = ViewTransform {
        zoom: 1.0,
        center_x: 400.0,
        center_y: 300.0,
    };
    let m = t.image_to_ndc(VP, 800, 600);
    let apply = |x, y| {
        (
            m[0][0] * x + m[1][0] * y + m[3][0],
            m[0][1] * x + m[1][1] * y + m[3][1],
        )
    };
    assert_eq!(apply(0.5, 0.5), (0.0, 0.0));
    assert_eq!(apply(1.0, 0.5), (1.0, 0.0));
    assert_eq!(apply(0.5, 0.0), (0.0, 1.0));
}

#[test]
fn texture_limits_are_checked() {
    assert!(check_texture_fits(16384, 100, 16384).is_ok());
    assert!(matches!(
        check_texture_fits(16385, 100, 16384),
        Err(RenderError::ImageTooLarge { .. })
    ));
    assert!(check_texture_fits(0, 1, 16384).is_err());
}

#[test]
fn invalid_gesture_values_do_not_poison_transform() {
    let original = ViewTransform::default();
    let mut t = original;
    for factor in [f32::NAN, f32::INFINITY, 0.0, -1.0] {
        t.zoom_at(VP, 0.0, 0.0, factor);
    }
    t.pan_by(f32::INFINITY, 0.0);
    assert_eq!(t, original);
    assert_eq!(t.rescaled(0.0, 2.0), original);
}

#[test]
fn zero_viewport_fit_is_finite() {
    let t = ViewTransform::fit(
        0,
        0,
        Viewport {
            width: 0,
            height: 0,
            scale: 1.0,
        },
    );
    assert!(t.is_valid());
}
