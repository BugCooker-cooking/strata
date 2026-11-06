use super::*;
use image::{ImageBuffer, ImageEncoder, Rgba};
use std::sync::Weak;
use strata_pixels::{PixelBuffer, PixelValue, TileCoord};

#[derive(Default)]
struct Recorder(Mutex<Vec<EngineEventDto>>);
impl EngineListener for Recorder {
    fn on_event(&self, event: EngineEventDto) {
        self.0.lock().unwrap().push(event);
    }
}
fn photo(path: &Path) {
    ImageBuffer::from_pixel(4, 3, Rgba([255u8, 0, 0, 255]))
        .save(path)
        .unwrap();
}
#[test]
fn version_no_canvas_and_view_commands() {
    let e = StrataEngine::new(Arc::new(Recorder::default()));
    assert_eq!(e.core_version(), "0.0.1");
    assert!(e.document_info().is_none());
    assert!(matches!(e.render_frame(), Err(StrataError::NoCanvas)));
    e.resize_canvas(CanvasViewport {
        width: 0,
        height: 0,
        scale: 2.0,
    })
    .unwrap();
    e.pan_by(10.0, 10.0).unwrap();
    e.zoom_at(0.0, 0.0, 2.0).unwrap();
    e.fit_to_screen().unwrap();
    e.detach_canvas().unwrap();
    e.detach_canvas().unwrap();
}

#[test]
fn actual_size_is_absolute_preserves_center_and_never_edits_document() {
    let e = StrataEngine::new(Arc::new(Recorder::default()));
    e.dispatch(
        "file.new".into(),
        json!({"width":16,"height":16,"bit_depth":16}).to_string(),
    )
    .unwrap();
    let truth = e.snapshot_json().unwrap();
    let mut renderer = match strata_render::Renderer::new_offscreen(strata_render::Viewport {
        width: 64,
        height: 64,
        scale: 2.0,
    }) {
        Ok(r) => r,
        Err(strata_render::RenderError::NoAdapter)
            if std::env::var_os("STRATA_REQUIRE_GPU").is_none() =>
        {
            return;
        }
        Err(e) => panic!("GPU: {e}"),
    };
    renderer
        .set_view(strata_render::ViewTransform {
            zoom: 0.25,
            center_x: -12.5,
            center_y: 31.25,
        })
        .unwrap();
    e.inner.lock().unwrap().renderer = Some(renderer);
    for zoom in [0.25, 3.0] {
        let mut inner = e.inner.lock().unwrap();
        let mut view = inner.renderer.as_ref().unwrap().view();
        view.zoom = zoom;
        inner.renderer.as_mut().unwrap().set_view(view).unwrap();
        drop(inner);
        e.dispatch("view.actual_size".into(), "{}".into()).unwrap();
        let view = e.inner.lock().unwrap().renderer.as_ref().unwrap().view();
        assert_eq!(view.zoom, 1.0);
        assert_eq!((view.center_x, view.center_y), (-12.5, 31.25));
        assert_eq!(e.snapshot_json().unwrap(), truth);
    }
}
#[test]
fn palette_state_keeps_crop_and_filter_preview_ownership_without_document_edits() {
    let e = StrataEngine::new(Arc::new(Recorder::default()));
    e.dispatch(
        "file.new".into(),
        json!({"width":16,"height":16,"bit_depth":16,"background":[12001,30003,50001,32769]})
            .to_string(),
    )
    .unwrap();
    let state = e.lock().unwrap().editor.snapshot();
    let before = e
        .lock()
        .unwrap()
        .editor
        .document()
        .unwrap()
        .to_archive()
        .unwrap();
    let crop = e
        .begin_crop(state.document_id.unwrap().to_string(), state.revision)
        .unwrap();
    e.dispatch(
        "paint.colors.set".into(),
        json!({"foreground":[0.2,0.3,0.4,1.]}).to_string(),
    )
    .unwrap();
    assert_eq!(e.lock().unwrap().editor.revision(), state.revision);
    e.end_crop(
        crop,
        json!({"rect":{"x":2,"y":3,"width":8,"height":6},"pixels":"preserve"}).to_string(),
        false,
    )
    .unwrap();
    e.dispatch("edit.undo".into(), "{}".into()).unwrap();
    assert_eq!(
        e.lock()
            .unwrap()
            .editor
            .document()
            .unwrap()
            .to_archive()
            .unwrap(),
        before
    );
    let state = e.lock().unwrap().editor.snapshot();
    let sequence = e.issue_tonal_preview().unwrap();
    e.begin_filter_preview(
        state.active_layer.unwrap().to_string(),
        "filter.add_noise".into(),
        state.revision,
        sequence,
    )
    .unwrap();
    let params = json!({"amount":0.1,"seed":42}).to_string();
    e.preview_filter(params.clone(), sequence).unwrap();
    e.dispatch("paint.colors.swap".into(), "{}".into()).unwrap();
    assert_eq!(
        e.tonal_preview_sequence
            .load(std::sync::atomic::Ordering::Acquire),
        sequence
    );
    e.end_filter_preview_checked(params, false, sequence)
        .unwrap();
    assert_eq!(
        e.lock()
            .unwrap()
            .editor
            .snapshot()
            .history
            .last()
            .unwrap()
            .command,
        "filter.add_noise"
    );
}

