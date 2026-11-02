use super::*;
use image::{ImageBuffer, Rgba};
use std::{
    path::Path,
    sync::{Arc, Mutex, Weak},
};
use strata_pixels::PixelBuffer;

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
fn version_and_no_canvas_error() {
    let engine = StrataEngine::new(Arc::new(Recorder::default()));
    assert_eq!(engine.core_version(), "0.0.1");
    assert!(engine.document_info().is_none());
    assert!(matches!(engine.render_frame(), Err(StrataError::NoCanvas)));
    engine
        .resize_canvas(CanvasViewport {
            width: 0,
            height: 0,
            scale: 2.0,
        })
        .unwrap();
    engine.pan_by(10.0, 10.0).unwrap();
    engine.zoom_at(0.0, 0.0, 2.0).unwrap();
    engine.fit_to_screen().unwrap();
    engine.detach_canvas().unwrap();
    engine.detach_canvas().unwrap();
}

#[test]
fn open_notifies_once_and_preserves_source() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("red.png");
    photo(&path);
    let events = Arc::new(Recorder::default());
    let engine = StrataEngine::new(events.clone());
    let info = engine
        .open_image(path.to_string_lossy().into_owned())
        .unwrap();
    assert_eq!((info.width, info.height, info.bit_depth), (4, 3, 8));
    assert_eq!(info.source_profile, "sRGB (assumed)");
    assert_eq!(
        events.0.lock().unwrap().as_slice(),
        &[EngineEventDto::DocumentOpened {
            info: info.clone(),
            generation: 1
        }]
    );
    assert_eq!(engine.document_info(), Some(info));
    assert_eq!(
        engine
            .inner
            .lock()
            .unwrap()
            .engine
            .document()
            .unwrap()
            .pixels(),
        &PixelBuffer::Rgba8([255, 0, 0, 255].repeat(12))
    );
}

#[test]
fn failed_open_preserves_document_and_event_count() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("red.png");
    photo(&path);
    let events = Arc::new(Recorder::default());
    let engine = StrataEngine::new(events.clone());
    let original = engine
        .open_image(path.to_string_lossy().into_owned())
        .unwrap();
    let missing = dir.path().join("missing.png");
    assert!(matches!(
        engine.open_image(missing.to_string_lossy().into_owned()),
        Err(StrataError::NotFound { .. })
    ));
    let bad = dir.path().join("bad.jpg");
    std::fs::write(&bad, b"hello").unwrap();
    assert!(matches!(
        engine.open_image(bad.to_string_lossy().into_owned()),
        Err(StrataError::UnsupportedFormat { .. })
    ));
    assert_eq!(engine.document_info(), Some(original));
    assert_eq!(events.0.lock().unwrap().len(), 1);
}

#[test]
fn rgba16_source_and_icc_are_not_display_cache() {
    use image::ImageEncoder;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("p3.png");
    let source = vec![32768u16, 32769, 50001, 12345];
    let profile = strata_color::display_p3_profile().unwrap().icc().unwrap();
    let mut encoder = image::codecs::png::PngEncoder::new(std::fs::File::create(&path).unwrap());
    encoder.set_icc_profile(profile.clone()).unwrap();
    let bytes: Vec<u8> = source.iter().flat_map(|s| s.to_ne_bytes()).collect();
    encoder
        .write_image(&bytes, 1, 1, image::ExtendedColorType::Rgba16)
        .unwrap();
    let engine = StrataEngine::new(Arc::new(Recorder::default()));
    assert_eq!(
        engine
            .open_image(path.to_string_lossy().into_owned())
            .unwrap()
            .bit_depth,
        16
    );
    let inner = engine.inner.lock().unwrap();
    let doc = inner.engine.document().unwrap();
    assert_eq!(doc.pixels(), &PixelBuffer::Rgba16(source));
    assert_eq!(doc.icc_profile(), Some(profile.as_slice()));
}

#[test]
fn stale_open_cannot_commit() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("red.png");
    photo(&path);
    let events = Arc::new(Recorder::default());
    let engine = StrataEngine::new(events.clone());
    let old_generation = engine.begin_open().unwrap();
    let old_candidate = prepare_document(&path).unwrap();
    let latest = engine
        .open_image(path.to_string_lossy().into_owned())
        .unwrap();
    assert!(matches!(
        engine.commit_open(old_generation, old_candidate),
        Err(StrataError::Cancelled { .. })
    ));
    assert_eq!(engine.document_info(), Some(latest));
    assert_eq!(events.0.lock().unwrap().len(), 1);
}

#[derive(Default)]
struct ReentrantListener {
    engine: Mutex<Weak<StrataEngine>>,
    observed: Mutex<Option<ImageInfo>>,
}
impl EngineListener for ReentrantListener {
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
    let listener = Arc::new(ReentrantListener::default());
    let engine = StrataEngine::new(listener.clone());
    *listener.engine.lock().unwrap() = Arc::downgrade(&engine);
    let (tx, rx) = std::sync::mpsc::channel();
    let path = path.to_string_lossy().into_owned();
    let worker = std::thread::spawn(move || {
        tx.send(engine.open_image(path)).unwrap();
    });
    let result = rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .expect("callback deadlocked")
        .unwrap();
    worker.join().unwrap();
    assert_eq!(*listener.observed.lock().unwrap(), Some(result));
}

#[test]
fn renderer_prepare_failure_keeps_document_and_frame() {
    use strata_render::{RenderError, Renderer, Viewport};
    let renderer = match Renderer::new_offscreen(Viewport {
        width: 64,
        height: 64,
        scale: 1.0,
    }) {
        Ok(r) => r,
        Err(RenderError::NoAdapter) if std::env::var_os("STRATA_REQUIRE_GPU").is_none() => {
            eprintln!("SKIPPED GPU FFI test");
            return;
        }
        Err(e) => panic!("{e}"),
    };
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("red.png");
    photo(&path);
    let events = Arc::new(Recorder::default());
    let engine = StrataEngine::new(events.clone());
    let max = renderer.max_texture_dimension();
    engine.inner.lock().unwrap().renderer = Some(renderer);
    let original = engine
        .open_image(path.to_string_lossy().into_owned())
        .unwrap();
    engine.render_frame().unwrap();
    let before = engine
        .inner
        .lock()
        .unwrap()
        .renderer
        .as_ref()
        .unwrap()
        .read_offscreen_rgba()
        .unwrap();
    let too_large = dir.path().join("wide.png");
    ImageBuffer::from_pixel(max + 1, 1, Rgba([1u8, 2, 3, 255]))
        .save(&too_large)
        .unwrap();
    assert!(matches!(
        engine.open_image(too_large.to_string_lossy().into_owned()),
        Err(StrataError::Render { .. })
    ));
    engine.render_frame().unwrap();
    assert_eq!(
        engine
            .inner
            .lock()
            .unwrap()
            .renderer
            .as_ref()
            .unwrap()
            .read_offscreen_rgba()
            .unwrap(),
        before
    );
    assert_eq!(engine.document_info(), Some(original));
    assert_eq!(events.0.lock().unwrap().len(), 1);
}
