use serde_json::{Value, json};
use strata_core::{Editor, Source, commands::Control};
use strata_pixels::{PixelValue, TileCoord};

fn run(editor: &mut Editor, command: &str, params: Value) -> Value {
    editor
        .execute(editor.request(Source::User), command, params, |_| Ok(()))
        .unwrap()
}

fn fixture(editor: &mut Editor, depth: u8) {
    let color = if depth == 16 {
        [10001, 30003, 50001, 32769]
    } else {
        [41, 117, 191, 129]
    };
    run(
        editor,
        "file.new",
        json!({"width":16,"height":12,"bit_depth":depth,"working_space":"display_p3","resolution_dpi":144,"background":color}),
    );
}

fn duplicate(editor: &mut Editor, mode: &str) -> Editor {
    let params = json!({"name":"Working copy","mode":mode,"expected_revision":editor.revision()});
    let prepared = editor
        .prepare(editor.request(Source::User), "image.duplicate", params)
        .unwrap();
    assert!(!prepared.replaces_document() && !prepared.changes_display());
    let (result, copy) = editor.commit_with_document_copy(prepared).unwrap();
    let copy = copy.unwrap();
    assert_eq!(
        result,
        json!({"name":"Working copy","mode":mode,"copy_document_id":copy.editor.document().unwrap().id()})
    );
    copy.editor
}

#[test]
fn layered_copy_preserves_editable_structure_and_shares_tiles_until_edit() {
    let mut source = Editor::new();
    fixture(&mut source, 16);
    run(
        &mut source,
        "select.marquee",
        json!({"x":1,"y":2,"width":3,"height":4}),
    );
    run(
        &mut source,
        "layer.mask.add",
        json!({"layer":{"name":"Background"},"source":"from_selection"}),
    );
    run(
        &mut source,
        "select.marquee",
        json!({"x":2,"y":2,"width":2,"height":2}),
    );
    run(
        &mut source,
        "layer.create",
        json!({"name":"Hidden","fill":[10001,30003,50001,0]}),
    );
    run(
        &mut source,
        "layer.set_position",
        json!({"layer":{"name":"Hidden"},"x":-20,"y":10}),
    );
    run(
        &mut source,
        "layer.set_visibility",
        json!({"layer":{"name":"Hidden"},"visible":false}),
    );
    run(
        &mut source,
        "layer.set_blend_mode",
        json!({"layer":{"name":"Background"},"blend_mode":"dissolve"}),
    );
    run(
        &mut source,
        "layer.group",
        json!({"layers":[{"name":"Background"},{"name":"Hidden"}],"name":"Both"}),
    );
    run(
        &mut source,
        "view.guides.add",
        json!({"name":"Saved reference","axis":"vertical","position":5.5}),
    );

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("source.strata");
    let resource = source.grant_write(&path, false, Source::User).unwrap();
    run(&mut source, "file.save_as", json!({"resource":resource}));
    let bytes = std::fs::read(&path).unwrap();
    let unrelated = source
        .grant_write(&dir.path().join("unrelated.strata"), false, Source::User)
        .unwrap();
    let old_snapshot = serde_json::to_value(source.snapshot()).unwrap();
    let old_archive = source.document().unwrap().to_archive().unwrap();

    let mut child = duplicate(&mut source, "layered");
    let original = source.document().unwrap();
    let copy = child.document().unwrap();
    assert_ne!(copy.id(), original.id());
    assert_ne!(copy.state_id(), original.state_id());
    assert_ne!(child.snapshot().session_id, source.snapshot().session_id);
    assert_eq!(copy.frame(), original.frame());
    assert_eq!(copy.working_icc(), original.working_icc());
    assert_eq!(
        copy.export_pixels().unwrap(),
        original.export_pixels().unwrap()
    );
    assert_eq!(copy.resolution_dpi(), original.resolution_dpi());
    assert_eq!(copy.guides()[0].position, original.guides()[0].position);
    assert_eq!(copy.guides()[0].id, original.guides()[0].id);
    assert_eq!(
        copy.selection().unwrap().layout(),
        original.selection().unwrap().layout()
    );
    assert_eq!(
        copy.previous_selection().unwrap().layout(),
        original.previous_selection().unwrap().layout()
    );

    let tile = TileCoord::new(0, 0).unwrap();
    let group = copy
        .resolve(&strata_core::LayerRef::Name("Both".into()))
        .unwrap();
    for (a, b) in original.layers().iter().zip(copy.layers()) {
        assert_eq!(a.info(), b.info());
        assert_eq!(a.pixels().tile_version(tile), b.pixels().tile_version(tile));
        if let Some(mask) = a.mask() {
            assert_eq!(
                mask.tile_version(tile),
                b.mask().unwrap().tile_version(tile)
            );
        }
    }
    assert_eq!(copy.active_layer(), Some(group));
    assert_eq!(child.control(), &Control::User);
    assert!(child.dirty() && child.snapshot().history.is_empty());
    assert!(child.project_path().is_none() && child.revert_source().is_none());
    assert!(
        child
            .prepare(
                child.request(Source::User),
                "file.save_as",
                json!({"resource":unrelated})
            )
            .is_err()
    );

    run(&mut child, "select.none", json!({}));
    run(
        &mut child,
        "paint.fill",
        json!({"layer":{"name":"Background"},"color":[0,0,0,1]}),
    );
    let index = original
        .layers()
        .iter()
        .position(|layer| layer.info().name == "Background")
        .unwrap();
    let current = child.document().unwrap();
    assert_eq!(
        original.layers()[index].pixels().pixel(0, 0).unwrap(),
        PixelValue::Rgba16([10001, 30003, 50001, 32769])
    );
    assert_eq!(
        current.layers()[index].pixels().pixel(0, 0).unwrap(),
        PixelValue::Rgba16([0, 0, 0, 65535])
    );
    let child_path = dir.path().join("copy.strata");
    let resource = child.grant_write(&child_path, false, Source::User).unwrap();
    run(&mut child, "file.save_as", json!({"resource":resource}));
    assert!(child_path.is_file() && !child.dirty());
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    assert_eq!(
        serde_json::to_value(source.snapshot()).unwrap(),
        old_snapshot
    );
    assert!(source.document().unwrap().to_archive().unwrap() == old_archive);
}

