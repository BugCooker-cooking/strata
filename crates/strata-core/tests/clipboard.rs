use serde_json::{Value, json};
use strata_core::{ClipboardImage, Editor, Source};
use strata_pixels::{PixelBuffer, PixelValue, memory::MemoryPool};
fn run(e: &mut Editor, id: &str, p: Value) -> Value {
    e.execute(e.request(Source::User), id, p, |_| Ok(()))
        .unwrap()
}
fn editor() -> Editor {
    let mut e = Editor::new();
    run(
        &mut e,
        "file.new",
        json!({"width":8,"height":8,"bit_depth":16,"background":[32768,32769,50001,65535]}),
    );
    e
}
#[test]
fn copy_is_external_and_cut_waits_for_host_ack_then_undo_restores_pixels_only() {
    let mut e = editor();
    run(
        &mut e,
        "select.marquee",
        json!({"x":1,"y":2,"width":3,"height":4}),
    );
    let before = e.document().unwrap().to_archive().unwrap();
    let revision = e.revision();
    let history = e.snapshot().history.len();
    let copied = run(&mut e, "edit.copy", json!({}));
    let token = copied["clipboard_resource"].as_str().unwrap().to_string();
    assert_eq!(e.revision(), revision);
    assert_eq!(e.snapshot().history.len(), history);
    assert!(
        e.execute(
            e.request(Source::User),
            "edit.cut",
            json!({"resource":token}),
            |_| Ok(())
        )
        .is_err()
    );
    assert_eq!(e.document().unwrap().to_archive().unwrap(), before);
    let image = e.clipboard_image(&token, &Source::User).unwrap();
    assert_eq!(image.size(), (3, 4));
    assert_eq!(image.origin(), (1, 2));
    assert_eq!(
        image.pixels(),
        &PixelBuffer::Rgba16([32768, 32769, 50001, 65535].repeat(12))
    );
    e.confirm_clipboard_write(&token, &Source::User).unwrap();
    run(&mut e, "edit.cut", json!({"resource":token}));
    assert_eq!(
        e.document().unwrap().layers()[0]
            .pixels()
            .pixel(1, 2)
            .unwrap(),
        PixelValue::Rgba16([32768, 32769, 50001, 0])
    );
    assert_eq!(
        e.document().unwrap().layers()[0]
            .pixels()
            .pixel(0, 0)
            .unwrap(),
        PixelValue::Rgba16([32768, 32769, 50001, 65535])
    );
    run(&mut e, "edit.undo", json!({}));
    assert_eq!(e.document().unwrap().to_archive().unwrap(), before);
    assert!(e.clipboard_image(&token, &Source::User).is_ok());
    assert!(
        e.snapshot()
            .executions
            .iter()
            .any(|x| x["command"] == "edit.clipboard.write" && x["status"] == "completed")
    );
}
#[test]
fn stale_cut_source_and_cross_source_clipboard_resources_are_rejected() {
    let mut e = editor();
    let copied = run(&mut e, "edit.copy", json!({}));
    let token = copied["clipboard_resource"].as_str().unwrap().to_owned();
    e.confirm_clipboard_write(&token, &Source::User).unwrap();
    run(
        &mut e,
        "layer.set_opacity",
        json!({"layer":{"name":"Background"},"opacity":0.5}),
    );
    let before = e.revision();
    assert!(
        e.execute(
            e.request(Source::User),
            "edit.cut",
            json!({"resource":token}),
            |_| Ok(())
        )
        .is_err()
    );
    assert_eq!(e.revision(), before);
    let turn = e.begin_turn().unwrap();
    assert!(
        e.clipboard_image(&token, &Source::Ai { turn_id: turn })
            .is_err()
    );
    assert!(
        e.execute(
            e.request(Source::Ai { turn_id: turn }),
            "edit.paste",
            json!({"resource":token}),
            |_| Ok(())
        )
        .is_err()
    );
    e.stop_turn();
}
#[test]
fn paste_positions_conversion_and_via_copy_cut_have_one_undo_boundary() {
    let mut e = editor();
    let icc = strata_color::working_profile("display_p3").unwrap();
    let source = ClipboardImage::new(
        PixelBuffer::Rgba16([32768, 32769, 50001, 65535].repeat(4)),
        2,
        2,
        -4,
        5,
        icc,
        &MemoryPool::global(),
    )
    .unwrap();
    let resource = e.grant_clipboard_image(source, Source::User).unwrap();
    let layer =
        run(&mut e, "edit.paste_in_place", json!({"resource":resource}))["layer_id"].clone();
    let pasted = e.document().unwrap().layers().last().unwrap();
    assert_eq!((pasted.info().x, pasted.info().y), (-4, 5));
    assert_eq!(pasted.pixels().format().bit_depth(), 16);
    run(&mut e, "edit.undo", json!({}));
    run(&mut e, "edit.paste", json!({"resource":resource}));
    let pasted = e.document().unwrap().layers().last().unwrap();
    assert_eq!((pasted.info().x, pasted.info().y), (3, 3));
    run(&mut e, "edit.undo", json!({}));
    run(
        &mut e,
        "select.marquee",
        json!({"x":1,"y":1,"width":2,"height":2}),
    );
    let before = e.document().unwrap().to_archive().unwrap();
    for id in ["layer.via_copy", "layer.via_cut"] {
        run(&mut e, id, json!({}));
        assert_eq!(e.document().unwrap().layers().len(), 2);
        run(&mut e, "edit.undo", json!({}));
        assert_eq!(e.document().unwrap().to_archive().unwrap(), before);
    }
    let _ = layer;
}
#[test]
fn clipboard_side_effects_cannot_enter_atomic_batch_and_locks_prevent_clear() {
    let mut e = editor();
    let before = e.revision();
    assert!(e.execute(e.request(Source::User),"document.batch",json!({"steps":[{"command":"layer.create","params":{"name":"not committed"}},{"command":"edit.copy","params":{}}]}), |_|Ok(())).is_err());
    assert_eq!(e.revision(), before);
    run(
        &mut e,
        "layer.set_locks",
        json!({"layer":{"name":"Background"},"locks":{"transparency":true}}),
    );
    let before = e.document().unwrap().to_archive().unwrap();
    assert!(
        e.execute(e.request(Source::User), "edit.clear", json!({}), |_| Ok(()))
            .is_err()
    );
    assert_eq!(e.document().unwrap().to_archive().unwrap(), before);
}
