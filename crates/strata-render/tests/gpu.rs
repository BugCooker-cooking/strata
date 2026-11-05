use strata_pixels::PixelBuffer;
use strata_render::{CANVAS_BG_SRGB, RenderError, Renderer, ViewTransform, Viewport};

const VP: Viewport = Viewport {
    width: 64,
    height: 64,
    scale: 1.0,
};

fn renderer() -> Option<Renderer> {
    match Renderer::new_offscreen(VP) {
        Ok(r) => Some(r),
        Err(RenderError::NoAdapter) if std::env::var_os("STRATA_REQUIRE_GPU").is_none() => {
            eprintln!(
                "SKIPPED GPU TEST: no adapter (use STRATA_REQUIRE_GPU=1 to require hardware)"
            );
            None
        }
        Err(e) => panic!("GPU setup failed: {e}"),
    }
}

#[test]
fn camera_fingerprint_cache_is_reset_for_metadata_edits_in_the_same_document() {
    let Some(mut r) = renderer() else {
        return;
    };
    let mut editor = strata_core::Editor::new();
    for (id, args) in [
        (
            "file.new",
            serde_json::json!({"width":16,"height":16,"bit_depth":8}),
        ),
        (
            "layer.create",
            serde_json::json!({"name":"red","fill":[255,0,0,255]}),
        ),
    ] {
        let req = editor.request(strata_core::Source::Test);
        editor.execute(req, id, args, |_| Ok(())).unwrap();
    }
    let document_id = editor.document().unwrap().id();
    let version = editor.document().unwrap().layers()[1]
        .pixels()
        .tile_version(strata_pixels::TileCoord::new(0, 0).unwrap());
    let scene = r
        .prepare_document(editor.document().unwrap(), true)
        .unwrap();
    r.commit_document(scene).unwrap();
    r.render().unwrap();
    r.set_view(ViewTransform {
        zoom: 2.0,
        center_x: 8.0,
        center_y: 8.0,
    })
    .unwrap();
    r.render().unwrap();
    let before_pixels = r.read_offscreen_rgba().unwrap();
    let before = pixel(&before_pixels, 32, 32);
    let req = editor.request(strata_core::Source::Test);
    editor
        .execute(
            req,
            "layer.set_opacity",
            serde_json::json!({"layer":{"name":"red"},"opacity":0.25}),
            |_| Ok(()),
        )
        .unwrap();
    assert_eq!(editor.document().unwrap().id(), document_id);
    assert_eq!(
        editor.document().unwrap().layers()[1]
            .pixels()
            .tile_version(strata_pixels::TileCoord::new(0, 0).unwrap()),
        version
    );
    let scene = r
        .prepare_document(editor.document().unwrap(), false)
        .unwrap();
    r.commit_document(scene).unwrap();
    r.render().unwrap();
    let after_pixels = r.read_offscreen_rgba().unwrap();
    let after = pixel(&after_pixels, 32, 32);
    assert!(
        after[0] + 40 < before[0],
        "stale cached color: before={before:?}, after={after:?}"
    );
}

#[test]
fn gpu_exposure_preview_matches_neutral_reference_and_refreshes_mask_and_opacity() {
    let Some(mut renderer) = renderer() else {
        return;
    };
    let mut editor = strata_core::Editor::new();
    let mut command = |id: &str, params: serde_json::Value| {
        let request = editor.request(strata_core::Source::Test);
        editor.execute(request, id, params, |_| Ok(())).unwrap()
    };
    command(
        "file.new",
        serde_json::json!({"width":16,"height":16,"bit_depth":16,"background":[16384,16384,16384,65535]}),
    );
    command(
        "select.marquee",
        serde_json::json!({"x":0,"y":0,"width":8,"height":16}),
    );
    let layer=command("layer.add_adjustment",serde_json::json!({"name":"exposure","type":"exposure","stops":0.5,"mask":"from_selection"}))["layer_id"].clone();
    for (id, params) in [
        (
            "adjust.exposure.set",
            serde_json::json!({"layer":{"id":layer},"stops":0.5}),
        ),
        (
            "adjust.exposure.set",
            serde_json::json!({"layer":{"id":layer},"stops":1.0}),
        ),
        (
            "layer.set_opacity",
            serde_json::json!({"layer":{"id":layer},"opacity":0.5}),
        ),
        (
            "layer.mask.invert",
            serde_json::json!({"layer":{"id":layer}}),
        ),
        (
            "layer.mask.set_enabled",
            serde_json::json!({"layer":{"id":layer},"enabled":false}),
        ),
        (
            "layer.set_blend_mode",
            serde_json::json!({"layer":{"id":layer},"blend_mode":"multiply"}),
        ),
    ] {
        let request = editor.request(strata_core::Source::Test);
        editor.execute(request, id, params, |_| Ok(())).unwrap();
        let scene = renderer
            .prepare_document(editor.document().unwrap(), true)
            .unwrap();
        renderer.commit_document(scene).unwrap();
        renderer
            .set_view(ViewTransform {
                zoom: 4.0,
                center_x: 8.0,
                center_y: 8.0,
            })
            .unwrap();
        renderer.render().unwrap();
        let actual = renderer.read_offscreen_rgba().unwrap();
        let PixelBuffer::Rgba16(expected) = editor.document().unwrap().export_pixels().unwrap()
        else {
            panic!()
        };
        for (screen_x, image_x) in [(16usize, 4usize), (48, 12)] {
            let offset = (8 * 16 + image_x) * 4;
            let observed = pixel(&actual, screen_x, 32);
            for channel in 0..3 {
                let target = ((u32::from(expected[offset + channel]) + 128) / 257) as i32;
                assert!(
                    (i32::from(observed[channel]) - target).abs() <= 2,
                    "{id}: channel {channel} {} vs {target}",
                    observed[channel]
                );
            }
            assert_eq!(observed[3], 255);
        }
    }
}

