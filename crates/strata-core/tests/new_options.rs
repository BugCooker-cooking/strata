use serde_json::{Value, json};
use strata_core::{Editor, Source};
use strata_pixels::PixelValue;
fn run(e: &mut Editor, id: &str, p: Value) {
    let r = e.request(Source::User);
    e.execute(r, id, p, |_| Ok(())).unwrap();
}

#[test]
fn new_presets_resolution_background_and_space_survive_project_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("new.strata");
    let mut e = Editor::new();
    run(
        &mut e,
        "file.new",
        json!({"preset":"web_1080p","bit_depth":16,"resolution_dpi":300,"working_space":"display_p3","background":[32768,32769,50001,12345]}),
    );
    let doc = e.document().unwrap();
    assert_eq!(
        (doc.frame().width, doc.frame().height, doc.resolution_dpi()),
        (1920, 1080, 300)
    );
    assert!(doc.profile_name().contains("P3"));
    assert_eq!(doc.layers().len(), 1);
    assert_eq!(
        doc.layers()[0].pixels().pixel(0, 0).unwrap(),
        PixelValue::Rgba16([32768, 32769, 50001, 12345])
    );
    assert_eq!(
        doc.layers()[0].pixels().pixel(1919, 1079).unwrap(),
        PixelValue::Rgba16([32768, 32769, 50001, 12345])
    );
    assert_eq!(
        doc.layers()[0].pixels().pixel(1920, 1080).unwrap(),
        PixelValue::Rgba16([0; 4])
    );
    assert!(e.snapshot().history.is_empty());
    assert!(e.dirty());
    let id = doc.id();
    let archive = doc.to_archive().unwrap();
    let grant = e.grant_write(&path, false, Source::User).unwrap();
    run(&mut e, "file.save_as", json!({"resource":grant}));
    let grant = e.grant_read(&path, Source::User).unwrap();
    run(&mut e, "file.open", json!({"resource":grant}));
    assert_eq!(e.document().unwrap().id(), id);
    assert_eq!(e.document().unwrap().resolution_dpi(), 300);
    assert_eq!(e.document().unwrap().to_archive().unwrap(), archive);
    assert!(!e.dirty());
}

#[test]
fn invalid_new_options_and_failed_display_keep_previous_document() {
    let mut e = Editor::new();
    run(
        &mut e,
        "file.new",
        json!({"width":2,"height":2,"bit_depth":8}),
    );
    let before = serde_json::to_value(e.snapshot()).unwrap();
    for extra in [
        json!({"preset":"unknown"}),
        json!({"preset":"a4_300dpi"}),
        json!({"resolution_dpi":0}),
        json!({"resolution_dpi":2401}),
        json!({"resolution_dpi":72.5}),
        json!({"working_space":"cmyk"}),
        json!({"background":[256,0,0,255]}),
    ] {
        let mut args = json!({"width":2,"height":2,"bit_depth":8,"discard_changes":true});
        args.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        let r = e.request(Source::User);
        assert!(e.prepare(r, "file.new", args).is_err());
        assert_eq!(serde_json::to_value(e.snapshot()).unwrap(), before);
    }
    let r = e.request(Source::User);
    assert!(
        e.execute(
            r,
            "file.new",
            json!({"preset":"square_2048","bit_depth":8,"discard_changes":true}),
            |_| Err("display failed".into())
        )
        .is_err()
    );
    assert_eq!(serde_json::to_value(e.snapshot()).unwrap(), before);
}

#[test]
fn old_projects_default_to_72_dpi_and_transparent_new_documents_remain_sparse() {
    let mut e = Editor::new();
    run(
        &mut e,
        "file.new",
        json!({"preset":"a4_300dpi","bit_depth":8}),
    );
    assert_eq!(
        (
            e.document().unwrap().frame().width,
            e.document().unwrap().frame().height
        ),
        (2480, 3508)
    );
    assert_eq!(e.document().unwrap().resolution_dpi(), 300);
    assert_eq!(e.document().unwrap().layers()[0].pixels().tile_count(), 0);
    let mut old = e.document().unwrap().to_archive().unwrap();
    old.metadata
        .as_object_mut()
        .unwrap()
        .remove("resolution_dpi");
    old.metadata.as_object_mut().unwrap().remove("preset");
    let doc = strata_core::Document::from_archive(old).unwrap();
    assert_eq!(doc.resolution_dpi(), 72);
}
