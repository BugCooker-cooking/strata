use super::*;
use strata_pixels::PixelBuffer;

fn converted8(samples: &[u8], profile: Option<&[u8]>) -> Converted {
    to_display_p3(
        &PixelBuffer::Rgba8(samples.to_vec()),
        (samples.len() / 4) as u32,
        1,
        profile,
    )
    .unwrap()
}

fn assert_close8(pixels: &PixelBuffer, expected: &[u8]) {
    let PixelBuffer::Rgba8(actual) = pixels else {
        panic!("changed bit depth")
    };
    assert_eq!(actual.len(), expected.len());
    for (a, b) in actual.iter().zip(expected) {
        assert!(a.abs_diff(*b) <= 1, "{actual:?} != {expected:?}");
    }
}

#[test]
fn srgb_red_maps_to_p3_234_51_35() {
    let c = converted8(&[255, 0, 0, 255], None);
    assert_close8(&c.pixels, &[234, 51, 35, 255]);
    assert_eq!(c.source_profile, "sRGB (assumed)");
    assert!(!c.profile_fallback);
}

#[test]
fn white_black_and_alpha_are_preserved() {
    let c = converted8(&[255, 255, 255, 31, 0, 0, 0, 0], None);
    assert_close8(&c.pixels, &[255, 255, 255, 31, 0, 0, 0, 0]);
    let PixelBuffer::Rgba8(p) = c.pixels else {
        panic!("changed depth")
    };
    assert_eq!((p[3], p[7]), (31, 0));
}

#[test]
fn p3_tagged_input_is_identity() {
    let icc = display_p3_profile().unwrap().icc().unwrap();
    let c = converted8(&[255, 0, 0, 255], Some(&icc));
    assert_close8(&c.pixels, &[255, 0, 0, 255]);
    assert!(c.source_profile.contains("Display P3"));
}

#[test]
fn invalid_icc_falls_back_to_srgb() {
    let c = converted8(&[255, 0, 0, 128], Some(b"not an ICC profile"));
    assert!(c.profile_fallback);
    assert_eq!(c.source_profile, "sRGB (fallback)");
    assert_close8(&c.pixels, &[234, 51, 35, 128]);
}

#[test]
fn invalid_buffer_and_dimensions_fail_without_panicking() {
    assert!(matches!(
        to_display_p3(&PixelBuffer::Rgba16(vec![0; 7]), 1, 2, None),
        Err(ColorError::Pixels(
            strata_pixels::PixelError::LengthMismatch {
                expected: 8,
                actual: 7
            }
        ))
    ));
    assert!(to_display_p3(&PixelBuffer::Rgba8(vec![]), 0, 0, None).is_err());
}

#[test]
fn non_rgb_icc_is_reported() {
    let icc = lcms2::Profile::new_xyz().icc().unwrap();
    assert!(matches!(
        to_display_p3(&PixelBuffer::Rgba8(vec![0; 4]), 1, 1, Some(&icc)),
        Err(ColorError::UnsupportedProfile(_))
    ));
}

#[test]
fn rgba16_identity_preserves_adjacent_values_and_alpha() {
    let icc = display_p3_profile().unwrap().icc().unwrap();
    let samples: Vec<u16> = (0..=u16::MAX).flat_map(|v| [v, v, v, v]).collect();
    let source = PixelBuffer::Rgba16(samples.clone());
    let converted = to_display_p3(&source, 256, 256, Some(&icc)).unwrap();
    let PixelBuffer::Rgba16(actual) = converted.pixels else {
        panic!("precision lost")
    };
    for (i, (a, b)) in actual.iter().zip(&samples).enumerate() {
        assert_eq!(a, b, "identity mismatch at sample {i}");
    }
    assert!(
        source == PixelBuffer::Rgba16(samples),
        "source was modified"
    );
}

#[test]
fn rgba16_transform_matches_float_reference() {
    use lcms2::{Flags, Intent, PixelFormat, Profile, Transform};
    let source_profile = Profile::new_srgb();
    let destination_profile = display_p3_profile().unwrap();
    let values = [0u16, 1, 100, 2650, 32768, 32769, 50001, 65535];
    let mut pixels = Vec::new();
    for r in values {
        for g in values {
            for b in values {
                pixels.extend_from_slice(&[r, g, b, r]);
            }
        }
    }
    let input = PixelBuffer::Rgba16(pixels.clone());
    let actual = to_display_p3(&input, (pixels.len() / 4) as u32, 1, None).unwrap();
    let PixelBuffer::Rgba16(actual) = actual.pixels else {
        panic!("precision lost")
    };
    let reference = Transform::<[f32; 3], [f32; 3]>::new_flags(
        &source_profile,
        PixelFormat::RGB_FLT,
        &destination_profile,
        PixelFormat::RGB_FLT,
        Intent::RelativeColorimetric,
        Flags::NO_OPTIMIZE,
    )
    .unwrap();
    for (original, result) in pixels.chunks_exact(4).zip(actual.chunks_exact(4)) {
        let src = [[
            original[0] as f32 / 65535.0,
            original[1] as f32 / 65535.0,
            original[2] as f32 / 65535.0,
        ]];
        let mut dst = [[0.0; 3]];
        reference.transform_pixels(&src, &mut dst);
        for c in 0..3 {
            let expected = (dst[0][c].clamp(0.0, 1.0) * 65535.0).round() as u16;
            assert!(
                result[c].abs_diff(expected) <= 2,
                "{original:?} -> {result:?}, expected {dst:?}"
            );
        }
        assert_eq!(result[3], original[3]);
    }
    assert_eq!(input, PixelBuffer::Rgba16(pixels));
}

