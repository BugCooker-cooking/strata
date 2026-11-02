use image::{DynamicImage, ImageBuffer, ImageEncoder, ImageFormat, Luma, Rgba};
use std::fs;
use strata_io::{IoError, decode_image, decode_image_with_limit};
use strata_pixels::PixelBuffer;

#[test]
fn decodes_png_pixels_and_sniffs_content_not_extension() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("photo.unknown");
    DynamicImage::ImageRgba8(ImageBuffer::from_pixel(3, 2, Rgba([255, 0, 0, 255])))
        .save_with_format(&path, ImageFormat::Png)
        .unwrap();
    let d = decode_image(&path).unwrap();
    assert_eq!((d.width, d.height), (3, 2));
    assert_eq!(d.pixels, PixelBuffer::Rgba8([255, 0, 0, 255].repeat(6)));
    assert!(d.icc_profile.is_none());
}

#[test]
fn extracts_embedded_icc_profile() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("p3.png");
    let profile = strata_color::display_p3_profile().unwrap().icc().unwrap();
    let mut encoder = image::codecs::png::PngEncoder::new(fs::File::create(&path).unwrap());
    encoder.set_icc_profile(profile.clone()).unwrap();
    encoder
        .write_image(&[1, 2, 3, 255], 1, 1, image::ExtendedColorType::Rgba8)
        .unwrap();
    assert_eq!(decode_image(&path).unwrap().icc_profile, Some(profile));
}

#[test]
fn grayscale_16bit_png_preserves_adjacent_codes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("gray.png");
    let source = [0u16, 32768, 32769, 65535];
    ImageBuffer::<Luma<u16>, _>::from_raw(2, 2, source.to_vec())
        .unwrap()
        .save(&path)
        .unwrap();
    let expected = source.into_iter().flat_map(|v| [v, v, v, 65535]).collect();
    assert_eq!(
        decode_image(&path).unwrap().pixels,
        PixelBuffer::Rgba16(expected)
    );
}

#[test]
fn rgba16_tiff_preserves_all_samples() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rgba.tiff");
    let source = vec![32768, 32769, 50001, 12345, 1, 2, 3, 65535];
    ImageBuffer::<Rgba<u16>, _>::from_raw(2, 1, source.clone())
        .unwrap()
        .save(&path)
        .unwrap();
    assert_eq!(
        decode_image(&path).unwrap().pixels,
        PixelBuffer::Rgba16(source)
    );
}

#[test]
fn transparent_pixels_keep_color_and_alpha() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("alpha.png");
    ImageBuffer::from_pixel(1, 1, Rgba([10u8, 20, 30, 0]))
        .save(&path)
        .unwrap();
    assert_eq!(
        decode_image(&path).unwrap().pixels,
        PixelBuffer::Rgba8(vec![10, 20, 30, 0])
    );
}

#[test]
fn supported_eight_bit_formats_decode() {
    let dir = tempfile::tempdir().unwrap();
    for (name, format) in [
        ("jpeg", ImageFormat::Jpeg),
        ("webp", ImageFormat::WebP),
        ("gif", ImageFormat::Gif),
        ("bmp", ImageFormat::Bmp),
    ] {
        let path = dir.path().join(name);
        DynamicImage::ImageRgb8(ImageBuffer::from_pixel(8, 8, image::Rgb([128u8; 3])))
            .save_with_format(&path, format)
            .unwrap();
        let d = decode_image(&path).unwrap();
        assert_eq!((d.width, d.height), (8, 8), "{name}");
        let PixelBuffer::Rgba8(p) = d.pixels else {
            panic!("wrong format")
        };
        assert!(
            p.chunks_exact(4)
                .all(|p| p[..3].iter().all(|v| v.abs_diff(128) <= 3) && p[3] == 255),
            "{name}"
        );
    }
}

#[test]
fn garbage_bytes_are_unsupported() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("photo.jpg");
    fs::write(&path, b"hello").unwrap();
    assert!(matches!(
        decode_image(&path),
        Err(IoError::Unsupported { .. })
    ));
}

#[test]
fn missing_file_is_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("missing.png");
    assert!(matches!(decode_image(&path), Err(IoError::NotFound(p)) if p == path));
}

#[test]
fn rgba_expansion_is_checked_against_budget() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("gray.png");
    ImageBuffer::from_pixel(8, 8, Luma([128u8]))
        .save(&path)
        .unwrap();
    // Grayscale decode is 64 bytes, expanded RGBA requires 256.
    assert!(matches!(
        decode_image_with_limit(&path, 100),
        Err(IoError::ResourceLimit { .. })
    ));
}

#[test]
fn truncated_known_format_is_decode_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("truncated.png");
    fs::write(&path, b"\x89PNG\r\n\x1a\n").unwrap();
    assert!(matches!(decode_image(&path), Err(IoError::Decode { .. })));
}