#[test]
fn merged_copy_matches_current_artboard_pixels_at_both_depths() {
    for depth in [8, 16] {
        let mut source = Editor::new();
        fixture(&mut source, depth);
        let front = if depth == 16 {
            [60001, 15001, 5001, 40001]
        } else {
            [234, 58, 20, 156]
        };
        run(
            &mut source,
            "layer.create",
            json!({"name":"Second","fill":front}),
        );
        run(
            &mut source,
            "layer.set_position",
            json!({"layer":{"name":"Second"},"x":4,"y":3}),
        );
        run(
            &mut source,
            "image.crop",
            json!({"rect":{"x":2,"y":1,"width":12,"height":10},"pixels":"preserve"}),
        );
        let expected = source.document().unwrap().export_pixels().unwrap();
        let frame = source.document().unwrap().frame().clone();
        let child = duplicate(&mut source, "merged");
        let doc = child.document().unwrap();
        assert_eq!(doc.frame(), &frame);
        assert_eq!(doc.bit_depth(), depth);
        assert_eq!(doc.working_icc(), source.document().unwrap().working_icc());
        assert_eq!(doc.layers().len(), 1);
        assert_eq!((doc.layers()[0].info().x, doc.layers()[0].info().y), (2, 1));
        assert_eq!(doc.export_pixels().unwrap(), expected);
        assert_eq!(source.document().unwrap().layers().len(), 2);
    }
}

#[test]
fn stale_cancelled_invalid_and_retried_commands_cannot_publish_an_extra_document() {
    let mut source =
        Editor::with_memory_pool(strata_pixels::memory::MemoryPool::new(Default::default()));
    fixture(&mut source, 8);
    let pool = source.document().unwrap().memory_pool();
    let params = json!({"name":"New tab","mode":"layered"});
    let request = source.request(Source::User);
    let prepared = source
        .prepare(request.clone(), "image.duplicate", params.clone())
        .unwrap();
    let (original_result, effect) = source.commit_with_document_copy(prepared).unwrap();
    assert!(effect.is_some());
    drop(effect);
    let prepared = source
        .prepare(request, "image.duplicate", params.clone())
        .unwrap();
    let (replayed, duplicate) = source.commit_with_document_copy(prepared).unwrap();
    assert_eq!(replayed, original_result);
    assert!(duplicate.is_none());

    for invalid in [
        json!({"name":" \n"}),
        json!({"name":"中".repeat(86)}),
        json!({"name":"copy","path":"unused"}),
        json!({"name":"copy","expected_revision":source.revision()+1}),
    ] {
        assert!(
            source
                .prepare(source.request(Source::User), "image.duplicate", invalid)
                .is_err()
        );
    }
    let prepared = source
        .prepare(
            source.request(Source::User),
            "image.duplicate",
            params.clone(),
        )
        .unwrap();
    run(&mut source, "layer.create", json!({"name":"New edit"}));
    assert!(source.commit_with_document_copy(prepared).is_err());
    let request = source.request(Source::User);
    let prepared = source
        .prepare(request.clone(), "image.duplicate", params.clone())
        .unwrap();
    let reserved = pool.stats().source_bytes;
    source.cancel_pending_work();
    assert!(source.commit_with_document_copy(prepared).is_err());
    assert!(pool.stats().source_bytes < reserved);
    let prepared = source
        .prepare(source.request(Source::User), "image.duplicate", params)
        .unwrap();
    assert!(source.commit(prepared).is_err());
    assert_eq!(source.snapshot().history.len(), 1);
}