#[test]
fn exposure_mask_display_resources_release_the_document_gpu_budget() {
    use strata_pixels::memory::{MemoryLimits, MemoryPool};
    let Some(mut renderer) = renderer() else {
        return;
    };
    let pool = MemoryPool::new(MemoryLimits {
        cpu_bytes: 64 * 1024 * 1024,
        gpu_bytes: 64 * 1024 * 1024,
        disk_bytes: 64 * 1024 * 1024,
    });
    let mut editor = strata_core::Editor::with_memory_pool(pool.clone());
    for (id, params) in [
        (
            "file.new",
            serde_json::json!({"width":16,"height":16,"bit_depth":16,"background":[16384,16384,16384,65535]}),
        ),
        (
            "select.marquee",
            serde_json::json!({"x":2,"y":2,"width":8,"height":8}),
        ),
        (
            "layer.add_adjustment",
            serde_json::json!({"name":"exposure","type":"exposure","stops":0.5,"mask":"from_selection"}),
        ),
    ] {
        let request = editor.request(strata_core::Source::Test);
        editor.execute(request, id, params, |_| Ok(())).unwrap();
    }
    let scene = renderer
        .prepare_document(editor.document().unwrap(), true)
        .unwrap();
    renderer.commit_document(scene).unwrap();
    renderer.render().unwrap();
    assert!(pool.stats().gpu_bytes > 0);
    drop(renderer);
    assert_eq!(pool.stats().gpu_bytes, 0);
}

#[test]
fn group_fill_and_style_display_match_cpu_reference_at_sample_centers() {
    let Some(mut renderer) = renderer() else {
        return;
    };
    let mut editor = strata_core::Editor::new();
    for (id, params) in [
        (
            "file.new",
            serde_json::json!({"width":16,"height":16,"bit_depth":16}),
        ),
        (
            "layer.fill.gradient",
            serde_json::json!({"name":"fill","start":[10000,10000,10000,65535],"end":[40000,40000,40000,65535]}),
        ),
        (
            "select.marquee",
            serde_json::json!({"x":4,"y":4,"width":4,"height":4}),
        ),
        (
            "layer.mask.add",
            serde_json::json!({"layer":{"name":"fill"},"source":"from_selection"}),
        ),
        (
            "layer.group",
            serde_json::json!({"layers":[{"name":"fill"}],"name":"group"}),
        ),
        (
            "layer.set_styles",
            serde_json::json!({"layer":{"name":"group"},"styles":{"stroke":{"color":[65535,65535,65535,65535],"size":1},"drop_shadow":{"color":[20000,20000,20000,65535],"offset_x":4,"offset_y":0,"blur":0}}}),
        ),
    ] {
        editor
            .execute(
                editor.request(strata_core::Source::Test),
                id,
                params,
                |_| Ok(()),
            )
            .unwrap();
    }
    let scene = renderer
        .prepare_document(editor.document().unwrap(), true)
        .unwrap();
    renderer.commit_document(scene).unwrap();
    renderer
        .set_view(ViewTransform {
            zoom: 3.0,
            center_x: 8.0,
            center_y: 8.0,
        })
        .unwrap();
    renderer.render().unwrap();
    let actual = renderer.read_offscreen_rgba().unwrap();
    let PixelBuffer::Rgba16(expected) = editor.document().unwrap().export_pixels().unwrap() else {
        panic!()
    };
    for (x, y) in [(5usize, 5usize), (3, 5), (10, 5)] {
        let actual = pixel(&actual, x * 3 + 9, y * 3 + 9);
        let offset = (y * 16 + x) * 4;
        for channel in 0..3 {
            let target = ((u32::from(expected[offset + channel]) + 128) / 257) as i32;
            assert!(
                (i32::from(actual[channel]) - target).abs() <= 4,
                "({x},{y}) {channel}: {actual:?} target {target}"
            );
        }
    }
}

