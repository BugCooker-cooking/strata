use serde_json::{Value, json};
use strata_core::{Editor, Source};
use strata_pixels::PixelBuffer;
fn run(e: &mut Editor, id: &str, p: Value) -> Value {
    e.execute(e.request(Source::User), id, p, |_| Ok(()))
        .unwrap()
}
#[test]
fn clipping_uses_base_shape_instead_of_opaque_canvas_and_preserves_base_alpha() {
    let mut e = Editor::new();
    run(
        &mut e,
        "file.new",
        json!({"width":2,"height":1,"bit_depth":16}),
    );
    run(
        &mut e,
        "layer.create",
        json!({"name":"base","fill":[20000,20000,20000,32768]}),
    );
    run(
        &mut e,
        "select.marquee",
        json!({"x":0,"y":0,"width":1,"height":1}),
    );
    run(
        &mut e,
        "layer.mask.add",
        json!({"layer":{"name":"base"},"source":"from_selection"}),
    );
    let clip = run(
        &mut e,
        "layer.fill.solid",
        json!({"name":"Clip","color":[65535,0,0,65535]}),
    )["layer_id"]
        .clone();
    run(
        &mut e,
        "layer.set_clipping",
        json!({"layer":{"id":clip},"clipped":true}),
    );
    let PixelBuffer::Rgba16(p) = e.document().unwrap().export_pixels().unwrap() else {
        panic!()
    };
    assert!(p[0] > 60000);
    assert!(p[1] < 10);
    assert!(p[2] < 10);
    assert!(p[3].abs_diff(32768) <= 1);
    assert_eq!(&p[4..8], &[0, 0, 0, 0]);
    let archive = e.document().unwrap().to_archive().unwrap();
    let reopened = strata_core::Document::from_archive(archive.clone()).unwrap();
    assert_eq!(reopened.to_archive().unwrap(), archive);
    assert_eq!(reopened.export_pixels().unwrap(), PixelBuffer::Rgba16(p));
}
#[test]
fn clipping_chain_isolated_from_lower_background_and_respects_sibling_groups() {
    let mut e = Editor::new();
    run(
        &mut e,
        "file.new",
        json!({"width":1,"height":1,"bit_depth":16,"background":[0,0,65535,65535]}),
    );
    run(
        &mut e,
        "layer.create",
        json!({"name":"base","fill":[0,65535,0,32768]}),
    );
    run(
        &mut e,
        "layer.create",
        json!({"name":"Clip","fill":[65535,0,0,65535]}),
    );
    run(
        &mut e,
        "layer.set_clipping",
        json!({"layer":{"name":"Clip"},"clipped":true}),
    );
    let PixelBuffer::Rgba16(p) = e.document().unwrap().export_pixels().unwrap() else {
        panic!()
    };
    assert!(p[0] > 40000);
    assert!(p[2] > 40000);
    assert!(p[1] < 10);
    assert_eq!(p[3], 65535);
    let before = e.document().unwrap().to_archive().unwrap();
    assert!(
        e.execute(
            e.request(Source::User),
            "layer.reparent",
            json!({"layer":{"name":"base"},"parent":{"name":"Clip"}}),
            |_| Ok(())
        )
        .is_err()
    );
    assert_eq!(e.document().unwrap().to_archive().unwrap(), before);
}
