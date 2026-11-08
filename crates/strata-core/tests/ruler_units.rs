use serde_json::{Value, json};
use strata_core::{Editor, Source};
fn run(e: &mut Editor, id: &str, p: Value) -> Value {
    e.execute(e.request(Source::User), id, p, |_| Ok(()))
        .unwrap()
}
fn fixture() -> Editor {
    let mut e = Editor::new();
    run(
        &mut e,
        "file.new",
        json!({"width":3000,"height":1500,"bit_depth":16,"resolution_dpi":300}),
    );
    e
}
fn converted(e: &mut Editor, direction: &str, pair: [f64; 2]) -> Value {
    run(
        e,
        "view.rulers.convert",
        json!({"direction":direction,"coordinates":pair}),
    )
}
#[test]
fn physical_units_and_custom_zero_match_independent_known_distances_and_reverse() {
    let mut e = fixture();
    let before = e.document().unwrap().to_archive().unwrap();
    let revision = e.revision();
    for (unit, pair, expected) in [
        ("pixels", [400., 800.], [300., 600.]),
        ("inches", [400., 800.], [1., 2.]),
        ("centimeters", [400., 800.], [2.54, 5.08]),
        ("millimeters", [400., 800.], [25.4, 50.8]),
        ("points", [400., 800.], [72., 144.]),
    ] {
        let view = run(
            &mut e,
            "view.presentation.set",
            json!({"ruler_unit":unit,"ruler_origin":[100,200]}),
        );
        let result = converted(&mut e, "canvas_to_ruler", pair);
        assert_eq!(result["metrics"]["unit"], unit);
        assert_eq!(result["metrics"]["resolution_dpi"], 300);
        assert_eq!(result["metrics"]["presentation_version"], view["version"]);
        for i in 0..2 {
            assert!((result["ruler"][i].as_f64().unwrap() - expected[i]).abs() < 1e-12)
        }
        let reverse = converted(&mut e, "ruler_to_canvas", expected);
        for i in 0..2 {
            assert!((reverse["canvas"][i].as_f64().unwrap() - pair[i]).abs() < 1e-10)
        }
    }
    assert_eq!(e.revision(), revision);
    assert!(e.document().unwrap().to_archive().unwrap() == before);
    assert_eq!(e.snapshot().history_cursor, 0);
}
#[test]
fn percentage_axes_use_independent_frame_dimensions_and_canvas_zero_is_not_frame_origin() {
    let mut e = fixture();
    run(
        &mut e,
        "image.crop",
        json!({"rect":{"x":100,"y":200,"width":600,"height":300},"pixels":"preserve"}),
    );
    let before = e.document().unwrap().to_archive().unwrap();
    run(
        &mut e,
        "view.presentation.set",
        json!({"ruler_unit":"percent","ruler_origin":[100,200]}),
    );
    let result = converted(&mut e, "canvas_to_ruler", [700., 500.]);
    assert_eq!(result["ruler"], json!([100., 100.]));
    assert_eq!(result["metrics"]["pixels_per_unit"], json!([6., 3.]));
    let negative = converted(&mut e, "ruler_to_canvas", [-50., -25.]);
    assert_eq!(negative["canvas"], json!([-200., 125.]));
    assert!(e.document().unwrap().to_archive().unwrap() == before);
    run(
        &mut e,
        "view.presentation.set",
        json!({"ruler_origin":[0,0]}),
    );
    let result = converted(&mut e, "canvas_to_ruler", [100., 200.]);
    assert!((result["ruler"][0].as_f64().unwrap() - 100. / 6.).abs() < 1e-12);
    assert!(result["ruler"][1].as_f64().unwrap() > 66.);
}
#[test]
fn same_view_is_noop_while_queries_and_prepared_settings_reject_stale_versions_without_document_changes()
 {
    let mut blank = Editor::new();
    run(
        &mut blank,
        "view.presentation.set",
        json!({"ruler_unit":"points","ruler_origin":[-12.5,30]}),
    );
    assert!(
        blank
            .execute(
                blank.request(Source::User),
                "view.rulers.convert",
                json!({"coordinates":[1,2]}),
                |_| Ok(())
            )
            .is_err()
    );
    let mut e = fixture();
    let before = e.document().unwrap().to_archive().unwrap();
    let v = run(
        &mut e,
        "view.presentation.set",
        json!({"ruler_unit":"millimeters","ruler_origin":[-12.5,30]}),
    );
    let same = run(
        &mut e,
        "view.presentation.set",
        json!({"ruler_unit":"millimeters","ruler_origin":[-12.5,30],"expected_version":v["version"]}),
    );
    assert_eq!(same["version"], v["version"]);
    let candidate = e
        .prepare(
            e.request(Source::User),
            "view.rulers.convert",
            json!({"coordinates":[1,2]}),
        )
        .unwrap();
    run(
        &mut e,
        "view.presentation.set",
        json!({"ruler_unit":"inches"}),
    );
    assert!(e.commit(candidate).is_err());
    assert!(
        e.execute(
            e.request(Source::User),
            "view.rulers.convert",
            json!({"coordinates":[1,2],"expected_version":0}),
            |_| Ok(())
        )
        .is_err()
    );
    assert!(e.document().unwrap().to_archive().unwrap() == before);
    let request = e.request(Source::User);
    run(
        &mut e,
        "file.new",
        json!({"width":32,"height":24,"resolution_dpi":72,"bit_depth":8,"discard_changes":true}),
    );
    assert!(
        e.prepare(request, "view.rulers.convert", json!({"coordinates":[1,2]}))
            .is_err()
    );
    assert_eq!(
        e.snapshot().presentation.ruler_unit,
        strata_core::RulerUnit::Inches
    );
    assert_eq!(
        converted(&mut e, "canvas_to_ruler", [59.5, 102.])["ruler"],
        json!([1., 1.])
    );
}
#[test]
fn all_invalid_units_origins_extra_fields_and_conversion_ranges_are_atomic() {
    let mut e = fixture();
    let before = e.document().unwrap().to_archive().unwrap();
    let view = run(&mut e, "view.presentation.get", json!({}));
    for p in [
        json!({"ruler_unit":"cm"}),
        json!({"ruler_unit":"unknown"}),
        json!({"ruler_origin":[1]}),
        json!({"ruler_origin":[0,0,0]}),
        json!({"ruler_origin":[1e13,0]}),
        json!({"ruler_origin":[0,0],"unexpected":true}),
    ] {
        assert!(
            e.execute(e.request(Source::User), "view.presentation.set", p, |_| Ok(
                ()
            ))
            .is_err()
        );
        assert_eq!(run(&mut e, "view.presentation.get", json!({})), view);
    }
    for p in [
        json!({"direction":"invalid","coordinates":[1,2]}),
        json!({"coordinates":[1,2,3]}),
        json!({"coordinates":[1e13,0]}),
        json!({"coordinates":[1,2],"unit":"cm"}),
    ] {
        assert!(
            e.execute(
                e.request(Source::User),
                "view.rulers.convert",
                p,
                |_| Ok(())
            )
            .is_err()
        );
    }
    run(
        &mut e,
        "view.presentation.set",
        json!({"ruler_unit":"inches"}),
    );
    assert!(
        e.execute(
            e.request(Source::User),
            "view.rulers.convert",
            json!({"direction":"ruler_to_canvas","coordinates":[1e15,0]}),
            |_| Ok(())
        )
        .is_err()
    );
    assert!(e.document().unwrap().to_archive().unwrap() == before);
}
#[test]
fn unit_settings_never_reinterpret_guides_or_undo_their_exact_canvas_positions() {
    let mut e = fixture();
    let guide = run(
        &mut e,
        "view.guides.add",
        json!({"name":"X","axis":"vertical","position":1181.1023622047244}),
    )["guide"]
        .clone();
    let before = e.document().unwrap().to_archive().unwrap();
    run(
        &mut e,
        "view.presentation.set",
        json!({"ruler_unit":"millimeters","ruler_origin":[0,0]}),
    );
    let query = converted(
        &mut e,
        "canvas_to_ruler",
        [guide["position"].as_f64().unwrap(), 0.],
    );
    assert!((query["ruler"][0].as_f64().unwrap() - 100.).abs() < 1e-12);
    assert!(e.document().unwrap().to_archive().unwrap() == before);
    run(&mut e, "edit.undo", json!({}));
    assert!(e.document().unwrap().guides().is_empty());
    assert_eq!(
        e.snapshot().presentation.ruler_unit,
        strata_core::RulerUnit::Millimeters
    );
}
