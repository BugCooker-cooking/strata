use serde_json::{Value, json};
use strata_core::{Editor, Source};
fn run(e: &mut Editor, id: &str, p: Value) -> Value {
    e.execute(e.request(Source::User), id, p, |_| Ok(()))
        .unwrap()
}
fn editor() -> Editor {
    let mut e = Editor::new();
    run(
        &mut e,
        "file.new",
        json!({"width":16,"height":16,"bit_depth":16,"background":[32768,32769,50001,65535]}),
    );
    e
}
fn coverage(e: &Editor, x: i64, y: i64) -> u16 {
    e.document()
        .unwrap()
        .selection()
        .unwrap()
        .sample(x, y)
        .unwrap()
}
#[test]
fn polygon_rectangle_matches_marquee_and_ellipse_has_symmetric_soft_coverage() {
    let mut e = editor();
    run(
        &mut e,
        "select.marquee",
        json!({"x":2,"y":2,"width":8,"height":8}),
    );
    let rectangular = e.document().unwrap().selection().unwrap().clone();
    run(
        &mut e,
        "select.polygon",
        json!({"points":[[2,2],[10,2],[10,10],[2,10]]}),
    );
    for y in 0..16 {
        for x in 0..16 {
            assert_eq!(coverage(&e, x, y), rectangular.sample(x, y).unwrap());
        }
    }
    run(
        &mut e,
        "select.ellipse",
        json!({"x":2,"y":2,"width":8,"height":8}),
    );
    assert_eq!(coverage(&e, 5, 5), 65535);
    assert_eq!(coverage(&e, 2, 2), 0);
    let mut soft = false;
    for y in 2..10 {
        for x in 2..10 {
            let value = coverage(&e, x, y);
            assert_eq!(value, coverage(&e, 11 - x, y));
            assert_eq!(value, coverage(&e, x, 11 - y));
            soft |= value > 0 && value < 65535;
        }
    }
    assert!(soft);
}
#[test]
fn selection_reselect_and_previous_plane_survive_undo_project_and_legacy_rejection() {
    let mut e = editor();
    run(
        &mut e,
        "select.ellipse",
        json!({"x":2,"y":3,"width":8,"height":6}),
    );
    let before = e.document().unwrap().selection().unwrap().clone();
    run(&mut e, "select.none", json!({}));
    assert!(e.document().unwrap().selection().is_none());
    let archive = e.document().unwrap().to_archive().unwrap();
    let doc = strata_core::Document::from_archive(archive.clone()).unwrap();
    assert!(doc.previous_selection().is_some());
    assert!(
        archive
            .tiles
            .iter()
            .any(|t| t.layer == "previous_selection")
    );
    run(&mut e, "select.reselect", json!({}));
    for y in 0..16 {
        for x in 0..16 {
            assert_eq!(coverage(&e, x, y), before.sample(x, y).unwrap());
        }
    }
    run(&mut e, "edit.undo", json!({}));
    assert!(e.document().unwrap().selection().is_none());
    let mut old = archive;
    old.metadata["version"] = json!(2);
    assert!(strata_core::Document::from_archive(old).is_err());
}
#[test]
fn euclidean_expansion_contraction_and_border_have_expected_fixed_geometry() {
    let mut e = editor();
    run(
        &mut e,
        "select.marquee",
        json!({"x":5,"y":5,"width":4,"height":4}),
    );
    run(&mut e, "select.expand", json!({"radius":2}));
    assert_eq!(coverage(&e, 4, 6), 65535);
    assert_eq!(coverage(&e, 2, 6), 0);
    assert_eq!(coverage(&e, 3, 6), 32768);
    run(&mut e, "edit.undo", json!({}));
    run(&mut e, "select.contract", json!({"radius":1}));
    assert_eq!(coverage(&e, 6, 6), 65535);
    assert_eq!(coverage(&e, 5, 6), 32768);
    assert_eq!(coverage(&e, 4, 6), 0);
    run(&mut e, "edit.undo", json!({}));
    run(&mut e, "select.border", json!({"radius":2}));
    assert_eq!(coverage(&e, 5, 6), 65535);
    assert_eq!(coverage(&e, 4, 6), 65535);
    assert_eq!(coverage(&e, 6, 6), 0);
}
#[test]
fn feather_and_transform_preserve_source_and_adjacent_soft_values() {
    let mut e = editor();
    let source = e.document().unwrap().export_pixels().unwrap();
    run(
        &mut e,
        "select.marquee",
        json!({"x":5,"y":5,"width":4,"height":4}),
    );
    run(&mut e, "select.feather", json!({"radius":1}));
    let left = coverage(&e, 4, 6);
    let edge = coverage(&e, 5, 6);
    let center = coverage(&e, 6, 6);
    assert!(left > 0 && left < edge && edge < center && center < 65535);
    let before = e.document().unwrap().to_archive().unwrap();
    run(&mut e, "select.smooth", json!({"radius":0}));
    assert_eq!(coverage(&e, 4, 6), left);
    run(&mut e, "select.transform", json!({"matrix":[1,0,0,1,2,3]}));
    assert_eq!(coverage(&e, 6, 9), left);
    assert_eq!(coverage(&e, 7, 9), edge);
    assert_eq!(e.document().unwrap().export_pixels().unwrap(), source);
    let revision = e.revision();
    assert!(
        e.execute(
            e.request(Source::User),
            "select.transform",
            json!({"matrix":[0,0,0,0,0,0]}),
            |_| Ok(())
        )
        .is_err()
    );
    assert_eq!(e.revision(), revision);
    let _ = before;
}
#[test]
fn cross_tile_selection_changes_and_over_budget_failure_are_atomic() {
    use strata_pixels::memory::{MemoryLimits, MemoryPool};
    let pool = MemoryPool::new(MemoryLimits {
        cpu_bytes: 4 * 1024 * 1024,
        gpu_bytes: 0,
        disk_bytes: 16 * 1024 * 1024,
    });
    let mut e = Editor::with_memory_pool(pool);
    run(
        &mut e,
        "file.new",
        json!({"width":520,"height":32,"bit_depth":16}),
    );
    run(
        &mut e,
        "select.polygon",
        json!({"points":[[250,2],[270,2],[270,20],[250,20]]}),
    );
    assert_eq!(coverage(&e, 255, 10), 65535);
    assert_eq!(coverage(&e, 256, 10), 65535);
    run(&mut e, "select.feather", json!({"radius":2}));
    assert_eq!(coverage(&e, 255, 10), coverage(&e, 264, 10));
    run(
        &mut e,
        "file.new",
        json!({"width":2048,"height":2048,"bit_depth":16,"discard_changes":true}),
    );
    run(&mut e, "select.all", json!({}));
    let revision = e.revision();
    assert!(
        e.execute(
            e.request(Source::User),
            "select.feather",
            json!({"radius":2}),
            |_| Ok(())
        )
        .is_err()
    );
    assert_eq!(e.revision(), revision);
    assert_eq!(coverage(&e, 0, 0), 65535);
}

