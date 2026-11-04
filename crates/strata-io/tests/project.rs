use serde_json::json;
use strata_io::export::{ExportFormat, export_image};
use strata_io::project::*;
use strata_pixels::PixelBuffer;

fn archive(value: u16) -> ProjectArchive {
    ProjectArchive {
        metadata: json!({"id":"fixture","name":"test"}),
        tiles: vec![ProjectTile {
            channels: 4,
            layer: "layer".into(),
            x: -1,
            y: 2,
            bit_depth: 16,
            bytes: [value, 32768, 32769, 0]
                .repeat(256 * 256)
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect(),
        }],
    }
}
#[test]
fn sqlite_roundtrip_incremental_and_no_sidecars() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.strata");
    let first = archive(1);
    write_project(&path, &first, false, false).unwrap();
    assert_eq!(read_project(&path).unwrap(), first);
    assert!(write_project(&path, &first, false, false).is_err());
    let second = archive(2);
    write_project(&path, &second, true, true).unwrap();
    assert_eq!(read_project(&path).unwrap(), second);
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[test]
fn mono_coverage_planes_preserve_adjacent_16_bit_codes_and_validate_channels() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("mask.strata");
    let mut source = archive(1);
    let bytes: Vec<u8> = (0..=u16::MAX).flat_map(u16::to_le_bytes).collect();
    source.tiles.push(ProjectTile {
        layer: "mask:fixture-layer".into(),
        x: 0,
        y: -1,
        bit_depth: 16,
        channels: 1,
        bytes: bytes.clone(),
    });
    write_project(&path, &source, false, false).unwrap();
    assert_eq!(read_project(&path).unwrap(), source);
    let connection = rusqlite::Connection::open(&path).unwrap();
    assert_eq!(
        connection
            .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
    let (channels, size): (i64, i64) = connection
        .query_row(
            "SELECT channels,length(data) FROM tiles WHERE layer='mask:fixture-layer'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!((channels, size), (1, 256 * 256 * 2));
    drop(connection);
    let mut invalid = source.clone();
    invalid.tiles[1].channels = 2;
    assert!(write_project(&path, &invalid, true, true).is_err());
    assert_eq!(read_project(&path).unwrap(), source);
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute(
            "UPDATE tiles SET channels=4 WHERE layer='mask:fixture-layer'",
            [],
        )
        .unwrap();
    assert!(read_project(&path).is_err());
}

#[test]
fn v0_rgba_reader_and_transactional_migration_keep_old_file_on_failure() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("legacy.strata");
    let old = archive(32768);
    write_project(&path, &old, false, false).unwrap();
    {
        let connection = rusqlite::Connection::open(&path).unwrap();
        connection
            .execute_batch("ALTER TABLE tiles DROP COLUMN channels; PRAGMA user_version=0;")
            .unwrap();
    }
    assert_eq!(read_project(&path).unwrap(), old);
    assert_eq!(read_project_metadata(&path).unwrap(), old.metadata);
    assert!(
        write_project_with_hook(&path, &archive(32769), true, true, |stage| {
            if stage == SaveStage::BeforeCommit {
                Err(StorageError::Io(std::io::Error::new(
                    std::io::ErrorKind::StorageFull,
                    "migration failure",
                )))
            } else {
                Ok(())
            }
        })
        .is_err()
    );
    assert_eq!(read_project(&path).unwrap(), old);
    {
        let connection = rusqlite::Connection::open(&path).unwrap();
        assert_eq!(
            connection
                .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT count(*) FROM pragma_table_info('tiles') WHERE name='channels'",
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
            0
        );
    }
    let new = archive(32769);
    write_project(&path, &new, true, true).unwrap();
    assert_eq!(read_project(&path).unwrap(), new);
}
#[test]
fn precommit_faults_keep_previous_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.strata");
    let old = archive(1);
    write_project(&path, &old, false, false).unwrap();
    for incremental in [false, true] {
        for stage in [
            SaveStage::BeforeWrite,
            SaveStage::AfterTiles,
            SaveStage::BeforeCommit,
        ] {
            assert!(
                write_project_with_hook(&path, &archive(2), true, incremental, |at| {
                    if at == stage {
                        Err(StorageError::Io(std::io::Error::new(
                            std::io::ErrorKind::StorageFull,
                            "injected disk full",
                        )))
                    } else {
                        Ok(())
                    }
                })
                .is_err()
            );
            assert_eq!(read_project(&path).unwrap(), old);
        }
    }
}
#[test]
fn corruption_and_future_version_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.strata");
    write_project(&path, &archive(1), false, false).unwrap();
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute("UPDATE tiles SET hash=zeroblob(32)", [])
        .unwrap();
    assert!(read_project(&path).is_err());
    conn.execute_batch("PRAGMA user_version=99").unwrap();
    assert!(read_project(&path).is_err());
}
#[test]
fn png_tiff_export_preserves_all_16bit_samples_and_icc() {
    let dir = tempfile::tempdir().unwrap();
    let icc = strata_color::display_p3_profile().unwrap().icc().unwrap();
    let pixels = PixelBuffer::Rgba16(
        (0..=u16::MAX)
            .flat_map(|v| [v, v, v, if v % 2 == 0 { 0 } else { 65535 }])
            .collect(),
    );
    for (name, format) in [
        ("out.png", ExportFormat::Png),
        ("out.tiff", ExportFormat::Tiff),
    ] {
        let path = dir.path().join(name);
        export_image(&path, format, 1024, 64, &pixels, &icc, false).unwrap();
        let decoded = strata_io::decode_image(&path).unwrap();
        assert_eq!(decoded.pixels, pixels);
        assert_eq!(decoded.icc_profile.as_deref(), Some(icc.as_slice()));
    }
}