#[test]
fn tonal_adjustment_display_matches_cpu_and_parameter_updates_invalidate_tiles() {
    let Some(mut renderer) = renderer() else {
        return;
    };
    for (initial, changed) in [
        (
            serde_json::json!({"type":"brightness_contrast","brightness":10}),
            serde_json::json!({"type":"brightness_contrast","brightness":-10}),
        ),
        (
            serde_json::json!({"type":"levels","master":{"gamma":1.4}}),
            serde_json::json!({"type":"levels","master":{"gamma":0.7}}),
        ),
        (
            serde_json::json!({"type":"curves","master":[[0,0],[0.5,0.7],[1,1]]}),
            serde_json::json!({"type":"curves","master":[[0,0],[0.5,0.3],[1,1]]}),
        ),
    ] {
        let mut editor = strata_core::Editor::new();
        for (id, args) in [
            (
                "file.new",
                serde_json::json!({"width":16,"height":16,"bit_depth":16}),
            ),
            (
                "layer.fill.gradient",
                serde_json::json!({"name":"gradient","start":[8000,8000,8000,65535],"end":[50000,50000,50000,65535]}),
            ),
            (
                "select.marquee",
                serde_json::json!({"x":0,"y":0,"width":8,"height":16}),
            ),
            (
                "layer.add_tonal_adjustment",
                serde_json::json!({"name":"tone","adjustment":initial}),
            ),
        ] {
            editor
                .execute(editor.request(strata_core::Source::Test), id, args, |_| {
                    Ok(())
                })
                .unwrap();
        }
        for settings in [None, Some(changed)] {
            if let Some(settings) = settings {
                editor
                    .execute(
                        editor.request(strata_core::Source::Test),
                        "adjust.tonal.set",
                        serde_json::json!({"layer":{"name":"tone"},"adjustment":settings}),
                        |_| Ok(()),
                    )
                    .unwrap();
            }
            let scene = renderer
                .prepare_document(editor.document().unwrap(), false)
                .unwrap();
            renderer.commit_document(scene).unwrap();
            renderer
                .set_view(ViewTransform {
                    zoom: 3.0,
                    center_x: 8.0,
                    center_y: 8.0,
                })
                .unwrap();
            renderer.render().unwrap();
            let actual = renderer.read_offscreen_rgba().unwrap();
            let PixelBuffer::Rgba16(expected) = editor.document().unwrap().export_pixels().unwrap()
            else {
                panic!()
            };
            for (x, y) in [(2usize, 6usize), (6, 6), (10, 6), (14, 6)] {
                let shown = pixel(&actual, x * 3 + 9, y * 3 + 9);
                let offset = (y * 16 + x) * 4;
                for c in 0..3 {
                    let target = ((u32::from(expected[offset + c]) + 128) / 257) as i32;
                    assert!(
                        (i32::from(shown[c]) - target).abs() <= 4,
                        "{x},{y},{c}: {shown:?} expected {target}"
                    );
                }
            }
        }
    }
}