fn sampled_editor(depth: u8) -> Editor {
    use strata_core::ClipboardImage;
    use strata_pixels::{PixelBuffer, memory::MemoryPool};
    let mut e = Editor::new();
    run(
        &mut e,
        "file.new",
        json!({"width":16,"height":16,"bit_depth":depth}),
    );
    let mut samples = Vec::new();
    for y in 0..16 {
        for x in 0..16 {
            let color = if (3..7).contains(&x) && (3..12).contains(&y)
                || (10..13).contains(&x) && (4..10).contains(&y)
            {
                [65535, 0, 0, 65535]
            } else if y == 15 {
                [x * 257, 12345, 50001, 0]
            } else {
                [0, 0, 65535, 65535]
            };
            samples.extend(color);
        }
    }
    let pixels = if depth == 16 {
        PixelBuffer::Rgba16(samples)
    } else {
        PixelBuffer::Rgba8(samples.into_iter().map(|v| (v / 257) as u8).collect())
    };
    let image = ClipboardImage::new(
        pixels,
        16,
        16,
        0,
        0,
        strata_color::working_profile("srgb").unwrap(),
        &MemoryPool::global(),
    )
    .unwrap();
    let resource = e.grant_clipboard_image(image, Source::User).unwrap();
    run(&mut e, "edit.paste_in_place", json!({"resource":resource}));
    e
}
#[test]
fn wand_distinguishes_connected_and_global_colors_and_ignores_invisible_rgb() {
    for depth in [8, 16] {
        let mut e = sampled_editor(depth);
        let source = e.document().unwrap().export_pixels().unwrap();
        run(
            &mut e,
            "select.wand",
            json!({"x":4,"y":5,"tolerance":0,"sample":"current_layer"}),
        );
        assert_eq!(coverage(&e, 4, 5), 65535);
        assert_eq!(coverage(&e, 11, 5), 0);
        assert_eq!(coverage(&e, 2, 5), 0);
        run(
            &mut e,
            "select.wand",
            json!({"x":4,"y":5,"tolerance":0,"contiguous":false,"sample":"current_layer"}),
        );
        assert_eq!(coverage(&e, 11, 5), 65535);
        run(
            &mut e,
            "select.wand",
            json!({"x":4,"y":5,"tolerance":0,"sample":"current_layer","mode":"subtract"}),
        );
        assert_eq!(coverage(&e, 4, 5), 0);
        assert_eq!(coverage(&e, 11, 5), 65535);
        run(
            &mut e,
            "select.wand",
            json!({"x":0,"y":15,"tolerance":0,"sample":"current_layer"}),
        );
        assert_eq!(coverage(&e, 15, 15), 65535);
        assert_eq!(coverage(&e, 0, 14), 0);
        run(
            &mut e,
            "select.wand",
            json!({"x":4,"y":5,"tolerance":0,"sample":"merged"}),
        );
        assert_eq!(coverage(&e, 4, 5), 65535);
        assert_eq!(coverage(&e, 11, 5), 0);
        assert_eq!(e.document().unwrap().export_pixels().unwrap(), source);
    }
}
#[test]
fn seeded_graph_cut_follows_color_edges_and_preserves_source_through_history_and_save() {
    for depth in [8, 16] {
        let mut e = sampled_editor(depth);
        let source = e.document().unwrap().export_pixels().unwrap();
        run(
            &mut e,
            "select.quick",
            json!({"foreground":[[4.5,6.5]],"radius":0.5,"sample":"current_layer"}),
        );
        for y in 3..12 {
            for x in 3..7 {
                assert_eq!(coverage(&e, x, y), 65535);
            }
        }
        assert_eq!(coverage(&e, 0, 0), 0);
        assert_eq!(coverage(&e, 8, 6), 0);
        let selection = e.document().unwrap().selection().unwrap().clone();
        let archive = e.document().unwrap().to_archive().unwrap();
        let reopened = strata_core::Document::from_archive(archive).unwrap();
        assert_eq!(reopened.selection().unwrap().sample(4, 6).unwrap(), 65535);
        run(&mut e, "edit.undo", json!({}));
        assert!(e.document().unwrap().selection().is_none());
        run(&mut e, "edit.redo", json!({}));
        for y in 0..16 {
            for x in 0..16 {
                assert_eq!(coverage(&e, x, y), selection.sample(x, y).unwrap());
            }
        }
        assert_eq!(e.document().unwrap().export_pixels().unwrap(), source);
        let revision = e.revision();
        assert!(
            e.execute(
                e.request(Source::User),
                "select.quick",
                json!({"foreground":[[4,6]],"background":[[4,6]],"radius":1}),
                |_| Ok(())
            )
            .is_err()
        );
        assert_eq!(e.revision(), revision);
    }
}

