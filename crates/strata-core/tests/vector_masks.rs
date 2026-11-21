use serde_json::{Value, json};
use strata_core::{Document, Editor, Source};
use strata_pixels::{PixelBuffer, TileCoord};
fn run(e: &mut Editor, id: &str, p: Value) -> Value {
    e.execute(e.request(Source::User), id, p, |_| Ok(()))
        .unwrap()
}
fn new() -> Editor {
    let mut e = Editor::new();
    run(
        &mut e,
        "file.new",
        json!({"width":20,"height":16,"bit_depth":16,"background":[12345,23000,55000,65535]}),
    );
    run(
        &mut e,
        "path.create",
        json!({"name":"Window","anchors":[{"position":[2,2]},{"position":[10,2]},{"position":[10,11]},{"position":[2,11]}]}),
    );
    run(
        &mut e,
        "layer.vector_mask.set",
        json!({"layer":{"name":"Background"},"path":{"name":"Window"}}),
    );
    e
}
fn alpha(doc: &Document, x: usize, y: usize) -> u16 {
    let PixelBuffer::Rgba16(v) = doc.export_pixels().unwrap() else {
        panic!()
    };
    v[(y * doc.frame().width as usize + x) * 4 + 3]
}
#[test]
fn linked_path_changes_alpha_and_cache_without_modifying_pixels_or_raster_mask() {
    let mut e = new();
    let original = e.document().unwrap().layers()[0]
        .pixels()
        .pixel(15, 5)
        .unwrap();
    let mut compositor = strata_core::composite::Compositor::new(8);
    let coord = TileCoord::new(0, 0).unwrap();
    let before = compositor.tile(e.document().unwrap(), coord).unwrap();
    assert_eq!(alpha(e.document().unwrap(), 15, 5), 0);
    assert_eq!(alpha(e.document().unwrap(), 5, 5), 65535);
    run(
        &mut e,
        "select.marquee",
        json!({"x":0,"y":0,"width":20,"height":7}),
    );
    run(
        &mut e,
        "layer.mask.add",
        json!({"layer":{"name":"Background"},"source":"from_selection"}),
    );
    let args = json!({"path":{"name":"Window"},"anchors":[{"position":[2,2]},{"position":[18,2]},{"position":[18,14]},{"position":[2,14]}]});
    let p = e
        .prepare(e.request(Source::User), "path.update", args)
        .unwrap();
    assert!(p.changes_display());
    e.commit(p).unwrap();
    let after = compositor.tile(e.document().unwrap(), coord).unwrap();
    assert_eq!(before[5 * 256 + 15][3], 0.0);
    assert_eq!(after[5 * 256 + 15][3], 1.0);
    assert_eq!(alpha(e.document().unwrap(), 15, 5), 65535);
    assert_eq!(alpha(e.document().unwrap(), 15, 10), 0);
    assert_eq!(
        e.document().unwrap().layers()[0]
            .pixels()
            .pixel(15, 5)
            .unwrap(),
        original
    );
    let p = e
        .prepare(e.request(Source::User), "edit.undo", json!({}))
        .unwrap();
    assert!(p.changes_display());
    e.commit(p).unwrap();
    assert_eq!(alpha(e.document().unwrap(), 15, 5), 0);
    run(
        &mut e,
        "layer.vector_mask.set",
        json!({"layer":{"name":"Background"},"inverted":true}),
    );
    assert_eq!(alpha(e.document().unwrap(), 15, 5), 65535);
    assert_eq!(alpha(e.document().unwrap(), 5, 5), 0);
    run(
        &mut e,
        "layer.vector_mask.set",
        json!({"layer":{"name":"Background"},"enabled":false}),
    );
    assert_eq!(alpha(e.document().unwrap(), 5, 5), 65535);
    run(
        &mut e,
        "layer.mask.apply",
        json!({"layer":{"name":"Background"}}),
    );
    assert!(
        e.document().unwrap().layers()[0]
            .info()
            .vector_mask
            .is_some()
    );
    run(
        &mut e,
        "layer.vector_mask.remove",
        json!({"layer":{"name":"Background"}}),
    );
    assert!(
        e.document().unwrap().layers()[0]
            .info()
            .vector_mask
            .is_none()
    );
    assert_eq!(e.document().unwrap().paths().len(), 1);
}
#[test]
fn movement_link_toggle_project_storage_and_rotation_preserve_effective_placement() {
    let mut e = new();
    let layer = json!({"name":"Background"});
    run(
        &mut e,
        "layer.set_position",
        json!({"layer":layer,"x":4,"y":0}),
    );
    assert_eq!(alpha(e.document().unwrap(), 12, 6), 65535);
    assert_eq!(alpha(e.document().unwrap(), 5, 6), 0);
    run(
        &mut e,
        "layer.vector_mask.set",
        json!({"layer":layer,"linked":false}),
    );
    run(
        &mut e,
        "layer.set_position",
        json!({"layer":layer,"x":0,"y":0}),
    );
    assert_eq!(alpha(e.document().unwrap(), 12, 6), 65535);
    assert_eq!(alpha(e.document().unwrap(), 5, 6), 0);
    let before = e.document().unwrap().export_pixels().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("mask.strata");
    let target = e.grant_write(&file, false, Source::User).unwrap();
    run(&mut e, "file.save_as", json!({"resource":target}));
    let r = e.grant_read(&file, Source::User).unwrap();
    run(&mut e, "file.open", json!({"resource":r}));
    assert_eq!(e.document().unwrap().export_pixels().unwrap(), before);
    let mut archive = e.document().unwrap().to_archive().unwrap();
    assert_eq!(archive.metadata["version"], 14);
    archive.metadata["version"] = json!(13);
    assert!(Document::from_archive(archive).is_err());
    run(
        &mut e,
        "image.orient",
        json!({"operation":"flip_horizontal"}),
    );
    assert_eq!(alpha(e.document().unwrap(), 7, 6), 65535);
    assert_eq!(alpha(e.document().unwrap(), 14, 6), 0);
    run(&mut e, "edit.undo", json!({}));
    assert_eq!(e.document().unwrap().export_pixels().unwrap(), before);
    let revision = e.revision();
    assert!(
        e.execute(
            e.request(Source::User),
            "path.delete",
            json!({"path":{"name":"Window"}}),
            |_| Ok(())
        )
        .is_err()
    );
    assert_eq!(e.revision(), revision);
}

