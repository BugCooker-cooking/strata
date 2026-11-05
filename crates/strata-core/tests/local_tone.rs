use serde_json::{Value, json};
use strata_core::{ClipboardImage, Document, Editor, Source};
use strata_pixels::{PixelBuffer, PixelFormat};
fn run(e: &mut Editor, id: &str, p: Value) -> Value {
    e.execute(e.request(Source::User), id, p, |_| Ok(()))
        .unwrap()
}
fn pixels(doc: &Document) -> Vec<u16> {
    match doc.export_pixels().unwrap() {
        PixelBuffer::Rgba16(v) => v,
        PixelBuffer::Rgba8(v) => v.into_iter().map(u16::from).collect(),
    }
}
fn source(
    width: u32,
    height: u32,
    depth: u8,
    mut pixel: impl FnMut(u32, u32) -> [u16; 4],
) -> Editor {
    let mut e = Editor::new();
    run(
        &mut e,
        "file.new",
        json!({"width":width,"height":height,"bit_depth":depth}),
    );
    let mut values = Vec::new();
    for y in 0..height {
        for x in 0..width {
            values.extend(pixel(x, y));
        }
    }
    let buffer = if depth == 16 {
        PixelBuffer::Rgba16(values)
    } else {
        PixelBuffer::Rgba8(values.into_iter().map(|v| (v / 257) as u8).collect())
    };
    let image = ClipboardImage::new(
        buffer,
        width,
        height,
        0,
        0,
        e.document().unwrap().working_icc().to_vec(),
        &e.document().unwrap().memory_pool(),
    )
    .unwrap();
    let token = e.grant_clipboard_image(image, Source::User).unwrap();
    run(&mut e, "edit.paste_in_place", json!({"resource":token}));
    e
}
fn add(e: &mut Editor, settings: Value) -> Value {
    run(
        e,
        "layer.add_tonal_adjustment",
        json!({"name":"Local tone","adjustment":settings}),
    )["layer_id"]
        .clone()
}
#[test]
fn neutral_local_tone_is_exact_and_point_reference_preserves_alpha() {
    for depth in [8, 16] {
        let mut e = source(16, 4, depth, |x, y| {
            let v = if x < 8 { 16384 } else { 49151 };
            [v, v, v, if y == 3 { 32768 } else { 65535 }]
        });
        let before = pixels(e.document().unwrap());
        let layer = add(&mut e, json!({"type":"shadows_highlights"}));
        assert!(pixels(e.document().unwrap()) == before);
        run(
            &mut e,
            "adjust.tonal.set",
            json!({"layer":{"id":layer},"adjustment":{"type":"shadows_highlights","shadows":100,"highlights":100,"radius":0}}),
        );
        let after = pixels(e.document().unwrap());
        for y in 0..4 {
            for x in 0..16 {
                let i = (y * 16 + x) * 4;
                assert_eq!(after[i + 3], before[i + 3]);
                if x < 8 {
                    assert!(after[i] > before[i]);
                } else {
                    assert!(after[i] < before[i]);
                }
            }
        }
        if depth == 16 {
            let sample = PixelBuffer::Rgba16(vec![
                before[0], before[1], before[2], 65535, after[0], after[1], after[2], 65535,
            ]);
            let linear =
                strata_color::to_linear_p3(&sample, 2, 1, e.document().unwrap().working_icc())
                    .unwrap();
            assert!((linear[1][0] / linear[0][0] - 2.0).abs() < 0.003);
        }
        let archive = e.document().unwrap().to_archive().unwrap();
        let reopened = Document::from_archive(archive).unwrap();
        assert!(pixels(&reopened) == after);
        run(&mut e, "edit.undo", json!({}));
        assert!(pixels(e.document().unwrap()) == before);
    }
}
#[test]
fn identical_center_samples_respond_to_different_neighborhoods() {
    let mut dark = source(32, 16, 16, |_, _| [12000, 12000, 12000, 65535]);
    let mut surrounded = source(32, 16, 16, |x, y| {
        if x == 16 && y == 8 {
            [12000, 12000, 12000, 65535]
        } else {
            [55000, 55000, 55000, 65535]
        }
    });
    let settings = json!({"type":"shadows_highlights","shadows":100,"radius":12});
    add(&mut dark, settings.clone());
    add(&mut surrounded, settings);
    let a = pixels(dark.document().unwrap());
    let b = pixels(surrounded.document().unwrap());
    let i = (8 * 32 + 16) * 4;
    assert!(
        a[i] > b[i] + 1000,
        "local estimate was replaced with a global point curve"
    );
    assert!(b[i].abs_diff(12000) <= 3);
}
fn shifted(doc: &Document, dx: i64, dy: i64) -> Document {
    let mut archive = doc.to_archive().unwrap();
    let frame = &mut archive.metadata["frame"];
    frame["x"] = json!(frame["x"].as_i64().unwrap() + dx);
    frame["y"] = json!(frame["y"].as_i64().unwrap() + dy);
    for layer in archive.metadata["layers"].as_array_mut().unwrap() {
        layer["x"] = json!(layer["x"].as_i64().unwrap() + dx);
        layer["y"] = json!(layer["y"].as_i64().unwrap() + dy);
        if layer["mask"].is_object() {
            let mask = &mut layer["mask"];
            mask["anchor_x"] = json!(mask["anchor_x"].as_i64().unwrap() + dx);
            mask["anchor_y"] = json!(mask["anchor_y"].as_i64().unwrap() + dy);
            mask["layout"]["bounds"]["x"] =
                json!(mask["layout"]["bounds"]["x"].as_i64().unwrap() + dx);
            mask["layout"]["bounds"]["y"] =
                json!(mask["layout"]["bounds"]["y"].as_i64().unwrap() + dy);
        }
    }
    Document::from_archive(archive).unwrap()
}
#[test]
fn cross_tile_halo_is_translation_invariant_and_copy_crop_keeps_canonical_artboard() {
    let mut e = source(520, 8, 16, |x, _| {
        let v = if (x / 20).is_multiple_of(2) {
            12000
        } else {
            50000
        };
        [v, v, v, 65535]
    });
    add(
        &mut e,
        json!({"type":"shadows_highlights","shadows":75,"highlights":35,"radius":16}),
    );
    let before = pixels(e.document().unwrap());
    let moved = shifted(e.document().unwrap(), -129, -7);
    let after = pixels(&moved);
    let difference = before
        .iter()
        .zip(&after)
        .map(|(a, b)| a.abs_diff(*b))
        .max()
        .unwrap();
    assert!(
        difference <= 1,
        "tile boundary changed local tone by {difference}"
    );
    run(
        &mut e,
        "select.marquee",
        json!({"x":250,"y":0,"width":20,"height":8}),
    );
    let copied = run(&mut e, "edit.copy_merged", json!({}));
    let image = e
        .clipboard_image(
            copied["clipboard_resource"].as_str().unwrap(),
            &Source::User,
        )
        .unwrap();
    let PixelBuffer::Rgba16(actual) = image.pixels() else {
        panic!()
    };
    for y in 0..8 {
        for x in 0..20 {
            for c in 0..4 {
                assert_eq!(
                    actual[(y * 20 + x) * 4 + c],
                    before[(y * 520 + x + 250) * 4 + c]
                );
            }
        }
    }
    run(&mut e, "select.none", json!({}));
    run(&mut e, "layer.merge_visible", json!({}));
    assert!(
        pixels(e.document().unwrap())
            .iter()
            .zip(before)
            .all(|(a, b)| a.abs_diff(b) <= 3)
    );
}
fn over(bg: [u16; 4], foreground: [u16; 4], opacity: f64, icc: &[u8]) -> [u16; 4] {
    let buffer = PixelBuffer::Rgba16(bg.into_iter().chain(foreground).collect());
    let linear = strata_color::to_linear_p3(&buffer, 2, 1, icc).unwrap();
    let result = strata_core::composite::blend(
        linear[0].map(f64::from),
        linear[1].map(f64::from),
        strata_core::BlendMode::Normal,
        opacity,
    );
    let PixelBuffer::Rgba16(values) =
        strata_color::from_linear_p3(&[result.map(|v| v as f32)], PixelFormat::Rgba16, icc)
            .unwrap()
    else {
        panic!()
    };
    values.try_into().unwrap()
}
#[test]
fn pass_through_group_and_ungroup_keep_the_same_local_input() {
    let mut e = source(32, 8, 16, |x, _| {
        let v = 10000 + x as u16 * 1000;
        [v, v, v, 65535]
    });
    run(&mut e, "layer.create_group", json!({"name":"tone group"}));
    add(
        &mut e,
        json!({"type":"shadows_highlights","shadows":70,"radius":8}),
    );
    let before = pixels(e.document().unwrap());
    run(
        &mut e,
        "layer.ungroup",
        json!({"layer":{"name":"tone group"}}),
    );
    assert!(pixels(e.document().unwrap()) == before);
}