#[test]
fn point_color_and_white_balance_display_match_color_managed_cpu_reference() {
    let Some(mut renderer) = renderer() else {
        return;
    };
    for type_name in [
        "vibrance",
        "white_balance",
        "color_balance",
        "black_white",
        "photo_filter",
        "channel_mixer",
        "invert",
        "posterize",
        "threshold",
        "gradient_map",
        "selective_color",
        "desaturate",
    ] {
        let mut editor = strata_core::Editor::new();
        let settings = match type_name {
            "vibrance" => serde_json::json!({"type":type_name,"vibrance":40}),
            "white_balance" => serde_json::json!({"type":type_name,"temperature":30,"tint":10}),
            "color_balance" => serde_json::json!({"type":type_name,"midtones":[15,-10,5]}),
            _ => serde_json::json!({"type":type_name}),
        };
        for (id, args) in [
            (
                "file.new",
                serde_json::json!({"width":16,"height":16,"bit_depth":16,"working_space":"display_p3","background":[20000,35000,45000,65535]}),
            ),
            (
                "select.marquee",
                serde_json::json!({"x":0,"y":0,"width":8,"height":16}),
            ),
            (
                "layer.add_tonal_adjustment",
                serde_json::json!({"name":type_name,"adjustment":settings}),
            ),
        ] {
            editor
                .execute(editor.request(strata_core::Source::Test), id, args, |_| {
                    Ok(())
                })
                .unwrap();
        }
        let doc = editor.document().unwrap();
        let scene = renderer.prepare_document(doc, false).unwrap();
        renderer.commit_document(scene).unwrap();
        renderer
            .set_view(ViewTransform {
                zoom: 3.0,
                center_x: 8.0,
                center_y: 8.0,
            })
            .unwrap();
        renderer.render().unwrap();
        let actual = renderer.read_offscreen_rgba().unwrap();
        let expected = strata_color::to_display_p3(
            &doc.export_pixels().unwrap(),
            16,
            16,
            Some(doc.working_icc()),
        )
        .unwrap()
        .pixels;
        let PixelBuffer::Rgba16(expected) = expected else {
            panic!()
        };
        for (x, y) in [(2usize, 6usize), (6, 6), (10, 6), (14, 6)] {
            let shown = pixel(&actual, x * 3 + 9, y * 3 + 9);
            let offset = (y * 16 + x) * 4;
            for c in 0..3 {
                let target = ((u32::from(expected[offset + c]) + 128) / 257) as i32;
                assert!(
                    (i32::from(shown[c]) - target).abs() <= 4,
                    "{type_name} ({x},{y}) channel {c}: {shown:?} target {target}"
                );
            }
        }
    }
}

#[test]
fn spatial_shadows_highlights_display_matches_cpu_and_cross_tile_reference() {
    let Some(mut renderer) = renderer() else {
        return;
    };
    let mut editor = strata_core::Editor::new();
    for (id, args) in [
        (
            "file.new",
            serde_json::json!({"width":520,"height":32,"bit_depth":16,"working_space":"display_p3"}),
        ),
        (
            "layer.fill.gradient",
            serde_json::json!({"name":"ramp","start":[8000,8000,8000,65535],"end":[50000,50000,50000,65535]}),
        ),
        (
            "layer.add_tonal_adjustment",
            serde_json::json!({"name":"local","adjustment":{"type":"shadows_highlights","shadows":70,"highlights":30,"shadows_width":0.5,"highlights_width":0.5,"radius":18}}),
        ),
    ] {
        editor
            .execute(editor.request(strata_core::Source::Test), id, args, |_| {
                Ok(())
            })
            .unwrap();
    }
    let doc = editor.document().unwrap();
    let scene = renderer.prepare_document(doc, false).unwrap();
    renderer.commit_document(scene).unwrap();
    renderer
        .set_view(ViewTransform {
            zoom: 3.0,
            center_x: 260.0,
            center_y: 16.0,
        })
        .unwrap();
    renderer.render().unwrap();
    let actual = renderer.read_offscreen_rgba().unwrap();
    let expected = strata_color::to_display_p3(
        &doc.export_pixels().unwrap(),
        520,
        32,
        Some(doc.working_icc()),
    )
    .unwrap()
    .pixels;
    let PixelBuffer::Rgba16(expected) = expected else {
        panic!()
    };
    for x in [250usize, 255, 256, 261] {
        let y = 16;
        let screen_x = ((x as f32 - 260.0) * 3.0 + 32.0).round() as usize;
        let shown = pixel(&actual, screen_x, 32);
        let offset = (y * 520 + x) * 4;
        for c in 0..3 {
            let target = ((u32::from(expected[offset + c]) + 128) / 257) as i32;
            assert!(
                (i32::from(shown[c]) - target).abs() <= 4,
                "x={x} channel {c}: {shown:?} target {target}"
            );
        }
    }
}