#[test]
fn color_panel_description_is_event_free_and_formatted_write_has_only_one_palette_event() {
    let recorder = Arc::new(Recorder::default());
    let e = StrataEngine::new(recorder.clone());
    let initial = e.snapshot_json().unwrap();
    let events = recorder.0.lock().unwrap().len();
    let d: Value = serde_json::from_str(
        &e.dispatch("paint.colors.describe".into(), "{}".into())
            .unwrap(),
    )
    .unwrap();
    assert_eq!(d["foreground"]["hex"], "#000000FF");
    assert_eq!(e.snapshot_json().unwrap(), initial);
    assert_eq!(recorder.0.lock().unwrap().len(), events);
    e.dispatch("paint.colors.set_components".into(),json!({"target":"foreground","color":{"model":"hsb","hue":120,"saturation":100,"brightness":100}}).to_string()).unwrap();
    assert_eq!(recorder.0.lock().unwrap().len(), events + 1);
    let state = e.snapshot_json().unwrap();
    e.dispatch("paint.colors.describe".into(), "{}".into())
        .unwrap();
    assert_eq!(e.snapshot_json().unwrap(), state);
    assert_eq!(recorder.0.lock().unwrap().len(), events + 1);
}

#[test]
fn color_panel_updates_leave_live_metal_preview_untouched_and_drive_the_same_explicit_fill() {
    let renderer = match Renderer::new_offscreen(Viewport {
        width: 64,
        height: 64,
        scale: 1.,
    }) {
        Ok(value) => value,
        Err(strata_render::RenderError::NoAdapter)
            if std::env::var_os("STRATA_REQUIRE_GPU").is_none() =>
        {
            return;
        }
        Err(error) => panic!("required GPU: {error}"),
    };
    let e = StrataEngine::new(Arc::new(Recorder::default()));
    e.lock().unwrap().renderer = Some(renderer);
    e.dispatch(
        "file.new".into(),
        json!({"width":16,"height":16,"bit_depth":16,"background":[10001,30003,50001,65535]})
            .to_string(),
    )
    .unwrap();
    {
        let mut i = e.lock().unwrap();
        i.renderer
            .as_mut()
            .unwrap()
            .set_view(strata_render::ViewTransform {
                zoom: 3.,
                center_x: 8.,
                center_y: 8.,
            })
            .unwrap();
    }
    let state = e.lock().unwrap().editor.snapshot();
    let sequence = e.issue_tonal_preview().unwrap();
    e.begin_filter_preview(
        state.active_layer.unwrap().to_string(),
        "filter.add_noise".into(),
        state.revision,
        sequence,
    )
    .unwrap();
    let params = json!({"amount":0.1,"seed":42}).to_string();
    e.preview_filter(params.clone(), sequence).unwrap();
    let before = {
        let mut i = e.lock().unwrap();
        let r = i.renderer.as_mut().unwrap();
        r.render().unwrap();
        r.read_offscreen_rgba().unwrap()
    };
    let result: Value = serde_json::from_str(
        &e.dispatch(
            "paint.colors.set_components".into(),
            json!({"target":"foreground","color":{"model":"hex","value":"#33669980"}}).to_string(),
        )
        .unwrap(),
    )
    .unwrap();
    let formats: Value = serde_json::from_str(
        &e.dispatch("paint.colors.describe".into(), "{}".into())
            .unwrap(),
    )
    .unwrap();
    assert_eq!(formats["foreground"]["rgba"], result["foreground"]);
    {
        let mut i = e.lock().unwrap();
        assert!(i.editor.snapshot().history.is_empty());
        assert_eq!(i.editor.revision(), state.revision);
        let r = i.renderer.as_mut().unwrap();
        r.render().unwrap();
        assert_eq!(r.read_offscreen_rgba().unwrap(), before);
    }
    e.end_filter_preview_checked(params, true, sequence)
        .unwrap();
    let original = e
        .lock()
        .unwrap()
        .editor
        .document()
        .unwrap()
        .to_archive()
        .unwrap();
    e.dispatch(
        "paint.fill".into(),
        json!({"layer":{"name":"Background"},"color":formats["foreground"]["rgba"]}).to_string(),
    )
    .unwrap();
    {
        let mut i = e.lock().unwrap();
        let reference = strata_core::composite::Compositor::new(1)
            .tile(i.editor.document().unwrap(), TileCoord::new(0, 0).unwrap())
            .unwrap()[8 * 256 + 8];
        let r = i.renderer.as_mut().unwrap();
        r.render().unwrap();
        let shown = r.read_offscreen_rgba().unwrap();
        for c in 0..3 {
            let v = f64::from(reference[c]);
            let v = if v <= 0.0031308 {
                12.92 * v
            } else {
                1.055 * v.powf(1. / 2.4) - 0.055
            };
            assert!(
                shown[(33 * 64 + 33) * 4 + c].abs_diff((v.clamp(0., 1.) * 255.).round() as u8) <= 3
            );
        }
    }
    e.dispatch("edit.undo".into(), "{}".into()).unwrap();
    assert_eq!(
        e.lock()
            .unwrap()
            .editor
            .document()
            .unwrap()
            .to_archive()
            .unwrap(),
        original
    );
    let colors: Value =
        serde_json::from_str(&e.dispatch("paint.colors.get".into(), "{}".into()).unwrap()).unwrap();
    assert_eq!(colors["foreground"], formats["foreground"]["rgba"]);
}
#[test]
fn registered_open_preserves_source_and_reports_once() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("red.png");
    photo(&path);
    let events = Arc::new(Recorder::default());
    let e = StrataEngine::new(events.clone());
    let info = e.open_image(path.display().to_string()).unwrap();
    assert_eq!((info.width, info.height, info.bit_depth), (4, 3, 8));
    assert_eq!(
        events
            .0
            .lock()
            .unwrap()
            .iter()
            .filter(|v| matches!(v, EngineEventDto::DocumentOpened { .. }))
            .count(),
        1
    );
    assert_eq!(
        e.inner.lock().unwrap().editor.document().unwrap().layers()[0]
            .pixels()
            .pixel(0, 0)
            .unwrap(),
        PixelValue::Rgba8([255, 0, 0, 255])
    );
}
#[test]
fn failed_open_keeps_document_and_event_count() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("red.png");
    photo(&path);
    let events = Arc::new(Recorder::default());
    let e = StrataEngine::new(events.clone());
    let original = e.open_image(path.display().to_string()).unwrap();
    let count = events.0.lock().unwrap().len();
    assert!(matches!(
        e.open_image(dir.path().join("missing.png").display().to_string()),
        Err(StrataError::NotFound { .. })
    ));
    let bad = dir.path().join("bad.jpg");
    std::fs::write(&bad, b"hello").unwrap();
    assert!(matches!(
        e.open_image(bad.display().to_string()),
        Err(StrataError::UnsupportedFormat { .. })
    ));
    assert_eq!(e.document_info(), Some(original));
    assert_eq!(events.0.lock().unwrap().len(), count);
}
#[test]
fn rgba16_source_and_icc_do_not_become_display_cache() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("p3.png");
    let source = vec![32768u16, 32769, 50001, 0];
    let profile = strata_color::display_p3_profile().unwrap().icc().unwrap();
    let mut encoder = image::codecs::png::PngEncoder::new(std::fs::File::create(&path).unwrap());
    encoder.set_icc_profile(profile.clone()).unwrap();
    let bytes: Vec<_> = source.iter().flat_map(|s| s.to_ne_bytes()).collect();
    encoder
        .write_image(&bytes, 1, 1, image::ExtendedColorType::Rgba16)
        .unwrap();
    let e = StrataEngine::new(Arc::new(Recorder::default()));
    assert_eq!(
        e.open_image(path.display().to_string()).unwrap().bit_depth,
        16
    );
    let inner = e.inner.lock().unwrap();
    let doc = inner.editor.document().unwrap();
    assert_eq!(doc.export_pixels().unwrap(), PixelBuffer::Rgba16(source));
    assert_eq!(doc.source_icc(), Some(profile.as_slice()));
}
#[derive(Default)]
struct Reentrant {
    engine: Mutex<Weak<StrataEngine>>,
    observed: Mutex<Option<ImageInfo>>,
}
impl EngineListener for Reentrant {
    fn on_event(&self, _: EngineEventDto) {
        let engine = self.engine.lock().unwrap().upgrade().unwrap();
        *self.observed.lock().unwrap() = engine.document_info();
    }
}
#[test]
fn listener_can_reenter_state_lock() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("red.png");
    photo(&path);
    let listener = Arc::new(Reentrant::default());
    let e = StrataEngine::new(listener.clone());
    *listener.engine.lock().unwrap() = Arc::downgrade(&e);
    let (tx, rx) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        tx.send(e.open_image(path.display().to_string())).unwrap();
    });
    let result = rx
        .recv_timeout(std::time::Duration::from_secs(10))
        .unwrap()
        .unwrap();
    worker.join().unwrap();
    assert_eq!(*listener.observed.lock().unwrap(), Some(result));
}
#[test]
fn ffi_commands_and_headless_engine_have_identical_pixels() {
    let e = StrataEngine::new(Arc::new(Recorder::default()));
    let mut headless = Editor::new();
    for (command, args) in [
        ("file.new", json!({"width":8,"height":8,"bit_depth":16})),
        (
            "layer.create",
            json!({"name":"color","fill":[32768,32769,50001,65535]}),
        ),
        (
            "layer.set_opacity",
            json!({"layer":{"name":"color"},"opacity":0.5}),
        ),
        ("edit.undo", json!({})),
    ] {
        e.dispatch(command.into(), args.to_string()).unwrap();
        let request = headless.request(Source::User);
        headless
            .execute(request, command, args, |_| Ok(()))
            .unwrap();
    }
    assert_eq!(
        e.inner
            .lock()
            .unwrap()
            .editor
            .document()
            .unwrap()
            .export_pixels()
            .unwrap(),
        headless.document().unwrap().export_pixels().unwrap()
    );
}

