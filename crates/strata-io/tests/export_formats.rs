use strata_io::{
    decode_image,
    export::{ExportFormat, export_image},
};
use strata_pixels::PixelBuffer;

#[test]
fn lossy_webp_quality_retains_alpha_icc_and_resolution_and_is_failure_safe() {
    use strata_io::export::{ExportOptions, export_with_options};
    let dir = tempfile::tempdir().unwrap();
    let icc = strata_color::working_profile("display_p3").unwrap();
    let source: Vec<u8> = (0..128 * 64)
        .flat_map(|i| {
            [
                (i * 37) as u8,
                (i * 71 + i / 128 * 3) as u8,
                (i * 13) as u8,
                (i * 17) as u8,
            ]
        })
        .collect();
    let pixels = PixelBuffer::Rgba8(source.clone());
    let mut sizes = Vec::new();
    for quality in [20, 95] {
        let path = dir.path().join(format!("q{quality}.webp"));
        export_with_options(
            &path,
            ExportOptions {
                format: ExportFormat::WebPLossy { quality },
                resolution_dpi: 300,
            },
            128,
            64,
            &pixels,
            &icc,
            false,
        )
        .unwrap();
        let decoded = strata_io::decode_image(&path).unwrap();
        assert_eq!(decoded.resolution_dpi, 300);
        assert_eq!(decoded.icc_profile.as_deref(), Some(icc.as_slice()));
        let PixelBuffer::Rgba8(samples) = decoded.pixels else {
            panic!()
        };
        assert_eq!(samples.len(), source.len());
        assert!(
            samples
                .chunks_exact(4)
                .zip(source.chunks_exact(4))
                .all(|(a, b)| a[3] == b[3])
        );
        sizes.push(std::fs::metadata(path).unwrap().len());
    }
    assert!(sizes[1] > sizes[0]);
    let path = dir.path().join("keep.webp");
    std::fs::write(&path, b"old").unwrap();
    for quality in [0, 101] {
        assert!(
            export_with_options(
                &path,
                ExportOptions {
                    format: ExportFormat::WebPLossy { quality },
                    resolution_dpi: 300
                },
                128,
                64,
                &pixels,
                &icc,
                true
            )
            .is_err()
        );
        assert_eq!(std::fs::read(&path).unwrap(), b"old");
    }
}

#[test]
fn all_raster_formats_roundtrip_document_print_resolution() {
    use strata_io::export::{ExportOptions, export_with_options};
    let dir = tempfile::tempdir().unwrap();
    let icc = strata_color::working_profile("srgb").unwrap();
    for format in [
        ExportFormat::Png,
        ExportFormat::Tiff,
        ExportFormat::Jpeg { quality: 90 },
        ExportFormat::WebP,
        ExportFormat::WebPLossy { quality: 80 },
    ] {
        let path = dir.path().join(format!("{format:?}"));
        export_with_options(
            &path,
            ExportOptions {
                format,
                resolution_dpi: 300,
            },
            16,
            16,
            &PixelBuffer::Rgba8([31, 128, 225, 255].repeat(16 * 16)),
            &icc,
            false,
        )
        .unwrap();
        assert_eq!(strata_io::decode_image(&path).unwrap().resolution_dpi, 300);
    }
}

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

#[test]
fn psd_destination_is_rechecked_after_the_temporary_file_is_verified() {
    let dir = tempfile::tempdir().unwrap();
    let dst = dir.path().join("destination.psd");
    std::fs::write(&dst, b"original").unwrap();
    let bytes = include_bytes!("../../strata-core/tests/fixtures/psd/group-mask.psd");
    let result = strata_io::export::publish_psd(&dst, bytes, true, || {
        std::fs::write(&dst, b"external update")?;
        Err(strata_io::project::StorageError::Invalid(
            "destination changed".into(),
        ))
    });
    assert!(result.is_err());
    assert_eq!(std::fs::read(&dst).unwrap(), b"external update");
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}
