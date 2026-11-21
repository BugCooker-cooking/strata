use serde_json::{Value, json};
use std::path::Path;
use strata_core::{Editor, LayerKind, Source, commands::CommandError};
use strata_pixels::PixelValue;

const PSD: &[u8] = include_bytes!("fixtures/psd/group-mask.psd");

fn run(editor: &mut Editor, command: &str, params: Value) -> Value {
    editor
        .execute(editor.request(Source::User), command, params, |_| Ok(()))
        .unwrap()
}
fn open(editor: &mut Editor, path: &Path) {
    let token = editor.grant_read(path, Source::User).unwrap();
    run(
        editor,
        "file.open",
        json!({"resource":token,"discard_changes":true}),
    );
}
#[test]
fn editable_layers_mask_source_samples_and_history_survive_save_and_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let original = dir.path().join("original.dat"); // Recognized by signature.
    let project = dir.path().join("edited.strata");
    std::fs::write(&original, PSD).unwrap();
    let mut editor = Editor::new();
    open(&mut editor, &original);
    let doc = editor.document().unwrap();
    assert_eq!(
        (
            doc.frame().width,
            doc.frame().height,
            doc.bit_depth(),
            doc.resolution_dpi()
        ),
        (4, 3, 16, 300)
    );
    assert_eq!(
        doc.layers()
            .iter()
            .map(|l| l.info().name.as_str())
            .collect::<Vec<_>>(),
        ["Base", "细节", "Exposure", "Invert", "Group"]
    );
    let detail = &doc.layers()[1];
    assert_eq!(detail.info().parent, Some(doc.layers()[4].id()));
    assert_eq!(doc.layers()[2].info().parent, detail.info().parent);
    assert!(doc.layers()[4].info().pass_through);
    assert!(!doc.layers()[2].info().visible);
    assert_eq!(
        doc.layers()[2].info().kind,
        LayerKind::Exposure { stops: 1.5 }
    );
    assert_eq!((detail.info().x, detail.info().y), (-1, 1));
    assert_eq!(
        detail.pixels().pixel(0, 0).unwrap(),
        PixelValue::Rgba16([12345, 0, 45678, 0])
    );
    assert_eq!(
        detail.pixels().pixel(2, 1).unwrap(),
        PixelValue::Rgba16([22222, 0, 45678, 32769])
    );
    let mask = detail.mask().unwrap();
    for ((x, y), value) in [
        ((-1, 1), 65535),
        ((0, 1), 0),
        ((1, 1), 65535),
        ((0, 2), 10001),
        ((1, 2), 40003),
    ] {
        assert_eq!(mask.sample(x, y).unwrap(), value);
    }
    assert!(editor.snapshot().history.is_empty());
    assert!(!editor.dirty());
    let before = doc.to_archive().unwrap();
    let composite = doc.export_pixels().unwrap();
    for name in ["Invert", "Exposure"] {
        run(
            &mut editor,
            "layer.set_visibility",
            json!({"layer":{"name":name},"visible":true}),
        );
        assert_ne!(
            editor.document().unwrap().export_pixels().unwrap(),
            composite
        );
        run(&mut editor, "edit.undo", json!({}));
        assert_eq!(
            editor.document().unwrap().export_pixels().unwrap(),
            composite
        );
    }
    run(
        &mut editor,
        "layer.rename",
        json!({"layer":{"name":"细节"},"name":"Detail"}),
    );
    run(&mut editor, "edit.undo", json!({}));
    assert!(editor.document().unwrap().to_archive().unwrap() == before);
    run(&mut editor, "edit.redo", json!({}));
    let edited = editor.document().unwrap().to_archive().unwrap();
    let save = editor.grant_write(&project, false, Source::User).unwrap();
    run(&mut editor, "file.save_as", json!({"resource":save}));
    open(&mut editor, &project);
    assert!(editor.document().unwrap().to_archive().unwrap() == edited);
    assert_eq!(std::fs::read(original).unwrap(), PSD);
}

