#[path = "../examples/support/fixtures.rs"]
mod fixtures;

use strata_io::decode_image;
use strata_pixels::PixelBuffer;

#[test]
fn generated_p3_swatches_have_exact_native_codes_and_icc() {
    let dir = tempfile::tempdir().unwrap();
    fixtures::generate(dir.path()).unwrap();
    let decoded = decode_image(&dir.path().join("p3-swatches.png")).unwrap();
    assert_eq!((decoded.width, decoded.height), (768, 512));
    let PixelBuffer::Rgba8(ref samples) = decoded.pixels else {
        panic!("wrong depth")
    };
    let colors = [
        [255, 0, 0, 255],
        [0, 255, 0, 255],
        [0, 0, 255, 255],
        [255, 255, 255, 255],
        [128, 128, 128, 255],
        [0, 0, 0, 255],
    ];
    for (i, color) in colors.iter().enumerate() {
        let offset = (((i / 3) * 256 + 128) * 768 + (i % 3) * 256 + 128) * 4;
        assert_eq!(&samples[offset..offset + 4], color);
    }
    let display = strata_color::to_display_p3(
        &decoded.pixels,
        decoded.width,
        decoded.height,
        decoded.icc_profile.as_deref(),
    )
    .unwrap();
    assert!(
        display.pixels == decoded.pixels,
        "same-profile fixture must retain native codes"
    );
}

#[test]
fn generated_ramp_contains_every_16bit_value_and_exact_alpha() {
    let dir = tempfile::tempdir().unwrap();
    fixtures::generate(dir.path()).unwrap();
    let decoded = decode_image(&dir.path().join("p3-ramp16.png")).unwrap();
    assert_eq!((decoded.width, decoded.height), (1024, 64));
    let PixelBuffer::Rgba16(ref samples) = decoded.pixels else {
        panic!("precision lost")
    };
    for (value, pixel) in samples.chunks_exact(4).enumerate() {
        assert_eq!(
            pixel,
            &[value as u16, value as u16, value as u16, 65535],
            "sample {value}"
        );
    }
    let alpha = decode_image(&dir.path().join("p3-alpha-edge.png")).unwrap();
    let PixelBuffer::Rgba8(samples) = alpha.pixels else {
        panic!("wrong depth")
    };
    assert_eq!(&samples[..4], &[255, 0, 0, 0]);
    assert_eq!(&samples[256 * 4..257 * 4], &[255, 255, 255, 255]);
}

#[test]
fn fixtures_are_reproducible_and_never_overwrite_existing_files() {
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    fixtures::generate(a.path()).unwrap();
    fixtures::generate(b.path()).unwrap();
    for name in fixtures::FILES {
        assert_eq!(
            std::fs::read(a.path().join(name)).unwrap(),
            std::fs::read(b.path().join(name)).unwrap(),
            "{name}"
        );
    }
    assert!(fixtures::generate(a.path()).is_err());
}
