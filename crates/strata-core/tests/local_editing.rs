use serde_json::json;
use strata_core::{Editor, Source};
use strata_pixels::PixelBuffer;

fn editor() -> Editor {
    let mut editor = Editor::new();
    editor
        .execute(
            editor.request(Source::Test),
            "file.new",
            json!({"width":8,"height":8,"bit_depth":16,"background":[16384,16384,16384,65535]}),
            |_| Ok(()),
        )
        .unwrap();
    editor
}
fn run(editor: &mut Editor, command: &str, params: serde_json::Value) -> serde_json::Value {
    editor
        .execute(editor.request(Source::Test), command, params, |_| Ok(()))
        .unwrap()
}

#[test]
fn rectangular_selection_is_bounded_and_supports_boolean_operations() {
    let mut editor = editor();
    run(
        &mut editor,
        "select.marquee",
        json!({"x":2,"y":2,"width":3,"height":3}),
    );
    assert_eq!(
        editor.snapshot().selection.unwrap().bounds,
        strata_pixels::CoverageRect {
            x: 0,
            y: 0,
            width: 8,
            height: 8
        }
    );
    assert_eq!(
        editor
            .document()
            .unwrap()
            .selection()
            .unwrap()
            .sample(0, 0)
            .unwrap(),
        0
    );
    assert_eq!(
        editor
            .document()
            .unwrap()
            .selection()
            .unwrap()
            .sample(2, 2)
            .unwrap(),
        65535
    );
    run(
        &mut editor,
        "select.marquee",
        json!({"x":4,"y":4,"width":3,"height":3,"mode":"add"}),
    );
    let selection = editor.document().unwrap().selection().unwrap();
    assert_eq!(selection.sample(2, 2).unwrap(), 65535);
    assert_eq!(selection.sample(6, 6).unwrap(), 65535);
    assert_eq!(selection.sample(3, 6).unwrap(), 0);
    run(&mut editor, "select.inverse", json!({}));
    assert_eq!(
        editor
            .document()
            .unwrap()
            .selection()
            .unwrap()
            .sample(3, 6)
            .unwrap(),
        65535
    );
    run(&mut editor, "select.none", json!({}));
    assert!(editor.document().unwrap().selection().is_none());
}

#[test]
fn mask_and_exposure_are_non_destructive_and_roundtrip_exactly() {
    let mut editor = editor();
    run(
        &mut editor,
        "select.marquee",
        json!({"x":0,"y":0,"width":4,"height":8}),
    );
    let layer = run(
        &mut editor,
        "layer.add_adjustment",
        json!({"name":"局部提亮","type":"exposure","stops":1.0,"mask":"from_selection"}),
    )["layer_id"]
        .clone();
    assert!(
        editor
            .document()
            .unwrap()
            .layers()
            .last()
            .unwrap()
            .mask()
            .is_some()
    );
    let before = editor
        .document()
        .unwrap()
        .layers()
        .first()
        .unwrap()
        .pixels()
        .pixel(0, 0)
        .unwrap();
    let exported = editor.document().unwrap().export_pixels().unwrap();
    let PixelBuffer::Rgba16(values) = exported else {
        panic!()
    };
    assert!(values[0] > 16384);
    assert_eq!(
        editor
            .document()
            .unwrap()
            .layers()
            .first()
            .unwrap()
            .pixels()
            .pixel(0, 0)
            .unwrap(),
        before
    );
    let archive = editor.document().unwrap().to_archive().unwrap();
    let reopened = strata_core::Document::from_archive(archive.clone()).unwrap();
    assert_eq!(reopened.to_archive().unwrap(), archive);
    assert_eq!(
        reopened.export_pixels().unwrap(),
        PixelBuffer::Rgba16(values.clone())
    );
    run(
        &mut editor,
        "layer.mask.invert",
        json!({"layer":{"id":layer}}),
    );
    let inverted = editor.document().unwrap().export_pixels().unwrap();
    let PixelBuffer::Rgba16(inverted) = inverted else {
        panic!()
    };
    assert_eq!(inverted[0], 16384);
    assert!(inverted[4 * 7] > 16384);
}