#[test]
fn brush_eraser_and_mask_strokes_match_linear_cpu_composite_over_canvas() {
    let Some(mut renderer) = renderer() else {
        return;
    };
    for (command, target) in [
        ("paint.stroke", "pixels"),
        ("paint.erase", "pixels"),
        ("paint.stroke", "mask"),
        ("paint.erase", "mask"),
    ] {
        let mut editor = strata_core::Editor::new();
        editor.execute(editor.request(strata_core::Source::Test),"file.new",serde_json::json!({"width":16,"height":16,"bit_depth":16,"working_space":"display_p3","background":[20000,30000,40000,65535]}),|_|Ok(())).unwrap();
        if target == "mask" {
            editor.execute(editor.request(strata_core::Source::Test),"layer.mask.add",serde_json::json!({"layer":{"name":"Background"},"source":if command=="paint.erase"{"black"}else{"white"}}),|_|Ok(())).unwrap();
        }
        editor.execute(editor.request(strata_core::Source::Test),command,serde_json::json!({"layer":{"name":"Background"},"target":target,"points":[{"x":8.5,"y":8.5}],"size":6,"hardness":0.5,"opacity":0.7,"flow":0.4,"color":[1,0.1,0.2,1],"value":if command=="paint.erase"{1}else{0}}),|_|Ok(())).unwrap();
        let doc = editor.document().unwrap();
        let scene = renderer.prepare_document(doc, false).unwrap();
        renderer.commit_document(scene).unwrap();
        renderer
            .set_view(ViewTransform {
                zoom: 3.0,
                center_x: 8.0,
                center_y: 8.0,
            })
            .unwrap();
        renderer.render().unwrap();
        let actual = renderer.read_offscreen_rgba().unwrap();
        let reference = strata_core::composite::Compositor::new(1)
            .tile(doc, strata_pixels::TileCoord::new(0, 0).unwrap())
            .unwrap();
        for (x, y) in [(2usize, 6usize), (8, 8), (10, 8), (13, 8)] {
            let p = reference[y * 256 + x];
            let mut expected = [0u8; 4];
            expected[3] = 255;
            for c in 0..3 {
                let bg = f64::from(CANVAS_BG_SRGB[c]) / 255.0;
                let bg = if bg <= 0.04045 {
                    bg / 12.92
                } else {
                    ((bg + 0.055) / 1.055).powf(2.4)
                };
                let linear = f64::from(p[c]) * f64::from(p[3]) + bg * (1.0 - f64::from(p[3]));
                let encoded = if linear <= 0.0031308 {
                    linear * 12.92
                } else {
                    1.055 * linear.powf(1.0 / 2.4) - 0.055
                };
                expected[c] = (encoded.clamp(0.0, 1.0) * 255.0).round() as u8;
            }
            let shown = pixel(&actual, x * 3 + 9, y * 3 + 9);
            for c in 0..4 {
                assert!(
                    shown[c].abs_diff(expected[c]) <= 2,
                    "{command} {target} ({x},{y}) {shown:?} != {expected:?}"
                );
            }
        }
    }
}

#[test]
fn all_pixel_gradient_shapes_and_fill_update_display_tiles_against_cpu() {
    let Some(mut renderer) = renderer() else {
        return;
    };
    for shape in ["linear", "radial", "angle", "reflected", "diamond"] {
        let mut editor = strata_core::Editor::new();
        for (id, args) in [
            (
                "file.new",
                serde_json::json!({"width":16,"height":16,"bit_depth":16,"working_space":"display_p3","background":[12000,22000,32000,65535]}),
            ),
            (
                "paint.gradient",
                serde_json::json!({"layer":{"name":"Background"},"start":[8.5,8.5],"end":[14.5,8.5],"shape":shape,"opacity":0.8,"stops":[{"at":0,"color":[1,0.1,0.2,0]},{"at":1,"color":[0.2,0.6,1,1]}]}),
            ),
        ] {
            editor
                .execute(editor.request(strata_core::Source::Test), id, args, |_| {
                    Ok(())
                })
                .unwrap();
        }
        for pass in 0..2 {
            if pass == 1 {
                editor.execute(editor.request(strata_core::Source::Test),"paint.fill",serde_json::json!({"layer":{"name":"Background"},"color":[0.8,0.4,0.2,1],"opacity":0.3}),|_|Ok(())).unwrap();
            }
            let doc = editor.document().unwrap();
            let scene = renderer.prepare_document(doc, false).unwrap();
            renderer.commit_document(scene).unwrap();
            renderer
                .set_view(ViewTransform {
                    zoom: 3.0,
                    center_x: 8.0,
                    center_y: 8.0,
                })
                .unwrap();
            renderer.render().unwrap();
            let actual = renderer.read_offscreen_rgba().unwrap();
            let expected = strata_color::to_display_p3(
                &doc.export_pixels().unwrap(),
                16,
                16,
                Some(doc.working_icc()),
            )
            .unwrap()
            .pixels;
            let PixelBuffer::Rgba16(expected) = expected else {
                panic!()
            };
            for (x, y) in [(2usize, 6usize), (6, 6), (10, 10), (14, 12)] {
                let shown = pixel(&actual, x * 3 + 9, y * 3 + 9);
                let i = (y * 16 + x) * 4;
                for c in 0..3 {
                    let target = ((u32::from(expected[i + c]) + 128) / 257) as i32;
                    assert!(
                        (i32::from(shown[c]) - target).abs() <= 4,
                        "{shape} pass {pass} {x},{y},{c}: {shown:?} expected {target}"
                    );
                }
            }
        }
    }
}