#[test]
fn srgb_export_keeps_profile_and_constant_16bit_samples() {
    let dir = tempfile::tempdir().unwrap();
    let icc = strata_color::working_profile("srgb").unwrap();
    for (depth, pixels) in [
        (8, PixelBuffer::Rgba8([127, 128, 200, 255].repeat(16 * 16))),
        (
            16,
            PixelBuffer::Rgba16([32768, 32769, 50001, 65535].repeat(16 * 16)),
        ),
    ] {
        for (extension, format) in [("png", ExportFormat::Png), ("tiff", ExportFormat::Tiff)] {
            let path = dir.path().join(format!("srgb-{depth}.{extension}"));
            export_image(&path, format, 16, 16, &pixels, &icc, false).unwrap();
            let decoded = strata_io::decode_image(&path).unwrap();
            assert_eq!(decoded.pixels, pixels);
            assert_eq!(decoded.icc_profile.as_deref(), Some(icc.as_slice()));
        }
    }
}

#[test]
fn crash_process_helper() {
    let Ok(path) = std::env::var("STRATA_CRASH_TEST_PATH") else {
        return;
    };
    let after = std::env::var("STRATA_CRASH_AFTER_COMMIT").unwrap() == "1";
    write_project_with_hook(
        std::path::Path::new(&path),
        &archive(2),
        true,
        true,
        |stage| {
            if stage
                == if after {
                    SaveStage::AfterCommit
                } else {
                    SaveStage::BeforeCommit
                }
            {
                std::process::exit(77);
            }
            Ok(())
        },
    )
    .unwrap();
}

#[test]
fn forced_process_exit_leaves_old_or_new_complete_project() {
    let dir = tempfile::tempdir().unwrap();
    for after in [false, true] {
        let path = dir.path().join(if after {
            "after.strata"
        } else {
            "before.strata"
        });
        write_project(&path, &archive(1), false, false).unwrap();
        let result = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "crash_process_helper", "--nocapture"])
            .env("STRATA_CRASH_TEST_PATH", &path)
            .env("STRATA_CRASH_AFTER_COMMIT", if after { "1" } else { "0" })
            .status()
            .unwrap();
        assert_eq!(result.code(), Some(77));
        assert_eq!(
            read_project(&path).unwrap(),
            archive(if after { 2 } else { 1 })
        );
    }
}

#[test]
fn publish_failure_and_post_publish_uncertainty_are_distinct() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("safe.strata");
    let old = archive(1);
    write_project(&path, &old, false, false).unwrap();
    for kind in [
        std::io::ErrorKind::WriteZero,
        std::io::ErrorKind::PermissionDenied,
        std::io::ErrorKind::StorageFull,
    ] {
        let result = write_project_with_hook(&path, &archive(2), true, false, |stage| {
            if stage == SaveStage::BeforePublish {
                Err(StorageError::Io(std::io::Error::new(kind, "injected")))
            } else {
                Ok(())
            }
        });
        assert!(result.is_err());
        assert_eq!(read_project(&path).unwrap(), old);
    }
    let result = write_project_with_hook(&path, &archive(2), true, false, |stage| {
        if stage == SaveStage::AfterPublish {
            Err(StorageError::Io(std::io::Error::other(
                "injected sync failure",
            )))
        } else {
            Ok(())
        }
    });
    assert!(matches!(result, Err(StorageError::PublishedUncertain(_))));
    assert_eq!(read_project(&path).unwrap(), archive(2));
}

#[test]
fn temporary_validation_failure_never_publishes_or_claims_uncertain_commit() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("safe.strata");
    let old = archive(1);
    write_project(&path, &old, false, false).unwrap();
    let result = write_project_with_hook(&path, &archive(2), true, false, |stage| {
        if stage == SaveStage::BeforeVerify {
            let temp = std::fs::read_dir(dir.path())?
                .map(|entry| entry.unwrap().path())
                .find(|entry| entry != &path)
                .unwrap();
            let conn = rusqlite::Connection::open(temp)?;
            conn.execute("UPDATE tiles SET hash=zeroblob(32)", [])?;
        }
        Ok(())
    });
    assert!(matches!(result, Err(StorageError::Invalid(_))));
    assert_eq!(read_project(&path).unwrap(), old);
    let result = write_project_with_hook(&path, &archive(2), true, false, |stage| {
        if stage == SaveStage::AfterCommit {
            Err(StorageError::Io(std::io::Error::other(
                "temporary sync failure",
            )))
        } else {
            Ok(())
        }
    });
    assert!(matches!(result, Err(StorageError::Io(_))));
    assert_eq!(read_project(&path).unwrap(), old);
}