#[test]
fn mask_enable_remove_and_bad_adjustment_parameters_are_atomic() {
    let mut editor = editor();
    let layer = run(
        &mut editor,
        "layer.mask.add",
        json!({"layer":{"name":"Background"},"source":"black"}),
    )["layer_id"]
        .clone();
    assert!(
        editor
            .document()
            .unwrap()
            .layers()
            .first()
            .unwrap()
            .mask()
            .is_some()
    );
    let revision = editor.revision();
    assert!(
        editor
            .execute(
                editor.request(Source::Test),
                "adjust.exposure.set",
                json!({"layer":{"id":layer},"stops":99.0}),
                |_| Ok(())
            )
            .is_err()
    );
    assert_eq!(editor.revision(), revision);
    run(
        &mut editor,
        "layer.mask.set_enabled",
        json!({"layer":{"id":layer},"enabled":false}),
    );
    run(
        &mut editor,
        "layer.mask.remove",
        json!({"layer":{"id":layer}}),
    );
    assert!(
        editor
            .document()
            .unwrap()
            .layers()
            .first()
            .unwrap()
            .mask()
            .is_none()
    );
}

#[test]
fn exposure_pixels_match_independent_linear_srgb_reference_at_both_depths() {
    for depth in [8, 16] {
        let maximum = if depth == 8 { 255u16 } else { 65535u16 };
        let code = (f64::from(maximum) * 0.25).round() as u16;
        let mut editor = Editor::new();
        run(
            &mut editor,
            "file.new",
            json!({"width":2,"height":1,"bit_depth":depth,"background":[code,code,code,maximum]}),
        );
        run(
            &mut editor,
            "select.marquee",
            json!({"x":0,"y":0,"width":1,"height":1}),
        );
        let created = run(
            &mut editor,
            "layer.add_adjustment",
            json!({"name":"exposure","type":"exposure","stops":0.5,"mask":"from_selection"}),
        );
        run(
            &mut editor,
            "layer.set_opacity",
            json!({"layer":{"id":created["layer_id"]},"opacity":0.7}),
        );
        let source = editor.document().unwrap().layers()[0]
            .pixels()
            .pixel(0, 0)
            .unwrap();
        let encoded = f64::from(code) / f64::from(maximum);
        let linear = if encoded <= 0.04045 {
            encoded / 12.92
        } else {
            ((encoded + 0.055) / 1.055).powf(2.4)
        };
        let brighter = linear * (1.0 + (2f64.sqrt() - 1.0) * 0.7);
        let expected = (if brighter <= 0.0031308 {
            brighter * 12.92
        } else {
            1.055 * brighter.powf(1.0 / 2.4) - 0.055
        }) * f64::from(maximum);
        let exported = match editor.document().unwrap().export_pixels().unwrap() {
            PixelBuffer::Rgba8(v) => v.into_iter().map(u16::from).collect::<Vec<_>>(),
            PixelBuffer::Rgba16(v) => v,
        };
        for c in 0..3 {
            assert!((f64::from(exported[c]) - expected).abs() <= 3.0);
            assert!(exported[c + 4].abs_diff(code) <= 3);
        }
        assert_eq!(exported[3], maximum);
        assert_eq!(exported[7], maximum);
        assert_eq!(
            editor.document().unwrap().layers()[0]
                .pixels()
                .pixel(0, 0)
                .unwrap(),
            source
        );
        run(&mut editor, "edit.undo", json!({}));
        run(&mut editor, "edit.undo", json!({}));
        assert_eq!(editor.document().unwrap().layers().len(), 1);
    }
}

#[test]
fn adjustment_parameters_mask_enable_and_inversion_invalidate_display_cache() {
    use strata_core::composite::Compositor;
    use strata_pixels::TileCoord;
    let mut editor = editor();
    let coord = TileCoord::new(0, 0).unwrap();
    let mut cache = Compositor::new(2);
    let baseline = cache.tile(editor.document().unwrap(), coord).unwrap()[0];
    let adjustment = run(
        &mut editor,
        "layer.add_adjustment",
        json!({"name":"exposure","type":"exposure","stops":0.5,"mask":"black"}),
    )["layer_id"]
        .clone();
    assert_eq!(
        cache.tile(editor.document().unwrap(), coord).unwrap()[0],
        baseline
    );
    run(
        &mut editor,
        "layer.mask.invert",
        json!({"layer":{"id":adjustment}}),
    );
    let brighter = cache.tile(editor.document().unwrap(), coord).unwrap()[0];
    assert!(brighter[0] > baseline[0]);
    run(
        &mut editor,
        "adjust.exposure.set",
        json!({"layer":{"id":adjustment},"stops":1.0}),
    );
    let brightest = cache.tile(editor.document().unwrap(), coord).unwrap()[0];
    assert!(brightest[0] > brighter[0]);
    run(
        &mut editor,
        "layer.mask.invert",
        json!({"layer":{"id":adjustment}}),
    );
    assert_eq!(
        cache.tile(editor.document().unwrap(), coord).unwrap()[0],
        baseline
    );
    run(
        &mut editor,
        "layer.mask.set_enabled",
        json!({"layer":{"id":adjustment},"enabled":false}),
    );
    assert_eq!(
        cache.tile(editor.document().unwrap(), coord).unwrap()[0],
        brightest
    );
}