#[test]
fn tiled_scene_preserves_source_and_has_no_mip_seam() {
    let Some(mut r) = renderer() else {
        return;
    };
    let icc = strata_color::display_p3_profile().unwrap().icc().unwrap();
    let source = PixelBuffer::Rgba16([32768, 32769, 50001, 65535].repeat(512 * 256));
    let doc = strata_core::DocumentCandidate::from_image(
        512,
        256,
        source.clone(),
        Some(icc),
        "test".into(),
    )
    .unwrap()
    .into_document();
    let prepared = r.prepare_document(&doc, true).unwrap();
    r.commit_document(prepared).unwrap();
    for zoom in [1.0, 0.5, 0.125] {
        r.set_view(ViewTransform {
            zoom,
            center_x: 256.0,
            center_y: 128.0,
        })
        .unwrap();
        r.render().unwrap();
        let frame = r.read_offscreen_rgba().unwrap();
        for x in 4..60 {
            close(pixel(&frame, x, 32), &[128, 128, 195, 255]);
        }
    }
    assert_eq!(doc.export_pixels().unwrap(), source);
}

#[test]
fn tiled_scene_negative_layers_and_foreign_candidate_are_safe() {
    let Some(mut r) = renderer() else {
        return;
    };
    let Some(mut other) = renderer() else {
        return;
    };
    let mut e = strata_core::Editor::new();
    for (command, args) in [
        (
            "file.new",
            serde_json::json!({"width":16,"height":16,"bit_depth":8}),
        ),
        (
            "layer.create",
            serde_json::json!({"name":"white","fill":[255,255,255,255]}),
        ),
        (
            "layer.set_position",
            serde_json::json!({"layer":{"name":"white"},"x":-8,"y":-8}),
        ),
    ] {
        let request = e.request(strata_core::Source::Test);
        e.execute(request, command, args, |_| Ok(())).unwrap();
    }
    let doc = e.document().unwrap();
    let scene = r.prepare_document(doc, true).unwrap();
    r.commit_document(scene).unwrap();
    r.set_view(ViewTransform {
        zoom: 2.0,
        center_x: 0.0,
        center_y: 0.0,
    })
    .unwrap();
    r.render().unwrap();
    let before = r.read_offscreen_rgba().unwrap();
    close(pixel(&before, 32, 32), &[255; 4]);
    let foreign = other.prepare_document(doc, true).unwrap();
    assert!(r.commit_document(foreign).is_err());
    r.render().unwrap();
    assert_eq!(r.read_offscreen_rgba().unwrap(), before);
}