#[test]
fn pass_through_group_used_as_clipping_base_has_an_independent_local_input() {
    let mut e = Editor::new();
    run(
        &mut e,
        "file.new",
        json!({"width":16,"height":8,"bit_depth":16,"background":[50000,50000,50000,65535]}),
    );
    run(&mut e, "layer.create_group", json!({"name":"base group"}));
    run(
        &mut e,
        "layer.create",
        json!({"name":"dark foreground","fill":[12000,12000,12000,32768]}),
    );
    add(
        &mut e,
        json!({"type":"shadows_highlights","shadows":100,"radius":12}),
    );
    run(
        &mut e,
        "layer.select",
        json!({"layer":{"name":"Background"}}),
    );
    run(
        &mut e,
        "layer.create",
        json!({"name":"clip","fill":[0,0,0,0]}),
    );
    run(
        &mut e,
        "layer.move_order",
        json!({"layer":{"name":"clip"},"index":2}),
    );
    run(
        &mut e,
        "layer.set_clipping",
        json!({"layer":{"name":"clip"},"clipped":true}),
    );
    let passthrough = pixels(e.document().unwrap());
    run(
        &mut e,
        "layer.set_pass_through",
        json!({"layer":{"name":"base group"},"pass_through":false}),
    );
    let isolated = pixels(e.document().unwrap());
    assert!(
        passthrough
            .iter()
            .zip(isolated)
            .all(|(a, b)| a.abs_diff(b) <= 2),
        "clipping-base pass-through inherited the external backdrop"
    );
}
#[test]
fn isolated_group_opacity_and_clipped_half_alpha_use_their_own_input_surfaces() {
    for clipped in [false, true] {
        let mut e = Editor::new();
        let background = [50000, 50000, 50000, 65535];
        run(
            &mut e,
            "file.new",
            json!({"width":8,"height":8,"bit_depth":16,"background":background}),
        );
        let root = e.document().unwrap().layers()[0].id();
        if !clipped {
            run(&mut e, "layer.create_group", json!({"name":"isolated"}));
            run(
                &mut e,
                "layer.set_pass_through",
                json!({"layer":{"name":"isolated"},"pass_through":false}),
            );
        }
        run(
            &mut e,
            "layer.create",
            json!({"name":"foreground","fill":[12000,18000,22000,if clipped{32768}else{65535}]}),
        );
        let tone = add(
            &mut e,
            json!({"type":"shadows_highlights","shadows":100,"radius":8}),
        );
        if clipped {
            run(
                &mut e,
                "layer.set_clipping",
                json!({"layer":{"id":tone},"clipped":true}),
            );
        } else {
            run(
                &mut e,
                "layer.set_opacity",
                json!({"layer":{"name":"isolated"},"opacity":0.5}),
            );
        }
        let actual = pixels(e.document().unwrap());
        let icc = e.document().unwrap().working_icc().to_vec();
        run(
            &mut e,
            "layer.set_visibility",
            json!({"layer":{"id":root},"visible":false}),
        );
        if !clipped {
            run(
                &mut e,
                "layer.set_opacity",
                json!({"layer":{"name":"isolated"},"opacity":1}),
            );
        }
        let foreground = pixels(e.document().unwrap());
        let expected = over(
            background,
            foreground[..4].try_into().unwrap(),
            if clipped { 1.0 } else { 0.5 },
            &icc,
        );
        for c in 0..4 {
            assert!(
                actual[c].abs_diff(expected[c]) <= 4,
                "clipped={clipped} channel {c}: {} != {}",
                actual[c],
                expected[c]
            );
        }
    }
}
#[test]
fn spatial_budget_failure_and_row_cancellation_preserve_committed_state() {
    use strata_pixels::memory::{MemoryLimits, MemoryPool};
    let mut e = Editor::with_memory_pool(MemoryPool::new(MemoryLimits {
        cpu_bytes: 16 * 1024 * 1024,
        gpu_bytes: 0,
        disk_bytes: 64 * 1024 * 1024,
    }));
    run(
        &mut e,
        "file.new",
        json!({"width":32,"height":16,"bit_depth":16,"background":[12000,12000,12000,65535]}),
    );
    let state = e.document().unwrap().state_id();
    let history = e.snapshot().history.len();
    assert!(e.execute(e.request(Source::User),"layer.add_tonal_adjustment",json!({"name":"too large","adjustment":{"type":"shadows_highlights","shadows":100,"radius":256}}),|p|p.document().unwrap().export_pixels().map(|_|()).map_err(|e|e.to_string())).is_err());
    assert_eq!(e.document().unwrap().state_id(), state);
    assert_eq!(e.snapshot().history.len(), history);
    let mut e = source(32, 16, 16, |_, _| [12000, 12000, 12000, 65535]);
    add(
        &mut e,
        json!({"type":"shadows_highlights","shadows":80,"radius":30}),
    );
    let doc = e.document().unwrap();
    let mut compositor = strata_core::composite::Compositor::new(1);
    let visits = std::cell::Cell::new(0);
    let result = compositor.tile_prefix_cancellable(
        doc,
        strata_pixels::TileCoord::new(0, 0).unwrap(),
        doc.layers().len(),
        &|| {
            visits.set(visits.get() + 1);
            visits.get() > 25
        },
    );
    assert!(matches!(result, Err(strata_core::DocumentError::Cancelled)));
    assert_eq!(compositor.cached_tiles(), 0);
}