#[test]
fn paused_user_rectangle_is_undoable_and_reported_on_resume() {
    let mut editor = editor();
    let turn = editor.begin_turn().unwrap();
    editor.pause_turn().unwrap();
    editor
        .execute(
            editor.request(Source::User),
            "select.marquee",
            json!({"x":2,"y":3,"width":3,"height":2}),
            |_| Ok(()),
        )
        .unwrap();
    let state = editor.snapshot();
    assert_eq!(
        state.selection_extent,
        Some(strata_pixels::CoverageRect {
            x: 2,
            y: 3,
            width: 3,
            height: 2
        })
    );
    assert_eq!(
        state.control_events.last().unwrap()["command"],
        "select.marquee"
    );
    assert_eq!(state.history.last().unwrap().turn_id, Some(turn));
    run(&mut editor, "edit.undo", json!({}));
    assert!(editor.document().unwrap().selection().is_none());
    editor.resume_turn().unwrap();
    let state = editor.snapshot();
    assert!(state.selection.is_none());
    assert_eq!(state.control_events.len(), 2);
}

struct AmbiguousSubject;
impl strata_core::SubjectSelector for AmbiguousSubject {
    fn select(
        &self,
        doc: &strata_core::Document,
        _: &strata_core::SubjectParams,
        _: &(dyn Fn() -> bool + Sync),
    ) -> Result<strata_core::SubjectSelection, String> {
        let frame = doc.frame();
        let mask = strata_pixels::TiledMask::new(
            strata_pixels::MaskLayout {
                bounds: strata_pixels::CoverageRect {
                    x: frame.x,
                    y: frame.y,
                    width: frame.width,
                    height: frame.height,
                },
                default_coverage: 65535,
                inverted: false,
            },
            1,
            doc.memory_pool(),
        )
        .map_err(|e| e.to_string())?;
        Ok(strata_core::SubjectSelection {
            mask,
            diagnostics: json!({"components":2,"fixture":true}),
            needs_confirmation: true,
        })
    }
}
#[test]
fn missing_or_ambiguous_subject_does_not_replace_existing_selection_without_confirmation() {
    let mut editor = editor();
    run(
        &mut editor,
        "select.marquee",
        json!({"x":1,"y":1,"width":2,"height":2}),
    );
    let before = editor.document().unwrap().to_archive().unwrap();
    let revision = editor.revision();
    assert!(
        editor
            .execute(
                editor.request(Source::User),
                "select.subject",
                json!({}),
                |_| Ok(())
            )
            .is_err()
    );
    editor.set_subject_selector(std::sync::Arc::new(AmbiguousSubject));
    let error = editor
        .execute(
            editor.request(Source::User),
            "select.subject",
            json!({}),
            |_| Ok(()),
        )
        .unwrap_err();
    assert!(error.to_string().contains("ambiguous"));
    assert_eq!(editor.revision(), revision);
    assert_eq!(editor.document().unwrap().to_archive().unwrap(), before);
    run(
        &mut editor,
        "select.subject",
        json!({"accept_ambiguous":true}),
    );
    assert_eq!(
        editor
            .document()
            .unwrap()
            .selection()
            .unwrap()
            .sample(0, 0)
            .unwrap(),
        65535
    );
}

