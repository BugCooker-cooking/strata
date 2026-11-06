use strata_core::{
    geometry::Homography, geometry_masks::transform_mask, geometry_pixels::Sampling,
};
use strata_pixels::{CoverageRect, MaskLayout, TiledMask, memory::MemoryPool};
#[test]
fn linked_offset_mask_identity_flip_and_integer_translation_keep_exact_u16() {
    let pool = MemoryPool::global();
    let mut mask = TiledMask::new(
        MaskLayout {
            bounds: CoverageRect {
                x: -2,
                y: 3,
                width: 9,
                height: 7,
            },
            default_coverage: 32769,
            inverted: false,
        },
        16,
        pool.clone(),
    )
    .unwrap();
    let mut values = mask.region_tile(0, 0).unwrap();
    values[5 * 256 + 2] = 50001;
    mask.insert_tile(strata_pixels::TileCoord::new(0, 0).unwrap(), values)
        .unwrap();
    for sampling in [Sampling::Nearest, Sampling::Bilinear] {
        for m in [
            [1., 0., 0., 1., 0., 0.],
            [1., 0., 0., 1., -129., 20.],
            [-1., 0., 0., 1., 32., 0.],
        ] {
            let h = Homography::affine(m).unwrap();
            let result =
                transform_mask(&mask, [7, -8], &h, sampling, pool.clone(), &|| false).unwrap();
            for y in 3..10 {
                for x in -2..7 {
                    let [dx, dy] = h.map([x as f64 + 7.5, y as f64 - 7.5]).unwrap();
                    assert_eq!(
                        result.sample(dx.floor() as i64, dy.floor() as i64).unwrap(),
                        mask.sample(x, y).unwrap()
                    );
                }
            }
        }
    }
}
#[test]
fn bilinear_mask_edge_preserves_half_coverage_without_8bit_quantization() {
    let pool = MemoryPool::global();
    let mask = TiledMask::new(
        MaskLayout {
            bounds: CoverageRect {
                x: 0,
                y: 0,
                width: 2,
                height: 1,
            },
            default_coverage: 50001,
            inverted: false,
        },
        16,
        pool.clone(),
    )
    .unwrap();
    let h = Homography::affine([1., 0., 0., 1., 0.5, 0.]).unwrap();
    let result = transform_mask(&mask, [0, 0], &h, Sampling::Bilinear, pool, &|| false).unwrap();
    assert_eq!(result.sample(0, 0).unwrap(), 25001);
    assert_eq!(result.sample(1, 0).unwrap(), 50001);
    assert_eq!(result.sample(2, 0).unwrap(), 25001);
}
#[test]
fn rebound_finite_defaults_and_inversion_keep_exact_codes_and_expand_with_zero() {
    use strata_core::geometry_masks::rebind;
    let pool = MemoryPool::global();
    for inverted in [false, true] {
        let bounds = CoverageRect {
            x: -2,
            y: 3,
            width: 9,
            height: 7,
        };
        let mut source = TiledMask::new(
            MaskLayout {
                bounds,
                default_coverage: 32769,
                inverted,
            },
            16,
            pool.clone(),
        )
        .unwrap();
        let mut tile = source.region_tile(0, 0).unwrap();
        tile[5 * 256 + 2] = 50001;
        source
            .insert_tile(strata_pixels::TileCoord::new(0, 0).unwrap(), tile)
            .unwrap();
        let expanded = CoverageRect {
            x: -5,
            y: 1,
            width: 15,
            height: 12,
        };
        let output = rebind(&source, expanded, pool.clone(), &|| false).unwrap();
        for y in 1..13 {
            for x in -5..10 {
                assert_eq!(output.sample(x, y).unwrap(), source.sample(x, y).unwrap());
            }
        }
        let calls = std::cell::Cell::new(0);
        assert!(matches!(
            rebind(&source, expanded, pool.clone(), &|| {
                calls.set(calls.get() + 1);
                calls.get() > 5
            }),
            Err(strata_core::DocumentError::Cancelled)
        ));
        assert_eq!(source.sample(2, 5).unwrap(), 50001);
    }
}
#[test]
fn coverage_bounds_scan_is_cancellable_and_handles_zero_inverted_baseline() {
    use strata_core::geometry_masks::coverage_bounds;
    let pool = MemoryPool::global();
    let layout = MaskLayout {
        bounds: CoverageRect {
            x: 0,
            y: 0,
            width: 300,
            height: 300,
        },
        default_coverage: 65535,
        inverted: true,
    };
    let mut source = TiledMask::new(layout, 16, pool.clone()).unwrap();
    let mut values = vec![0; 256 * 256];
    values[12 * 256 + 9] = 50001;
    source
        .insert_tile(strata_pixels::TileCoord::new(0, 0).unwrap(), values)
        .unwrap();
    assert_eq!(
        coverage_bounds(&source, &|| false).unwrap(),
        Some(CoverageRect {
            x: 9,
            y: 12,
            width: 1,
            height: 1
        })
    );
    let checks = std::cell::Cell::new(0);
    assert!(matches!(
        coverage_bounds(&source, &|| {
            checks.set(checks.get() + 1);
            checks.get() > 10
        }),
        Err(strata_core::DocumentError::Cancelled)
    ));
    let mut full = source.clone();
    full.invert();
    assert_eq!(
        coverage_bounds(&full, &|| false).unwrap(),
        Some(layout.bounds)
    );
}