#[test]
fn previous_selection_survives_real_file_and_recovery_and_paused_edits_are_reported() {
    let mut e = editor();
    run(
        &mut e,
        "select.ellipse",
        json!({"x":2,"y":3,"width":10,"height":8}),
    );
    run(&mut e, "select.none", json!({}));
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("selection.strata");
    let write = e.grant_write(&path, false, Source::User).unwrap();
    run(&mut e, "file.save_as", json!({"resource":write}));
    let mut reopened = Editor::new();
    let read = reopened.grant_read(&path, Source::User).unwrap();
    run(&mut reopened, "file.open", json!({"resource":read}));
    run(&mut reopened, "select.reselect", json!({}));
    assert_eq!(coverage(&reopened, 7, 7), 65535);
    run(&mut reopened, "select.none", json!({}));
    let store = strata_io::recovery::RecoveryStore::new(&dir.path().join("recovery")).unwrap();
    let info = reopened
        .checkpoint_recovery(&store, "selection")
        .unwrap()
        .unwrap();
    let mut recovered = Editor::new();
    let read = recovered
        .grant_read(&store.path(&info.session_id).unwrap(), Source::User)
        .unwrap();
    run(&mut recovered, "file.recover", json!({"resource":read}));
    let turn = recovered.begin_turn().unwrap();
    recovered.pause_turn().unwrap();
    run(&mut recovered, "select.reselect", json!({}));
    run(&mut recovered, "select.feather", json!({"radius":1}));
    assert!(coverage(&recovered, 7, 7) > 60000);
    recovered.resume_turn().unwrap();
    let state = recovered.snapshot();
    assert!(
        state
            .control_events
            .iter()
            .any(|event| event["command"] == "select.feather")
    );
    assert_eq!(state.history.last().unwrap().turn_id, Some(turn));
}

#[test]
fn disk_exhaustion_after_partial_ellipse_preparation_keeps_original_document() {
    use strata_pixels::memory::{MemoryLimits, MemoryPool};
    let pool = MemoryPool::new(MemoryLimits {
        cpu_bytes: 512 * 1024,
        gpu_bytes: 0,
        disk_bytes: 0,
    });
    let mut e = Editor::with_memory_pool(pool);
    run(
        &mut e,
        "file.new",
        json!({"width":1024,"height":1024,"bit_depth":16}),
    );
    run(&mut e, "select.all", json!({}));
    let revision = e.revision();
    assert!(
        e.execute(
            e.request(Source::User),
            "select.ellipse",
            json!({"x":0,"y":0,"width":1024,"height":1024}),
            |_| Ok(())
        )
        .is_err()
    );
    assert_eq!(e.revision(), revision);
    assert_eq!(coverage(&e, 0, 0), 65535);
    assert_eq!(coverage(&e, 512, 512), 65535);
}