#[test]
fn tiled_four_blends_match_analytic_reference_at_both_source_depths() {
    let Some(mut r) = renderer() else { return };
    let linear = |v: f64| {
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    let encode = |v: f64| {
        if v <= 0.0031308 {
            v * 12.92
        } else {
            1.055 * v.powf(1.0 / 2.4) - 0.055
        }
    };
    // Neutral samples avoid relying on production ICC transforms in the oracle.
    // Include partial alpha, layer opacity and both overlay branches.
    for depth in [8, 16] {
        let maximum: f64 = if depth == 8 { 255.0 } else { 65535.0 };
        for background in [0.25, 0.85] {
            for mode in ["normal", "multiply", "screen", "overlay"] {
                let b = (background * maximum).round() as u16;
                let s = (0.65 * maximum).round() as u16;
                let ab = (0.6 * maximum).round() as u16;
                let asrc = (0.8 * maximum).round() as u16;
                let mut editor = strata_core::Editor::new();
                for (id, params) in [
                    (
                        "file.new",
                        serde_json::json!({"width":16,"height":16,"bit_depth":depth}),
                    ),
                    (
                        "layer.create",
                        serde_json::json!({"name":"back","fill":[b,b,b,ab]}),
                    ),
                    (
                        "layer.create",
                        serde_json::json!({"name":"front","fill":[s,s,s,asrc]}),
                    ),
                    (
                        "layer.set_opacity",
                        serde_json::json!({"layer":{"name":"front"},"opacity":0.7}),
                    ),
                    (
                        "layer.set_blend_mode",
                        serde_json::json!({"layer":{"name":"front"},"blend_mode":mode}),
                    ),
                ] {
                    let request = editor.request(strata_core::Source::Test);
                    editor.execute(request, id, params, |_| Ok(())).unwrap();
                }
                let cb = linear(f64::from(b) / maximum);
                let cs = linear(f64::from(s) / maximum);
                let ba = f64::from(ab) / maximum;
                let sa = f64::from(asrc) / maximum * 0.7;
                let mix = match mode {
                    "normal" => cs,
                    "multiply" => cb * cs,
                    "screen" => 1.0 - (1.0 - cb) * (1.0 - cs),
                    _ if cb <= 0.5 => 2.0 * cb * cs,
                    _ => 1.0 - 2.0 * (1.0 - cb) * (1.0 - cs),
                };
                let premul = (1.0 - sa) * ba * cb + (1.0 - ba) * sa * cs + ba * sa * mix;
                let mut expected = [255u8; 4];
                for channel in 0..3 {
                    expected[channel] = (encode(
                        premul
                            + (1.0 - ba)
                                * (1.0 - sa)
                                * linear(f64::from(CANVAS_BG_SRGB[channel]) / 255.0),
                    ) * 255.0)
                        .round() as u8;
                }
                let scene = r
                    .prepare_document(editor.document().unwrap(), true)
                    .unwrap();
                r.commit_document(scene).unwrap();
                r.render().unwrap();
                close(pixel(&r.read_offscreen_rgba().unwrap(), 32, 32), &expected);
            }
        }
    }
}

fn put(r: &mut Renderer, pixels: PixelBuffer, w: u32, h: u32) {
    let prepared = r.prepare_image(w, h, &pixels).unwrap();
    r.commit_image(prepared).unwrap();
    r.set_view(ViewTransform {
        zoom: 16.0,
        center_x: w as f32 / 2.0,
        center_y: h as f32 / 2.0,
    })
    .unwrap();
}

fn pixel(bytes: &[u8], x: usize, y: usize) -> &[u8] {
    &bytes[(y * 64 + x) * 4..(y * 64 + x + 1) * 4]
}
fn close(actual: &[u8], expected: &[u8]) {
    assert_eq!(actual.len(), expected.len());
    for (a, b) in actual.iter().zip(expected) {
        assert!(a.abs_diff(*b) <= 2, "{actual:?} != {expected:?}");
    }
}

#[test]
fn renders_image_and_background() {
    let Some(mut r) = renderer() else {
        return;
    };
    put(
        &mut r,
        PixelBuffer::Rgba8([200, 100, 50, 255].repeat(4)),
        2,
        2,
    );
    assert!(!r.render().unwrap().skipped);
    let frame = r.read_offscreen_rgba().unwrap();
    close(pixel(&frame, 32, 32), &[200, 100, 50, 255]);
    close(pixel(&frame, 0, 0), &[20, 20, 21, 255]);
}

#[test]
fn transparent_image_shows_background() {
    let Some(mut r) = renderer() else {
        return;
    };
    put(&mut r, PixelBuffer::Rgba8([255, 0, 0, 0].repeat(4)), 2, 2);
    r.render().unwrap();
    close(
        pixel(&r.read_offscreen_rgba().unwrap(), 32, 32),
        &[20, 20, 21, 255],
    );
}

#[test]
fn changing_workspace_background_preserves_image_samples() {
    let Some(mut r) = renderer() else {
        return;
    };
    put(
        &mut r,
        PixelBuffer::Rgba8([200, 100, 50, 255].repeat(4)),
        2,
        2,
    );
    r.set_background([214, 214, 216]);
    r.render().unwrap();
    let frame = r.read_offscreen_rgba().unwrap();
    close(pixel(&frame, 0, 0), &[214, 214, 216, 255]);
    close(pixel(&frame, 32, 32), &[200, 100, 50, 255]);
    r.set_background(CANVAS_BG_SRGB);
    r.render().unwrap();
    let frame = r.read_offscreen_rgba().unwrap();
    close(pixel(&frame, 0, 0), &[20, 20, 21, 255]);
    close(pixel(&frame, 32, 32), &[200, 100, 50, 255]);
}

#[test]
fn empty_scene_and_zero_size_are_safe() {
    let Some(mut r) = renderer() else {
        return;
    };
    r.render().unwrap();
    close(
        pixel(&r.read_offscreen_rgba().unwrap(), 10, 10),
        &[CANVAS_BG_SRGB[0], CANVAS_BG_SRGB[1], CANVAS_BG_SRGB[2], 255],
    );
    r.resize(Viewport { width: 0, ..VP }).unwrap();
    assert!(r.render().unwrap().skipped);
    r.resize(VP).unwrap();
    assert!(!r.render().unwrap().skipped);
}

#[test]
fn failed_prepare_preserves_image_and_view() {
    let Some(mut r) = renderer() else {
        return;
    };
    put(
        &mut r,
        PixelBuffer::Rgba8([200, 100, 50, 255].repeat(4)),
        2,
        2,
    );
    let view = r.view();
    r.render().unwrap();
    let before = r.read_offscreen_rgba().unwrap();
    assert!(matches!(
        r.prepare_image(
            r.max_texture_dimension() + 1,
            1,
            &PixelBuffer::Rgba8(vec![])
        ),
        Err(RenderError::ImageTooLarge { .. })
    ));
    assert!(
        r.prepare_image(2, 2, &PixelBuffer::Rgba8(vec![0; 3]))
            .is_err()
    );
    assert_eq!(r.view(), view);
    r.render().unwrap();
    assert_eq!(r.read_offscreen_rgba().unwrap(), before);
}

#[test]
fn cancelling_scene_preparation_keeps_the_last_committed_frame() {
    let Some(mut renderer) = renderer() else {
        return;
    };
    put(
        &mut renderer,
        PixelBuffer::Rgba8([200, 100, 50, 255].repeat(4)),
        2,
        2,
    );
    renderer.render().unwrap();
    let before = renderer.read_offscreen_rgba().unwrap();
    let view = renderer.view();
    let mut editor = strata_core::Editor::new();
    editor.execute(editor.request(strata_core::Source::Test),"file.new",serde_json::json!({"width":512,"height":512,"bit_depth":16,"background":[30000,40000,50000,65535]}),|_|Ok(())).unwrap();
    let visits = std::cell::Cell::new(0);
    let result = renderer.prepare_document_cancellable(editor.document().unwrap(), true, &|| {
        visits.set(visits.get() + 1);
        visits.get() > 4
    });
    assert!(matches!(result, Err(RenderError::Cancelled)));
    assert!(visits.get() > 4);
    assert_eq!(renderer.view(), view);
    renderer.render().unwrap();
    assert_eq!(renderer.read_offscreen_rgba().unwrap(), before);
    let scene = renderer
        .prepare_document(editor.document().unwrap(), true)
        .unwrap();
    renderer.commit_document(scene).unwrap();
    renderer.render().unwrap();
    assert_ne!(renderer.read_offscreen_rgba().unwrap(), before);
}

#[test]
fn new_scale_rescales_view() {
    let Some(mut r) = renderer() else {
        return;
    };
    r.set_view(ViewTransform {
        zoom: 1.0,
        center_x: 5.0,
        center_y: 5.0,
    })
    .unwrap();
    r.resize(Viewport {
        width: 128,
        height: 128,
        scale: 2.0,
    })
    .unwrap();
    assert_eq!(
        r.view(),
        ViewTransform {
            zoom: 2.0,
            center_x: 5.0,
            center_y: 5.0
        }
    );
}

#[test]
fn rgba16_preview_does_not_mutate_source() {
    let Some(mut r) = renderer() else {
        return;
    };
    let source = PixelBuffer::Rgba16([32768, 32769, 50001, 65535].repeat(4));
    let before = source.clone();
    let prepared = r.prepare_image(2, 2, &source).unwrap();
    r.commit_image(prepared).unwrap();
    r.render().unwrap();
    assert_eq!(source, before);
    close(
        pixel(&r.read_offscreen_rgba().unwrap(), 32, 32),
        &[128, 128, 195, 255],
    );
}

#[test]
fn transparent_color_does_not_bleed_when_filtered() {
    let Some(mut r) = renderer() else {
        return;
    };
    put(
        &mut r,
        PixelBuffer::Rgba8(vec![255, 0, 0, 0, 255, 255, 255, 255]),
        2,
        1,
    );
    r.render().unwrap();
    let frame = r.read_offscreen_rgba().unwrap();
    let p = pixel(&frame, 32, 32);
    assert!(p[0].abs_diff(p[1]) <= 2, "red fringe: {p:?}");
    assert!(p[0] > 100 && p[0] < 240, "expected blended edge: {p:?}");
}

#[test]
fn prepared_image_cannot_cross_renderers() {
    let Some(a) = renderer() else {
        return;
    };
    let Some(mut b) = renderer() else {
        return;
    };
    let candidate = a
        .prepare_image(1, 1, &PixelBuffer::Rgba8(vec![255; 4]))
        .unwrap();
    assert!(b.commit_image(candidate).is_err());
}
