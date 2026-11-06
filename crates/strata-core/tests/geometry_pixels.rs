use strata_core::{
    geometry::{GeometryError, Homography},
    geometry_pixels::{
        PixelSource, Sampling, content_bounds, transform_pixels, transformed_bounds,
    },
};
use strata_pixels::{
    CoverageRect, PixelBuffer, PixelFormat, PixelValue, TileCoord, TiledPixels,
    memory::{MemoryLimits, MemoryPool},
};
fn source(depth: u8, w: u32, h: u32, mut value: impl FnMut(u32, u32) -> [u16; 4]) -> TiledPixels {
    let mut values = Vec::new();
    for y in 0..h {
        for x in 0..w {
            values.extend(value(x, y));
        }
    }
    let buffer = if depth == 16 {
        PixelBuffer::Rgba16(values)
    } else {
        PixelBuffer::Rgba8(values.into_iter().map(|v| (v / 257) as u8).collect())
    };
    TiledPixels::from_dense_with_pool(w, h, &buffer, 1024, MemoryPool::global()).unwrap()
}
fn pixel(p: &TiledPixels, x: i64, y: i64) -> [u16; 4] {
    match p.pixel(x, y).unwrap() {
        PixelValue::Rgba16(v) => v,
        PixelValue::Rgba8(v) => v.map(u16::from),
    }
}
#[test]
fn identity_integer_translation_and_flip_preserve_all_codes_and_hidden_color() {
    for depth in [8, 16] {
        for sampling in [Sampling::Nearest, Sampling::Bilinear] {
            let p = source(depth, 17, 9, |x, y| {
                [
                    10000 + x as u16 * 2300,
                    20000 + y as u16 * 3000,
                    50001,
                    if (x + y) % 3 == 0 { 0 } else { 32768 },
                ]
            });
            let b = content_bounds(&p, [-129, 25], &|| false).unwrap().unwrap();
            let icc = strata_color::working_profile("srgb").unwrap();
            for m in [
                [1., 0., 0., 1., 0., 0.],
                [1., 0., 0., 1., 10., -40.],
                [-1., 0., 0., 1., -240., 0.],
                [0., 1., -1., 0., -12., 7.],
            ] {
                let h = Homography::affine(m).unwrap();
                let result = transform_pixels(
                    PixelSource {
                        pixels: &p,
                        offset: [-129, 25],
                        bounds: b,
                        icc: &icc,
                    },
                    &h,
                    sampling,
                    MemoryPool::global(),
                    &|| false,
                )
                .unwrap();
                for y in 0..9 {
                    for x in 0..17 {
                        let [dx, dy] = h.map([x as f64 - 129.0 + 0.5, y as f64 + 25.5]).unwrap();
                        assert_eq!(
                            pixel(&result, dx.floor() as i64, dy.floor() as i64),
                            pixel(&p, x, y)
                        );
                    }
                }
            }
        }
    }
}
#[test]
fn fractional_translation_blends_premultiplied_linear_samples_without_hidden_rgb_bleed() {
    let p = source(16, 2, 1, |x, _| {
        if x == 0 {
            [65535, 0, 65535, 0]
        } else {
            [0, 0, 65535, 65535]
        }
    });
    let b = content_bounds(&p, [0, 0], &|| false).unwrap().unwrap();
    let h = Homography::affine([1., 0., 0., 1., 0.5, 0.]).unwrap();
    let icc = strata_color::working_profile("srgb").unwrap();
    let result = transform_pixels(
        PixelSource {
            pixels: &p,
            offset: [0, 0],
            bounds: b,
            icc: &icc,
        },
        &h,
        Sampling::Bilinear,
        MemoryPool::global(),
        &|| false,
    )
    .unwrap();
    let edge = pixel(&result, 1, 0);
    assert!(edge[0] <= 4 && edge[1] <= 4 && edge[2] >= 65530);
    assert!(edge[3].abs_diff(32768) <= 1);
    let result = transform_pixels(
        PixelSource {
            pixels: &p,
            offset: [0, 0],
            bounds: b,
            icc: &icc,
        },
        &h,
        Sampling::Nearest,
        MemoryPool::global(),
        &|| false,
    )
    .unwrap();
    assert_eq!(pixel(&result, 0, 0), [65535, 0, 65535, 0]);
}
#[test]
fn large_canvas_offsets_and_rebased_storage_preserve_exact_signed_axis_permutations() {
    let p = source(16, 7, 3, |x, y| {
        [
            12001 + x as u16 * 311,
            50001 - y as u16 * 721,
            20003,
            if x % 2 == 0 { 0 } else { 30001 },
        ]
    });
    let offset = [900000000000i64, -800000000000i64];
    let b = content_bounds(&p, offset, &|| false).unwrap().unwrap();
    let icc = strata_color::working_profile("display_p3").unwrap();
    for m in [
        [1., 0., 0., 1., -900000000000., 800000000000.],
        [-1., 0., 0., 1., 900000000020., 800000000000.],
        [0., 1., -1., 0., -800000000000., -900000000000.],
    ] {
        let h = Homography::affine(m).unwrap();
        assert!(h.preserves_lattice());
        let dest = transformed_bounds(b, &h, Sampling::Nearest).unwrap();
        let origin = [dest.x, dest.y];
        let result = strata_core::geometry_pixels::transform_pixels_at(
            PixelSource {
                pixels: &p,
                offset,
                bounds: b,
                icc: &icc,
            },
            &h,
            Sampling::Bilinear,
            origin,
            MemoryPool::global(),
            &|| false,
        )
        .unwrap();
        for y in 0..3 {
            for x in 0..7 {
                let [dx, dy] = h
                    .map([
                        offset[0] as f64 + x as f64 + 0.5,
                        offset[1] as f64 + y as f64 + 0.5,
                    ])
                    .unwrap();
                assert_eq!(
                    pixel(
                        &result,
                        dx.floor() as i64 - origin[0],
                        dy.floor() as i64 - origin[1]
                    ),
                    pixel(&p, x, y)
                );
            }
        }
    }
    let tiny_shear = Homography::affine([1., 1e-12, 0., 1., 0., 0.]).unwrap();
    assert!(!tiny_shear.preserves_lattice());
}
#[test]
fn scaling_and_perspective_match_independent_bilinear_reference_across_tiles() {
    let p = source(16, 520, 7, |x, y| {
        let v = 1000 + (x % 31) as u16 * 1700 + y as u16 * 500;
        [v, v + 1, v + 2, if x % 5 == 0 { 20000 } else { 65535 }]
    });
    let b = content_bounds(&p, [0, 0], &|| false).unwrap().unwrap();
    let icc = strata_color::working_profile("display_p3").unwrap();
    let m = [1.1, 0.03, 5.25, -0.02, 1.3, -2.5, 0.0001, 0.001, 1.0];
    let h = Homography::new(m).unwrap();
    let result = transform_pixels(
        PixelSource {
            pixels: &p,
            offset: [0, 0],
            bounds: b,
            icc: &icc,
        },
        &h,
        Sampling::Bilinear,
        MemoryPool::global(),
        &|| false,
    )
    .unwrap();
    for (x, y) in [(256i64, 1i64), (257, 2), (400, 3)] {
        let [sx, sy] = h.unmap([x as f64 + 0.5, y as f64 + 0.5]).unwrap();
        let x0 = (sx - 0.5).floor() as i64;
        let y0 = (sy - 0.5).floor() as i64;
        let fx = sx - 0.5 - x0 as f64;
        let fy = sy - 0.5 - y0 as f64;
        let mut reference = [0.0; 4];
        for (x, wx) in [(x0, 1.0 - fx), (x0 + 1, fx)] {
            for (y, wy) in [(y0, 1.0 - fy), (y0 + 1, fy)] {
                if x < 0 || y < 0 || x >= 520 || y >= 7 {
                    continue;
                }
                let codes = pixel(&p, x, y);
                let v = strata_color::working_to_linear_p3(
                    &[codes.map(|v| f32::from(v) / 65535.0)],
                    &icc,
                )
                .unwrap()[0]
                    .map(f64::from);
                for c in 0..3 {
                    reference[c] += v[c] * v[3] * wx * wy;
                }
                reference[3] += v[3] * wx * wy;
            }
        }
        for c in 0..3 {
            reference[c] /= reference[3];
        }
        let PixelBuffer::Rgba16(expected) =
            strata_color::from_linear_p3(&[reference.map(|v| v as f32)], PixelFormat::Rgba16, &icc)
                .unwrap()
        else {
            panic!()
        };
        let actual = pixel(&result, x, y);
        assert!(
            (0..4).all(|c| actual[c].abs_diff(expected[c]) <= 4),
            "{x},{y}: {actual:?} != {expected:?}"
        );
    }
    assert!(
        result
            .tile(TileCoord::new(1, 0).unwrap())
            .unwrap()
            .is_some()
    );
}
#[test]
fn bound_limits_horizon_budget_and_cancellation_fail_without_mutating_source() {
    let p = source(16, 17, 9, |_, _| [20000, 30000, 40000, 32768]);
    let b = content_bounds(&p, [0, 0], &|| false).unwrap().unwrap();
    let version = p.tile_version(TileCoord::new(0, 0).unwrap());
    let huge = Homography::affine([10000., 0., 0., 10000., 0., 0.]).unwrap();
    assert!(transformed_bounds(b, &huge, Sampling::Nearest).is_err());
    let horizon = Homography::new([1., 0., 0., 0., 1., 0., 0.2, 0., -1.]).unwrap();
    assert_eq!(
        horizon.quad([[0., 0.], [17., 0.], [17., 9.], [0., 9.]]),
        Err(GeometryError::Horizon)
    );
    let identity = Homography::affine([1., 0., 0., 1., 0., 0.]).unwrap();
    let icc = strata_color::working_profile("srgb").unwrap();
    let pool = MemoryPool::new(MemoryLimits {
        cpu_bytes: 1024 * 1024,
        gpu_bytes: 0,
        disk_bytes: 4 * 1024 * 1024,
    });
    assert!(
        transform_pixels(
            PixelSource {
                pixels: &p,
                offset: [0, 0],
                bounds: b,
                icc: &icc
            },
            &identity,
            Sampling::Nearest,
            pool,
            &|| false
        )
        .is_err()
    );
    assert!(matches!(
        transform_pixels(
            PixelSource {
                pixels: &p,
                offset: [0, 0],
                bounds: b,
                icc: &icc
            },
            &identity,
            Sampling::Nearest,
            MemoryPool::global(),
            &|| true
        ),
        Err(strata_core::DocumentError::Cancelled)
    ));
    assert_eq!(p.tile_version(TileCoord::new(0, 0).unwrap()), version);
    assert!(
        content_bounds(&TiledPixels::new(PixelFormat::Rgba16, 1), [0, 0], &|| false)
            .unwrap()
            .is_none()
    );
    assert!(
        CoverageRect {
            x: i64::MAX,
            y: 0,
            width: 1,
            height: 1
        }
        .validate()
        .is_err()
    );
}