#[test]
fn rgb8_linear_tables_match_unoptimized_float_icc_for_mixed_channels() {
    let mut custom = display_p3_profile().unwrap();
    for (tag, gamma) in [
        (TagSignature::RedTRCTag, 1.8),
        (TagSignature::GreenTRCTag, 2.1),
        (TagSignature::BlueTRCTag, 2.6),
    ] {
        assert!(custom.write_tag(tag, Tag::ToneCurve(&ToneCurve::new(gamma))));
    }
    let source: Vec<u8> = (0..8192u32)
        .flat_map(|i| {
            [
                (i.wrapping_mul(37) >> 1) as u8,
                (i.wrapping_mul(73) >> 3) as u8,
                (i.wrapping_mul(19) >> 5) as u8,
                i as u8,
            ]
        })
        .collect();
    let pixels = PixelBuffer::Rgba8(source.clone());
    for profile in [Profile::new_srgb(), display_p3_profile().unwrap(), custom] {
        let icc = profile.icc().unwrap();
        assert!(linear_rgb8_table(&icc).unwrap().is_some());
        let actual = to_linear_p3(&pixels, 8192, 1, &icc).unwrap();
        assert_eq!(to_linear_p3(&pixels, 8192, 1, &icc).unwrap(), actual);
        // Compare the same serialized ICC. The in-memory profile's colorants
        // have not undergone ICC s15Fixed16 serialization rounding yet.
        let serialized_profile = Profile::new_icc(&icc).unwrap();
        let reference = Transform::<[f32; 4], [f32; 4]>::new_flags(
            &serialized_profile,
            PixelFormat::RGBA_FLT,
            &linear_p3_profile().unwrap(),
            PixelFormat::RGBA_FLT,
            Intent::RelativeColorimetric,
            Flags::NO_OPTIMIZE | Flags::COPY_ALPHA,
        )
        .unwrap();
        let input: Vec<[f32; 4]> = source
            .chunks_exact(4)
            .map(|p| {
                [
                    f32::from(p[0]) / 255.0,
                    f32::from(p[1]) / 255.0,
                    f32::from(p[2]) / 255.0,
                    f32::from(p[3]) / 255.0,
                ]
            })
            .collect();
        let mut expected = vec![[0.0; 4]; input.len()];
        reference.transform_pixels(&input, &mut expected);
        for (actual, expected) in actual.iter().zip(expected) {
            for channel in 0..3 {
                assert!(
                    (actual[channel] - expected[channel]).abs() < 1e-6,
                    "{actual:?} != {expected:?}"
                );
            }
            assert_eq!(actual[3], expected[3]);
        }
        assert_eq!(pixels, PixelBuffer::Rgba8(source.clone()));
    }
}

#[test]
fn rgb8_linear_tables_do_not_accept_profiles_with_a_to_b_pipelines() {
    let mut profile = Profile::new_srgb();
    let pipeline = lcms2::Pipeline::new(3, 3).unwrap();
    assert!(profile.write_tag(TagSignature::AToB0Tag, Tag::Pipeline(&pipeline)));
    let icc = profile.icc().unwrap();
    assert!(
        Profile::new_icc(&icc)
            .unwrap()
            .has_tag(TagSignature::AToB0Tag)
    );
    assert!(linear_rgb8_table(&icc).unwrap().is_none());
}

#[test]
fn linear_profile_identity_preserves_negative_and_extended_float_values() {
    let profile = linear_p3_profile().unwrap();
    let Tag::ToneCurve(curve) = profile.read_tag(TagSignature::RedTRCTag) else {
        panic!("missing linear curve")
    };
    let inverse = curve.reversed();
    let reference = ToneCurve::new(1.0);
    for value in [-10.0f32, -0.1, 0.0, 0.00001, 0.1, 0.5, 1.0, 1.5, 2.0, 10.0] {
        assert_eq!(curve.eval(value), reference.eval(value));
        assert_eq!(inverse.eval(value), reference.eval(value));
    }
}
