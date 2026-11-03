use strata_io::{
    decode_image,
    export::{ExportFormat, export_image},
};
use strata_pixels::PixelBuffer;

#[test]
fn webp_is_sample_exact_including_transparent_rgb_and_embedded_icc() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("lossless.webp");
    let pixels = PixelBuffer::Rgba8(
        (0..256)
            .flat_map(|v| [v as u8, 127, 201, v as u8])
            .collect(),
    );
    let icc = strata_color::working_profile("display_p3").unwrap();
    export_image(&path, ExportFormat::WebP, 16, 16, &pixels, &icc, false).unwrap();
    let image = decode_image(&path).unwrap();
    assert_eq!(image.pixels, pixels);
    assert_eq!(image.icc_profile.as_deref(), Some(icc.as_slice()));
    assert_eq!((image.width, image.height), (16, 16));
}

#[test]
fn jpeg_quality_changes_encoding_and_keeps_color_metadata() {
    let dir = tempfile::tempdir().unwrap();
    let pixels = PixelBuffer::Rgba8(
        (0..64 * 64)
            .flat_map(|v| {
                [
                    (v % 64 * 4) as u8,
                    (v / 64 * 4) as u8,
                    ((v * 17) % 256) as u8,
                    255,
                ]
            })
            .collect(),
    );
    let icc = strata_color::working_profile("srgb").unwrap();
    let mut sizes = vec![];
    for quality in [20, 95] {
        let path = dir.path().join(format!("quality-{quality}.jpg"));
        export_image(
            &path,
            ExportFormat::Jpeg { quality },
            64,
            64,
            &pixels,
            &icc,
            false,
        )
        .unwrap();
        let image = decode_image(&path).unwrap();
        assert_eq!((image.width, image.height), (64, 64));
        assert_eq!(image.icc_profile.as_deref(), Some(icc.as_slice()));
        assert!(
            matches!(image.pixels,PixelBuffer::Rgba8(v) if v.chunks_exact(4).all(|p|p[3]==255))
        );
        sizes.push(std::fs::metadata(path).unwrap().len());
    }
    assert!(sizes[1] > sizes[0]);
}

#[test]
fn invalid_depth_alpha_and_quality_never_replace_target() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("existing.jpg");
    let icc = strata_color::working_profile("srgb").unwrap();
    std::fs::write(&path, b"old target").unwrap();
    for (format, pixels) in [
        (
            ExportFormat::Jpeg { quality: 0 },
            PixelBuffer::Rgba8(vec![1, 2, 3, 255]),
        ),
        (
            ExportFormat::Jpeg { quality: 90 },
            PixelBuffer::Rgba8(vec![1, 2, 3, 128]),
        ),
        (
            ExportFormat::Jpeg { quality: 90 },
            PixelBuffer::Rgba16(vec![1, 2, 3, 65535]),
        ),
        (
            ExportFormat::WebP,
            PixelBuffer::Rgba16(vec![1, 2, 3, 65535]),
        ),
    ] {
        assert!(export_image(&path, format, 1, 1, &pixels, &icc, true).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"old target");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}