#[test]
fn failed_or_unapproved_open_leaves_current_revision_samples_and_history_intact() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("document.psd");
    std::fs::write(&input, PSD).unwrap();
    let mut editor = Editor::new();
    open(&mut editor, &input);
    run(
        &mut editor,
        "layer.rename",
        json!({"layer":{"name":"细节"},"name":"Keep"}),
    );
    let revision = editor.revision();
    let truth = editor.document().unwrap().to_archive().unwrap();
    let history = editor.snapshot().history_cursor;
    let grant = editor.grant_read(&input, Source::User).unwrap();
    assert!(matches!(
        editor.execute(
            editor.request(Source::User),
            "file.open",
            json!({"resource":grant}),
            |_| Ok(())
        ),
        Err(CommandError::UnsavedChanges)
    ));
    for replacement in [b"TySh", b"SoLd", b"vmsk"] {
        let mut unsupported = PSD.to_vec();
        let offset = unsupported.windows(4).position(|k| k == b"expA").unwrap();
        unsupported[offset..offset + 4].copy_from_slice(replacement);
        std::fs::write(&input, unsupported).unwrap();
        let grant = editor.grant_read(&input, Source::User).unwrap();
        let error = editor
            .execute(
                editor.request(Source::User),
                "file.open",
                json!({"resource":grant,"discard_changes":true}),
                |_| Ok(()),
            )
            .unwrap_err();
        assert!(matches!(
            error,
            CommandError::Image(strata_io::IoError::Unsupported { .. })
        ));
    }
    std::fs::write(&input, &PSD[..PSD.len() / 2]).unwrap();
    let grant = editor.grant_read(&input, Source::User).unwrap();
    assert!(
        editor
            .execute(
                editor.request(Source::User),
                "file.open",
                json!({"resource":grant,"discard_changes":true}),
                |_| Ok(())
            )
            .is_err()
    );
    assert_eq!(
        (editor.revision(), editor.snapshot().history_cursor),
        (revision, history)
    );
    assert!(editor.document().unwrap().to_archive().unwrap() == truth);
}

#[test]
fn ai_open_requires_a_read_grant_for_this_turn_and_returns_editable_layer() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("design.psd");
    std::fs::write(&path, PSD).unwrap();
    let mut editor = Editor::new();
    let user_grant = editor.grant_read(&path, Source::User).unwrap();
    let turn = editor.begin_turn().unwrap();
    let source = Source::Ai { turn_id: turn };
    let request = editor.request(source.clone());
    assert!(matches!(
        editor.execute(
            request,
            "file.open",
            json!({"resource":user_grant}),
            |_| Ok(())
        ),
        Err(CommandError::Permission)
    ));
    let token = editor.grant_read(&path, source.clone()).unwrap();
    let result = editor
        .execute(
            editor.request(source),
            "file.open",
            json!({"resource":token}),
            |_| Ok(()),
        )
        .unwrap();
    let document = editor.document().unwrap();
    assert_eq!(result["document_id"], json!(document.id()));
    assert_eq!(result["layer_id"], json!(document.active_layer()));
    assert_eq!(document.layers().len(), 5);
}

#[test]
fn flat_rgb_samples_and_embedded_profile_are_kept_at_both_depths() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("flat.psd");
    let icc = strata_color::working_profile("display_p3").unwrap();
    for depth in [8u16, 16] {
        let mut rgb = b"8BPS\0\x01\0\0\0\0\0\0\0\x03\0\0\0\x01\0\0\0\x02".to_vec();
        rgb.extend_from_slice(&depth.to_be_bytes());
        rgb.extend_from_slice(&[0, 3, 0, 0, 0, 0]);
        let mut resource = b"8BIM\x04\x0f\0\0".to_vec();
        resource.extend_from_slice(&(icc.len() as u32).to_be_bytes());
        resource.extend_from_slice(&icc);
        if !icc.len().is_multiple_of(2) {
            resource.push(0);
        }
        rgb.extend_from_slice(&(resource.len() as u32).to_be_bytes());
        rgb.extend(resource);
        rgb.extend_from_slice(&[0; 6]); // No layer section, uncompressed planar composite.
        let expected = if depth == 8 {
            rgb.extend_from_slice(&[12, 13, 90, 91, 234, 235]);
            PixelValue::Rgba8([13, 91, 235, 255])
        } else {
            for sample in [12345u16, 12346, 20000, 20001, 55000, 55001] {
                rgb.extend_from_slice(&sample.to_be_bytes());
            }
            PixelValue::Rgba16([12346, 20001, 55001, 65535])
        };
        std::fs::write(&path, rgb).unwrap();
        let mut editor = Editor::new();
        open(&mut editor, &path);
        let doc = editor.document().unwrap();
        assert_eq!(doc.working_icc(), icc);
        assert_eq!(doc.layers()[0].pixels().pixel(1, 0).unwrap(), expected);
    }
}