#[test]
fn independently_stored_psd_mask_retains_its_geometry_and_a_matching_saved_binding() {
    let mut e = new();
    let layer = json!({"name":"Background"});
    run(
        &mut e,
        "layer.set_position",
        json!({"layer":layer,"x":3,"y":1}),
    );
    run(
        &mut e,
        "layer.vector_mask.set",
        json!({"layer":layer,"linked":false,"inverted":true}),
    );
    let expected = e.document().unwrap().export_pixels().unwrap();
    let t = tempfile::tempdir().unwrap();
    let file = t.path().join("mask.psd");
    let w = e.grant_write(&file, false, Source::User).unwrap();
    run(&mut e, "file.export_psd", json!({"resource":w}));
    let read = e.grant_read(&file, Source::User).unwrap();
    run(
        &mut e,
        "file.open",
        json!({"resource":read,"discard_changes":true}),
    );
    let doc = e.document().unwrap();
    assert_eq!(doc.paths().len(), 1);
    let m = doc.layers()[0].info().vector_mask.as_ref().unwrap();
    assert_eq!(m.path, doc.paths()[0].id);
    assert!(!m.linked);
    assert!(m.inverted);
    assert_eq!(doc.export_pixels().unwrap(), expected);
    run(
        &mut e,
        "path.update",
        json!({"path":{"name":"Window"},"anchors":[{"position":[0,0]},{"position":[1,0]},{"position":[1,1]}]}),
    );
    assert_ne!(e.document().unwrap().export_pixels().unwrap(), expected);
    let mut raw = std::fs::read(&file).unwrap();
    // An external editor changed only the standard vmsk, leaving the saved
    // work path and hint stale. The new standard geometry is authoritative.
    let mut external = raw.clone();
    let tag = external.windows(8).position(|s| s == b"8BIMvmsk").unwrap();
    let first_knot_x = tag + 12 + 8 + 26 * 3 + 2 + 8 + 4;
    external[first_knot_x..first_knot_x + 4].copy_from_slice(&(0i32).to_be_bytes());
    std::fs::write(&file, external).unwrap();
    let read = e.grant_read(&file, Source::User).unwrap();
    run(
        &mut e,
        "file.open",
        json!({"resource":read,"discard_changes":true}),
    );
    assert_eq!(e.document().unwrap().paths().len(), 2);
    let magic = raw.windows(8).position(|s| s == b"StrataVM").unwrap();
    raw[magic] = b'X';
    std::fs::write(&file, raw).unwrap();
    let read = e.grant_read(&file, Source::User).unwrap();
    run(
        &mut e,
        "file.open",
        json!({"resource":read,"discard_changes":true}),
    );
    let doc = e.document().unwrap();
    assert_eq!(doc.paths().len(), 2);
    assert_ne!(
        doc.layers()[0].info().vector_mask.as_ref().unwrap().path,
        doc.paths()[0].id
    );
    assert_eq!(doc.export_pixels().unwrap(), expected);
}

