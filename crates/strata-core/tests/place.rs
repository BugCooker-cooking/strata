use serde_json::{Value, json};
use strata_core::{Editor, Source};
use strata_io::export::{ExportFormat, export_image};
use strata_pixels::PixelBuffer;

fn run(e: &mut Editor, id: &str, params: Value) -> Value {
    let request = e.request(Source::User);
    e.execute(request, id, params, |_| Ok(())).unwrap()
}
fn fresh(depth: u8) -> Editor {
    let mut e = Editor::new();
    run(
        &mut e,
        "file.new",
        json!({"width":4,"height":4,"bit_depth":depth}),
    );
    e
}
fn source(path: &std::path::Path, pixels: &PixelBuffer) {
    let icc = strata_color::working_profile("srgb").unwrap();
    export_image(path, ExportFormat::Png, 1, 1, pixels, &icc, false).unwrap();
}

#[test]
fn placement_converts_working_space_and_depth_with_one_exact_undo() {
    for source_depth in [8, 16] {
        for depth in [8, 16] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("placed.png");
            let pixels = if source_depth == 8 {
                PixelBuffer::Rgba8(vec![173, 89, 32, 129])
            } else {
                PixelBuffer::Rgba16(vec![32769, 50001, 7777, 32897])
            };
            source(&path, &pixels);
            let mut e = fresh(depth);
            run(&mut e, "color.convert", json!({"space":"display_p3"}));
            let before = e.snapshot();
            let document_id = e.document().unwrap().id();
            let destination_icc = e.document().unwrap().working_icc().to_vec();
            let resource = e.grant_read(&path, Source::User).unwrap();
            let result = run(
                &mut e,
                "file.place",
                json!({"resource":resource,"x":-1,"y":1}),
            );
            let after = e.snapshot();
            assert_eq!(after.history.len(), before.history.len() + 1);
            assert_eq!(after.history.last().unwrap().command, "file.place");
            let doc = e.document().unwrap();
            assert_eq!(doc.id(), document_id);
            assert_eq!(doc.bit_depth(), depth);
            assert_eq!(doc.frame().width, 4);
            assert_eq!(doc.working_icc(), destination_icc);
            let layer = doc.layers().last().unwrap();
            assert_eq!(
                serde_json::to_value(layer.id()).unwrap(),
                result["layer_id"]
            );
            assert_eq!((layer.info().x, layer.info().y), (-1, 1));
            let promoted = match (pixels, depth) {
                (PixelBuffer::Rgba8(v), 16) => {
                    PixelBuffer::Rgba16(v.into_iter().map(|v| u16::from(v) * 257).collect())
                }
                (v, _) => v,
            };
            let expected = strata_color::convert_to_profile(
                &promoted,
                1,
                1,
                &strata_color::working_profile("srgb").unwrap(),
                &destination_icc,
            )
            .unwrap();
            let expected = match (expected, depth) {
                (PixelBuffer::Rgba16(v), 8) => PixelBuffer::Rgba8(
                    v.into_iter()
                        .map(|v| ((u32::from(v) + 128) / 257) as u8)
                        .collect(),
                ),
                (v, _) => v,
            };
            let tile = layer.pixels().tiles().next().unwrap().1;
            match (tile, &expected) {
                (PixelBuffer::Rgba8(a), PixelBuffer::Rgba8(b)) => assert_eq!(&a[..4], b),
                (PixelBuffer::Rgba16(a), PixelBuffer::Rgba16(b)) => assert_eq!(&a[..4], b),
                _ => panic!("wrong depth"),
            }
            let layers = serde_json::to_value(&after.layers).unwrap();
            run(&mut e, "edit.undo", json!({}));
            assert_eq!(
                serde_json::to_value(e.snapshot().layers).unwrap(),
                serde_json::to_value(before.layers).unwrap()
            );
            run(&mut e, "edit.redo", json!({}));
            assert_eq!(serde_json::to_value(e.snapshot().layers).unwrap(), layers);
        }
    }
}

#[test]
fn placement_authorization_stale_file_and_failed_prepare_are_atomic() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("input.png");
    source(&path, &PixelBuffer::Rgba8(vec![10, 20, 30, 255]));
    let mut e = fresh(8);
    let before = serde_json::to_value(e.snapshot()).unwrap();
    let r = e.request(Source::User);
    assert!(
        e.prepare(r, "file.place", json!({"resource":path}))
            .is_err()
    );
    let resource = e.grant_read(&path, Source::User).unwrap();
    let r = e.request(Source::User);
    assert!(
        e.execute(r, "file.place", json!({"resource":resource}), |_| Err(
            "display rejected".into()
        ))
        .is_err()
    );
    assert_eq!(serde_json::to_value(e.snapshot()).unwrap(), before);
    let r = e.request(Source::User);
    assert!(
        e.prepare(
            r,
            "document.batch",
            json!({"steps":[{"command":"file.place","params":{"resource":resource}}]})
        )
        .is_err()
    );
    let r = e.request(Source::User);
    let prepared = e
        .prepare(r, "file.place", json!({"resource":resource}))
        .unwrap();
    std::fs::write(&path, b"changed after preflight").unwrap();
    assert!(e.commit(prepared).is_err());
    assert_eq!(serde_json::to_value(e.snapshot()).unwrap(), before);
    let turn = e.begin_turn().unwrap();
    let r = e.request(Source::Ai { turn_id: turn });
    assert!(
        e.prepare(r, "file.place", json!({"resource":resource}))
            .is_err()
    );
}

#[test]
fn control_snapshot_tracks_running_pause_and_takeover() {
    let mut e = fresh(8);
    assert_eq!(
        serde_json::to_value(e.snapshot()).unwrap()["control"]["kind"],
        "user"
    );
    e.begin_turn().unwrap();
    assert_eq!(
        serde_json::to_value(e.snapshot()).unwrap()["control"]["kind"],
        "running"
    );
    e.pause_turn().unwrap();
    assert_eq!(
        serde_json::to_value(e.snapshot()).unwrap()["control"]["kind"],
        "paused"
    );
    e.stop_turn();
    assert_eq!(
        serde_json::to_value(e.snapshot()).unwrap()["control"]["kind"],
        "user"
    );
}
