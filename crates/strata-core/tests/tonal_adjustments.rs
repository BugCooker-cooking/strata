use serde_json::{Value, json};
use strata_core::{Editor, Source, TonalAdjustment};
use strata_pixels::{PixelBuffer, TileCoord};
fn run(e: &mut Editor, id: &str, p: Value) -> Value {
    e.execute(e.request(Source::User), id, p, |_| Ok(()))
        .unwrap()
}
fn editor(depth: u8, space: &str) -> Editor {
    let mut e = Editor::new();
    let max = if depth == 16 { 65535 } else { 255 };
    run(
        &mut e,
        "file.new",
        json!({"width":4,"height":2,"bit_depth":depth,"working_space":space,"background":[max/4,max/2,max*3/4,max]}),
    );
    e
}
fn values(e: &Editor) -> Vec<u16> {
    match e.document().unwrap().export_pixels().unwrap() {
        PixelBuffer::Rgba8(v) => v.into_iter().map(u16::from).collect(),
        PixelBuffer::Rgba16(v) => v,
    }
}
#[test]
fn all_neutral_adjustments_preserve_exact_8_16bit_source_samples_and_working_icc() {
    for depth in [8, 16] {
        for space in ["srgb", "display_p3"] {
            for kind in ["brightness_contrast", "levels", "curves"] {
                let mut e = editor(depth, space);
                let before = values(&e);
                let icc = e.document().unwrap().working_icc().to_vec();
                run(
                    &mut e,
                    "layer.add_tonal_adjustment",
                    json!({"name":"neutral","adjustment":{"type":kind}}),
                );
                assert_eq!(values(&e), before);
                assert_eq!(e.document().unwrap().working_icc(), icc);
            }
        }
    }
}
#[test]
fn masked_brightness_has_encoded_working_reference_and_keeps_protected_samples_and_alpha() {
    for depth in [8, 16] {
        for space in ["srgb", "display_p3"] {
            let mut e = editor(depth, space);
            let before = values(&e);
            let original = e.document().unwrap().layers()[0].pixels().clone();
            run(
                &mut e,
                "select.marquee",
                json!({"x":0,"y":0,"width":2,"height":2}),
            );
            run(
                &mut e,
                "layer.add_tonal_adjustment",
                json!({"name":"brighter","adjustment":{"type":"brightness_contrast","brightness":10}}),
            );
            let after = values(&e);
            let max = if depth == 16 { 65535.0 } else { 255.0 };
            for y in 0..2 {
                for x in 0..4 {
                    let i = (y * 4 + x) * 4;
                    for c in 0..3 {
                        let expected = if x < 2 {
                            (f64::from(before[i + c]) + max * 0.1).round()
                        } else {
                            f64::from(before[i + c])
                        };
                        assert!(
                            (f64::from(after[i + c]) - expected).abs()
                                <= if depth == 16 { 4.0 } else { 1.0 },
                            "{space} depth={depth} {x},{y},{c}"
                        );
                    }
                    assert_eq!(after[i + 3], before[i + 3]);
                }
            }
            for coord in original.tile_coords() {
                assert_eq!(
                    *original.tile(*coord).unwrap().unwrap(),
                    *e.document().unwrap().layers()[0]
                        .pixels()
                        .tile(*coord)
                        .unwrap()
                        .unwrap()
                );
            }
            run(&mut e, "edit.undo", json!({}));
            assert_eq!(values(&e), before);
        }
    }
}
#[test]
fn levels_and_curves_have_fixed_channel_math_and_keep_16bit_adjacent_codes() {
    let mut e = editor(16, "srgb");
    let before = values(&e);
    let layer=run(&mut e,"layer.add_tonal_adjustment",json!({"name":"levels","adjustment":{"type":"levels","master":{"input_black":0.1,"input_white":0.9,"gamma":2.0},"channels":[{"output_white":0.8},{},{}]}}))["layer_id"].clone();
    let after = values(&e);
    for c in 0..3 {
        let v = ((f64::from(before[c]) / 65535.0 - 0.1) / 0.8)
            .clamp(0.0, 1.0)
            .sqrt()
            * if c == 0 { 0.8 } else { 1.0 };
        assert!((f64::from(after[c]) - v * 65535.0).abs() < 5.0);
    }
    run(
        &mut e,
        "adjust.tonal.set",
        json!({"layer":{"id":layer},"adjustment":{"type":"levels","master":{"output_black":1,"output_white":0}}}),
    );
    let inverse = values(&e);
    for c in 0..3 {
        assert!(inverse[c].abs_diff(65535 - before[c]) <= 4);
    }
    run(&mut e, "edit.undo", json!({}));
    assert_eq!(values(&e), after);
    let archive = e.document().unwrap().to_archive().unwrap();
    assert_eq!(archive.metadata["version"], 6);
    let reopened = strata_core::Document::from_archive(archive.clone()).unwrap();
    assert_eq!(
        reopened.export_pixels().unwrap(),
        e.document().unwrap().export_pixels().unwrap()
    );
    let mut legacy = archive;
    legacy.metadata["version"] = json!(3);
    assert!(strata_core::Document::from_archive(legacy).is_err());
    run(&mut e, "layer.delete", json!({"layer":{"id":layer}}));
    run(
        &mut e,
        "layer.add_tonal_adjustment",
        json!({"name":"curve","adjustment":{"type":"curves","master":[[0,0],[1,0.8]]}}),
    );
    let after = values(&e);
    for c in 0..3 {
        assert!((f64::from(after[c]) - f64::from(before[c]) * 0.8).abs() < 5.0);
    }
}
#[test]
fn pchip_matches_knots_preserves_segment_bounds_and_accepts_nonmonotonic_outputs() {
    let points = [[0.0, 0.0], [0.25, 0.7], [0.5, 0.1], [0.75, 0.8], [1.0, 1.0]];
    let values = TonalAdjustment::curve_samples(&points, 1025).unwrap();
    for (i, p) in points.iter().enumerate() {
        assert!((f64::from(values[i * 256]) - p[1]).abs() < 1e-7);
    }
    for (i, pair) in points.windows(2).enumerate() {
        let low = pair[0][1].min(pair[1][1]);
        let high = pair[0][1].max(pair[1][1]);
        assert!(
            values[i * 256..=(i + 1) * 256]
                .iter()
                .all(|v| f64::from(*v) >= low - 1e-6 && f64::from(*v) <= high + 1e-6)
        );
    }
    assert!(TonalAdjustment::curve_samples(&[[0.0, 0.0], [0.0, 0.5], [1.0, 1.0]], 257).is_err());
}
#[test]
fn invalid_parameters_type_changes_locks_and_style_attempts_are_atomic() {
    let mut e = editor(16, "srgb");
    let layer = run(
        &mut e,
        "layer.add_tonal_adjustment",
        json!({"name":"levels","adjustment":{"type":"levels"}}),
    )["layer_id"]
        .clone();
    for adjustment in [
        json!({"type":"levels","master":{"input_black":0.9,"input_white":0.2}}),
        json!({"type":"curves"}),
        json!({"type":"levels","channels":[{},{},{}],"unexpected":2}),
    ] {
        let rev = e.revision();
        assert!(
            e.execute(
                e.request(Source::User),
                "adjust.tonal.set",
                json!({"layer":{"id":layer},"adjustment":adjustment}),
                |_| Ok(())
            )
            .is_err()
        );
        assert_eq!(e.revision(), rev);
    }
    run(
        &mut e,
        "layer.set_locks",
        json!({"layer":{"id":layer},"locks":{"all":true}}),
    );
    assert!(
        e.execute(
            e.request(Source::User),
            "adjust.tonal.set",
            json!({"layer":{"id":layer},"adjustment":{"type":"levels","master":{"gamma":2}}}),
            |_| Ok(())
        )
        .is_err()
    );
    run(
        &mut e,
        "layer.set_locks",
        json!({"layer":{"id":layer},"locks":{}}),
    );
    let revision = e.revision();
    let error=e.execute(e.request(Source::User),"layer.set_styles",json!({"layer":{"id":layer},"styles":{"stroke":{"color":[65535,65535,65535,65535],"size":2,"position":"outside"}}}),|_|Ok(())).unwrap_err();
    assert!(error.to_string().contains("styles require"));
    assert_eq!(e.revision(), revision);
    assert!(
        e.execute(e.request(Source::User), "edit.copy", json!({}), |_| Ok(()))
            .is_err()
    );
    let mut compositor = strata_core::composite::Compositor::new(1);
    let coord = TileCoord::new(0, 0).unwrap();
    let before = compositor.tile(e.document().unwrap(), coord).unwrap()[0];
    run(
        &mut e,
        "adjust.tonal.set",
        json!({"layer":{"id":layer},"adjustment":{"type":"levels","master":{"gamma":2}}}),
    );
    assert_ne!(
        compositor.tile(e.document().unwrap(), coord).unwrap()[0],
        before
    );
}