#[test]
fn shape_color_is_editable_and_vector_group_and_adjustment_masks_affect_only_their_regions() {
    let mut e = new();
    run(
        &mut e,
        "layer.vector_mask.remove",
        json!({"layer":{"name":"Background"}}),
    );
    run(
        &mut e,
        "layer.fill.solid",
        json!({"name":"Shape","color":[65535,0,0,65535]}),
    );
    run(
        &mut e,
        "layer.vector_mask.set",
        json!({"layer":{"name":"Shape"},"path":{"name":"Window"}}),
    );
    let expected = e.document().unwrap().export_pixels().unwrap();
    let d = tempfile::tempdir().unwrap();
    let f = d.path().join("shape.psd");
    let w = e.grant_write(&f, false, Source::User).unwrap();
    run(&mut e, "file.export_psd", json!({"resource":w}));
    let r = e.grant_read(&f, Source::User).unwrap();
    run(
        &mut e,
        "file.open",
        json!({"resource":r,"discard_changes":true}),
    );
    let doc = e.document().unwrap();
    assert!(matches!(
        doc.layers()[1].info().kind,
        strata_core::LayerKind::Solid {
            color: [65535, 0, 0, 65535]
        }
    ));
    assert_eq!(doc.export_pixels().unwrap(), expected);
    run(
        &mut e,
        "layer.fill.solid.set",
        json!({"layer":{"name":"Shape"},"color":[0,65535,0,65535]}),
    );
    let recolored = e.document().unwrap().export_pixels().unwrap();
    assert_ne!(recolored, expected);
    run(
        &mut e,
        "layer.group",
        json!({"name":"Folder","layers":[{"name":"Shape"}]}),
    );
    run(
        &mut e,
        "layer.vector_mask.set",
        json!({"layer":{"name":"Folder"},"path":{"name":"Window"},"inverted":true}),
    );
    let PixelBuffer::Rgba16(p) = e.document().unwrap().export_pixels().unwrap() else {
        panic!()
    };
    assert_eq!(
        &p[(20 * 5 + 5) * 4..(20 * 5 + 5) * 4 + 4],
        &[12345, 23000, 55000, 65535]
    );
    run(&mut e, "edit.undo", json!({}));
    assert_eq!(e.document().unwrap().export_pixels().unwrap(), recolored);
}

#[test]
fn mask_honors_layer_and_ancestor_locks_and_same_binding_is_idempotent() {
    let mut e = new();
    let rev = e.revision();
    let layer = json!({"name":"Background"});
    let path = json!({"name":"Window"});
    let result = run(
        &mut e,
        "layer.vector_mask.set",
        json!({"layer":layer,"path":path}),
    );
    assert_eq!(result["changed"], false);
    assert_eq!(e.revision(), rev);
    run(
        &mut e,
        "layer.group",
        json!({"name":"Locked parent","layers":[layer]}),
    );
    run(
        &mut e,
        "layer.set_locks",
        json!({"layer":{"name":"Locked parent"},"locks":{"all":true}}),
    );
    let before = e.document().unwrap().to_archive().unwrap();
    for (id, args) in [
        (
            "layer.vector_mask.set",
            json!({"layer":layer,"inverted":true}),
        ),
        ("layer.vector_mask.remove", json!({"layer":layer})),
        ("path.split", json!({"path":path,"segment":0,"t":0.5})),
        (
            "path.update",
            json!({"path":path,"anchors":[{"position":[2,3]},{"position":[7,7]}]}),
        ),
    ] {
        assert!(
            e.execute(e.request(Source::User), id, args, |_| Ok(()))
                .is_err(),
            "{id}"
        );
    }
    assert!(e.document().unwrap().to_archive().unwrap() == before);
}

#[test]
fn group_affine_transform_moves_mask_once_and_preserves_independent_work_path() {
    let mut e = new();
    let layer = json!({"name":"Background"});
    let path = json!({"name":"Window"});
    run(
        &mut e,
        "layer.group",
        json!({"name":"Group","layers":[layer]}),
    );
    let original = run(&mut e, "path.get", json!({"path":path}));
    run(
        &mut e,
        "transform.apply",
        json!({"layer":{"name":"Group"},"scope":"whole_layer","geometry":{"type":"affine","values":[1,0,0,1,4,0]},"interpolation":"nearest"}),
    );
    assert_eq!(alpha(e.document().unwrap(), 12, 5), 65535);
    assert_eq!(alpha(e.document().unwrap(), 5, 5), 0);
    assert_eq!(run(&mut e, "path.get", json!({"path":path})), original);
    run(&mut e, "edit.undo", json!({}));
    assert_eq!(alpha(e.document().unwrap(), 5, 5), 65535);
    run(
        &mut e,
        "transform.apply",
        json!({"layer":layer,"scope":"whole_layer","geometry":{"type":"affine","values":[1.2,0,0,1.3,1,1]},"interpolation":"nearest"}),
    );
    assert_eq!(alpha(e.document().unwrap(), 11, 10), 65535);
    assert_eq!(alpha(e.document().unwrap(), 2, 2), 0);
    let before = e.document().unwrap().export_pixels().unwrap();
    run(
        &mut e,
        "image.resize",
        json!({"width":40,"height":32,"interpolation":"nearest"}),
    );
    assert_eq!(alpha(e.document().unwrap(), 22, 20), 65535);
    run(&mut e, "edit.undo", json!({}));
    assert_eq!(e.document().unwrap().export_pixels().unwrap(), before);
}
