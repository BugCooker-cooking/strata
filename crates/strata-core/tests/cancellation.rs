use serde_json::json;
use strata_core::{DocumentError, Editor, Source, composite::Compositor};
use strata_pixels::TileCoord;
#[test]
fn cancellation_reaches_nested_composition_and_export_without_caching_partial_tiles() {
    let mut e = Editor::new();
    for (id, params) in [
        ("file.new", json!({"width":520,"height":32,"bit_depth":16})),
        ("layer.create_group", json!({"name":"group"})),
        (
            "layer.fill.gradient",
            json!({"name":"gradient","start":[10000,20000,30000,65535],"end":[30000,40000,50000,65535]}),
        ),
        (
            "layer.add_tonal_adjustment",
            json!({"name":"curve","adjustment":{"type":"curves","master":[[0,0],[0.5,0.7],[1,1]]}}),
        ),
    ] {
        e.execute(e.request(Source::User), id, params, |_| Ok(()))
            .unwrap();
    }
    let doc = e.document().unwrap();
    let state = doc.state_id();
    let mut compositor = Compositor::new(4);
    let visits = std::cell::Cell::new(0);
    let result = compositor.tile_prefix_cancellable(
        doc,
        TileCoord::new(0, 0).unwrap(),
        doc.layers().len(),
        &|| {
            visits.set(visits.get() + 1);
            visits.get() > 8
        },
    );
    assert!(matches!(result, Err(DocumentError::Cancelled)));
    assert_eq!(compositor.cached_tiles(), 0);
    assert!(matches!(
        doc.export_pixels_cancellable(&|| true),
        Err(DocumentError::Cancelled)
    ));
    let baseline = doc.export_pixels().unwrap();
    let export_visits = std::cell::Cell::new(0);
    assert!(matches!(
        doc.export_pixels_cancellable(&|| {
            export_visits.set(export_visits.get() + 1);
            export_visits.get() > 10
        }),
        Err(DocumentError::Cancelled)
    ));
    assert!(
        doc.export_pixels().unwrap() == baseline,
        "cancelled export must not change pixels"
    );
    assert_eq!(doc.state_id(), state);
}