#[test]
fn stale_native_candidate_keeps_committed_document_and_scene() {
    let renderer = match Renderer::new_offscreen(Viewport {
        width: 32,
        height: 32,
        scale: 1.0,
    }) {
        Ok(r) => r,
        Err(strata_render::RenderError::NoAdapter)
            if std::env::var_os("STRATA_REQUIRE_GPU").is_none() =>
        {
            return;
        }
        Err(e) => panic!("GPU required: {e}"),
    };
    let e = StrataEngine::new(Arc::new(Recorder::default()));
    e.inner.lock().unwrap().renderer = Some(renderer);
    e.dispatch(
        "file.new".into(),
        json!({"width":4,"height":4,"bit_depth":8}).to_string(),
    )
    .unwrap();
    let stale = {
        let inner = e.inner.lock().unwrap();
        inner
            .editor
            .prepare(
                inner.editor.request(Source::User),
                "layer.create",
                json!({"name":"stale","fill":[255,0,0,255]}),
            )
            .unwrap()
    };
    e.dispatch(
        "layer.create".into(),
        json!({"name":"newer","fill":[0,255,0,255]}).to_string(),
    )
    .unwrap();
    let mut inner = e.inner.lock().unwrap();
    inner.renderer.as_mut().unwrap().render().unwrap();
    let before = inner
        .renderer
        .as_ref()
        .unwrap()
        .read_offscreen_rgba()
        .unwrap();
    let state = inner.editor.document().unwrap().state_id();
    assert!(apply(&mut inner, stale, "layer.create", &json!({})).is_err());
    assert_eq!(inner.editor.document().unwrap().state_id(), state);
    inner.renderer.as_mut().unwrap().render().unwrap();
    assert_eq!(
        inner
            .renderer
            .as_ref()
            .unwrap()
            .read_offscreen_rgba()
            .unwrap(),
        before
    );
}