#[test]
fn transparent_hidden_rgb_does_not_pollute_local_estimate_and_masks_protect_source() {
    for depth in [8, 16] {
        let mut e = source(16, 8, depth, |x, _| {
            if x < 8 {
                [12000, 12000, 12000, 65535]
            } else {
                [65535, 0, 65535, 0]
            }
        });
        let original = e
            .document()
            .unwrap()
            .layers()
            .last()
            .unwrap()
            .pixels()
            .clone();
        run(
            &mut e,
            "select.marquee",
            json!({"x":0,"y":0,"width":4,"height":8}),
        );
        add(
            &mut e,
            json!({"type":"shadows_highlights","shadows":100,"radius":18}),
        );
        let adjusted = pixels(e.document().unwrap());
        let max = if depth == 16 { 65535.0 } else { 255.0 };
        // Identical opaque pixels receive the same estimate at the transparent
        // edge; mask coverage alone protects the other half of the opaque area.
        assert!(adjusted[0] as f64 > 12000.0 / 65535.0 * max + 10.0);
        assert!(adjusted[7 * 4] as f64 <= 12000.0 / 65535.0 * max + 1.0);
        assert_eq!(adjusted[8 * 4 + 3], 0);
        for coord in original.tile_coords() {
            assert_eq!(
                original.tile_version(*coord),
                e.document().unwrap().layers()[1]
                    .pixels()
                    .tile_version(*coord)
            );
        }
        run(
            &mut e,
            "layer.mask.remove",
            json!({"layer":{"name":"Local tone"}}),
        );
        let all = pixels(e.document().unwrap());
        assert!(
            all[3 * 4].abs_diff(all[7 * 4]) <= 1,
            "transparent hidden RGB darkened the local average"
        );
        let root = tempfile::tempdir().unwrap();
        let store = strata_io::recovery::RecoveryStore::new(root.path()).unwrap();
        let info = e
            .checkpoint_recovery(&store, "local tone")
            .unwrap()
            .unwrap();
        let mut recovered = Editor::new();
        let read = recovered
            .grant_read(&store.path(&info.session_id).unwrap(), Source::User)
            .unwrap();
        run(&mut recovered, "file.recover", json!({"resource":read}));
        assert!(pixels(recovered.document().unwrap()) == all);
    }
}