#[test]
fn nonnormal_neutral_adjustment_blends_with_backdrop_and_recovers_without_source_changes() {
    let mut e = editor(16, "srgb");
    let before = values(&e);
    let layer = run(
        &mut e,
        "layer.add_tonal_adjustment",
        json!({"name":"multiply curve","adjustment":{"type":"curves"}}),
    )["layer_id"]
        .clone();
    run(
        &mut e,
        "layer.set_blend_mode",
        json!({"layer":{"id":layer},"blend_mode":"multiply"}),
    );
    let darker = values(&e);
    assert!(darker[0] < before[0]);
    assert_eq!(darker[3], before[3]);
    let directory = tempfile::tempdir().unwrap();
    let store = strata_io::recovery::RecoveryStore::new(directory.path()).unwrap();
    let info = e.checkpoint_recovery(&store, "curve").unwrap().unwrap();
    let mut restored = Editor::new();
    let resource = restored
        .grant_read(&store.path(&info.session_id).unwrap(), Source::User)
        .unwrap();
    run(&mut restored, "file.recover", json!({"resource":resource}));
    assert_eq!(values(&restored), darker);
    let history = restored.snapshot().history.len();
    let revision = restored.revision();
    assert!(
        restored
            .execute(
                restored.request(Source::User),
                "layer.rasterize",
                json!({"layer":{"id":layer}}),
                |_| Ok(())
            )
            .is_err()
    );
    assert_eq!(restored.revision(), revision);
    run(
        &mut restored,
        "layer.rasterize",
        json!({"layer":{"id":layer},"bake_backdrop":true}),
    );
    assert_eq!(restored.snapshot().history.len(), history + 1);
    let baked = values(&restored);
    assert!(baked.iter().zip(darker).all(|(a, b)| a.abs_diff(b) <= 3));
}
