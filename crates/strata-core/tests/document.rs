use strata_core::{Document, DocumentCandidate, LayerRef};
use strata_pixels::{PixelBuffer, PixelValue};

fn candidate() -> DocumentCandidate {
    DocumentCandidate::from_image(
        2,
        1,
        PixelBuffer::Rgba16(vec![1, 32768, 32769, 0, 65535, 2, 3, 65535]),
        None,
        "sample".into(),
    )
    .unwrap()
}

#[test]
fn document_ids_survive_snapshot_and_import_is_lossless() {
    let doc = candidate().into_document();
    let snapshot = doc.clone();
    assert_eq!(doc.id(), snapshot.id());
    assert_eq!(doc.layers()[0].id(), snapshot.layers()[0].id());
    assert_eq!(
        doc.layers()[0].pixels().pixel(0, 0).unwrap(),
        PixelValue::Rgba16([1, 32768, 32769, 0])
    );
    assert_eq!(doc.bit_depth(), 16);
    assert_eq!((doc.frame().width, doc.frame().height), (2, 1));
    assert!(!doc.working_icc().is_empty());
    assert!(doc.source_icc().is_none());
}

#[test]
fn invalid_candidate_does_not_change_previous_document() {
    let doc = candidate().into_document();
    let id = doc.id();
    assert!(
        DocumentCandidate::from_image(2, 1, PixelBuffer::Rgba16(vec![0; 7]), None, "bad".into())
            .is_err()
    );
    assert_eq!(doc.id(), id);
}

#[test]
fn names_resolve_only_unique_matches() {
    let doc: Document = candidate().into_document();
    assert_eq!(
        doc.resolve(&LayerRef::Name("sample".into())).unwrap(),
        doc.layers()[0].id()
    );
    assert!(doc.resolve(&LayerRef::Name("missing".into())).is_err());
}

#[test]
fn invalid_icc_fallback_keeps_original_bytes() {
    let doc = DocumentCandidate::from_image(
        1,
        1,
        PixelBuffer::Rgba8(vec![1, 2, 3, 0]),
        Some(vec![1, 2, 3]),
        "bad icc".into(),
    )
    .unwrap()
    .into_document();
    assert!(doc.profile_fallback());
    assert_eq!(doc.source_icc(), Some([1, 2, 3].as_slice()));
    assert_eq!(
        doc.layers()[0].pixels().pixel(0, 0).unwrap(),
        PixelValue::Rgba8([1, 2, 3, 0])
    );
}

#[test]
fn archive_validates_duplicate_ids_orphan_tiles_and_opacity() {
    let doc = candidate().into_document();
    let archive = doc.to_archive().unwrap();
    let mut duplicate = archive.clone();
    let layer = duplicate.metadata["layers"][0].clone();
    duplicate.metadata["layers"]
        .as_array_mut()
        .unwrap()
        .push(layer);
    assert!(Document::from_archive(duplicate).is_err());
    let mut orphan = archive.clone();
    orphan.tiles[0].layer = "missing".into();
    assert!(Document::from_archive(orphan).is_err());
    let mut invalid = archive;
    invalid.metadata["layers"][0]["opacity"] = serde_json::json!(2.0);
    assert!(Document::from_archive(invalid).is_err());
}