#[test]
fn neighbor_only_source_change_invalidates_cached_local_tone() {
    let mut e = source(520, 8, 16, |_, _| [12000, 12000, 12000, 65535]);
    run(
        &mut e,
        "layer.fill.solid",
        json!({"name":"neighbor","color":[55000,55000,55000,65535]}),
    );
    run(
        &mut e,
        "layer.set_position",
        json!({"layer":{"name":"neighbor"},"x":256,"y":0}),
    );
    add(
        &mut e,
        json!({"type":"shadows_highlights","shadows":100,"radius":24}),
    );
    let mut cached = strata_core::composite::Compositor::new(2);
    let coord = strata_pixels::TileCoord::new(0, 0).unwrap();
    let before = cached.tile(e.document().unwrap(), coord).unwrap()[254][0];
    run(
        &mut e,
        "layer.fill.solid.set",
        json!({"layer":{"name":"neighbor"},"color":[12000,12000,12000,65535]}),
    );
    let after = cached.tile(e.document().unwrap(), coord).unwrap()[254][0];
    let fresh = strata_core::composite::Compositor::new(1)
        .tile(e.document().unwrap(), coord)
        .unwrap()[254][0];
    assert_eq!(after, fresh);
    assert!(
        after > before,
        "neighbor-only metadata change reused a stale local estimate"
    );
    for parameters in [
        json!({"radius":257}),
        json!({"shadows_width":0}),
        json!({"highlights":101}),
    ] {
        let mut settings = json!({"type":"shadows_highlights"});
        settings
            .as_object_mut()
            .unwrap()
            .extend(parameters.as_object().unwrap().clone());
        let revision = e.revision();
        assert!(
            e.execute(
                e.request(Source::User),
                "adjust.tonal.set",
                json!({"layer":{"name":"Local tone"},"adjustment":settings}),
                |_| Ok(())
            )
            .is_err()
        );
        assert_eq!(e.revision(), revision);
    }
}