#[test]
fn checkpoints_and_disk_project_preserve_selection_mask_exposure_and_pixels() {
    let dir = tempfile::tempdir().unwrap();
    let mut editor = editor();
    run(
        &mut editor,
        "select.marquee",
        json!({"x":1,"y":2,"width":3,"height":4}),
    );
    run(
        &mut editor,
        "layer.add_adjustment",
        json!({"name":"exposure","type":"exposure","stops":0.5,"mask":"from_selection"}),
    );
    let expected = editor.document().unwrap().to_archive().unwrap();
    let pixels = editor.document().unwrap().export_pixels().unwrap();
    let store = strata_io::recovery::RecoveryStore::new(&dir.path().join("Recovery")).unwrap();
    let info = editor
        .checkpoint_recovery(&store, "local editing")
        .unwrap()
        .unwrap();
    let mut restored = Editor::new();
    let resource = restored
        .grant_read(&store.path(&info.session_id).unwrap(), Source::Test)
        .unwrap();
    run(&mut restored, "file.recover", json!({"resource":resource}));
    assert!(restored.dirty());
    assert_eq!(restored.document().unwrap().to_archive().unwrap(), expected);
    let path = dir.path().join("local-edit.strata");
    let resource = restored.grant_write(&path, false, Source::Test).unwrap();
    run(&mut restored, "file.save_as", json!({"resource":resource}));
    let resource = restored.grant_read(&path, Source::Test).unwrap();
    run(&mut restored, "file.open", json!({"resource":resource}));
    assert_eq!(restored.document().unwrap().to_archive().unwrap(), expected);
    assert_eq!(
        restored.document().unwrap().export_pixels().unwrap(),
        pixels
    );
}

struct CancellableSubject(std::sync::mpsc::SyncSender<()>);
impl strata_core::SubjectSelector for CancellableSubject {
    fn select(
        &self,
        _: &strata_core::Document,
        _: &strata_core::SubjectParams,
        cancel: &(dyn Fn() -> bool + Sync),
    ) -> Result<strata_core::SubjectSelection, String> {
        self.0.send(()).map_err(|e| e.to_string())?;
        while !cancel() {
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        Err("model inference cancelled".into())
    }
}
#[test]
fn cancelling_subject_preparation_revokes_late_commit_and_keeps_document() {
    let mut editor = editor();
    let (started, ready) = std::sync::mpsc::sync_channel(1);
    editor.set_subject_selector(std::sync::Arc::new(CancellableSubject(started)));
    let revision = editor.revision();
    let snapshot = editor.clone();
    let request = editor.request(Source::User);
    let worker = std::thread::spawn(move || snapshot.prepare(request, "select.subject", json!({})));
    ready
        .recv_timeout(std::time::Duration::from_secs(2))
        .unwrap();
    editor.cancel_pending_work();
    assert!(worker.join().unwrap().is_err());
    assert_eq!(editor.revision(), revision);
    assert!(editor.document().unwrap().selection().is_none());
    run(
        &mut editor,
        "layer.create",
        json!({"name":"user can continue"}),
    );
    assert_eq!(editor.document().unwrap().layers().len(), 2);
}

#[test]
fn display_prefix_keeps_source_cache_reusable_across_exposure_changes() {
    use strata_core::composite::Compositor;
    use strata_pixels::TileCoord;
    let mut editor = editor();
    let coord = TileCoord::new(0, 0).unwrap();
    let mut cache = Compositor::new(4);
    let original = cache.tile(editor.document().unwrap(), coord).unwrap();
    let layer = run(
        &mut editor,
        "layer.add_adjustment",
        json!({"name":"exposure","type":"exposure","stops":0.5,"mask":"white"}),
    )["layer_id"]
        .clone();
    let prefix = editor.document().unwrap().layers().len() - 1;
    let base = cache
        .tile_prefix(editor.document().unwrap(), coord, prefix)
        .unwrap();
    assert!(std::sync::Arc::ptr_eq(&original, &base));
    let count = cache.recomputed;
    run(
        &mut editor,
        "adjust.exposure.set",
        json!({"layer":{"id":layer},"stops":1.0}),
    );
    let unchanged = cache
        .tile_prefix(editor.document().unwrap(), coord, prefix)
        .unwrap();
    assert!(std::sync::Arc::ptr_eq(&base, &unchanged));
    assert_eq!(cache.recomputed, count);
    assert!(cache.tile(editor.document().unwrap(), coord).unwrap()[0][0] > base[0][0]);
}
