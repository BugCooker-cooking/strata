use serde_json::{Value, json};
use strata_core::{ClipboardImage, Document, Editor, Source};
use strata_pixels::{PixelBuffer, TileCoord, memory::MemoryPool};
fn run(e: &mut Editor, id: &str, p: Value) -> Value {
    e.execute(e.request(Source::User), id, p, |_| Ok(()))
        .unwrap()
}
fn fixture(depth: u8) -> Editor {
    let mut e = Editor::new();
    run(
        &mut e,
        "file.new",
        json!({"width":16,"height":8,"bit_depth":depth,"working_space":"srgb"}),
    );
    let values = (0..128)
        .flat_map(|i| [32768, 32769, 50001, if i % 2 == 0 { 32768 } else { 0 }])
        .collect::<Vec<_>>();
    let pixels = if depth == 16 {
        PixelBuffer::Rgba16(values)
    } else {
        PixelBuffer::Rgba8(values.into_iter().map(|v| (v / 257) as u8).collect())
    };
    let image = ClipboardImage::new(
        pixels,
        16,
        8,
        0,
        0,
        e.document().unwrap().working_icc().to_vec(),
        &MemoryPool::global(),
    )
    .unwrap();
    let token = e.grant_clipboard_image(image, Source::User).unwrap();
    run(&mut e, "edit.paste_in_place", json!({"resource":token}));
    e
}
#[test]
fn assignment_keeps_every_sample_mask_generated_color_and_original_profile_exact() {
    for depth in [8, 16] {
        let mut e = fixture(depth);
        let layer = e.snapshot().active_layer.unwrap();
        run(
            &mut e,
            "layer.mask.add",
            json!({"layer":{"id":layer},"source":"white"}),
        );
        let max = if depth == 16 { 65535 } else { 255 };
        run(
            &mut e,
            "layer.fill.solid",
            json!({"name":"generated","color":[max,0,max,max]}),
        );
        run(
            &mut e,
            "select.marquee",
            json!({"x":1,"y":1,"width":7,"height":5}),
        );
        let before = e.document().unwrap().to_archive().unwrap();
        let history = e.snapshot().history.len();
        let version = e.document().unwrap().layers()[1]
            .pixels()
            .tile_version(TileCoord::new(0, 0).unwrap());
        let result = run(&mut e, "color.assign", json!({"space":"display_p3"}));
        assert_eq!(result["working_space"], "display_p3");
        let after = e.document().unwrap().to_archive().unwrap();
        assert!(after.tiles == before.tiles);
        assert_eq!(after.metadata["layers"], before.metadata["layers"]);
        assert_eq!(after.metadata["source_icc"], before.metadata["source_icc"]);
        assert_ne!(
            after.metadata["working_icc"],
            before.metadata["working_icc"]
        );
        assert_eq!(
            e.document().unwrap().layers()[1]
                .pixels()
                .tile_version(TileCoord::new(0, 0).unwrap()),
            version
        );
        assert_eq!(e.snapshot().history.len(), history + 1);
        let reopened = Document::from_archive(after.clone()).unwrap();
        assert!(reopened.to_archive().unwrap() == after);
        run(&mut e, "edit.undo", json!({}));
        assert!(e.document().unwrap().to_archive().unwrap() == before);
    }
}
#[test]
fn assignment_changes_interpretation_while_conversion_preserves_physical_color() {
    let base = fixture(16);
    let mut assigned = base.clone();
    let mut converted = base.clone();
    let coordinate = TileCoord::new(0, 0).unwrap();
    let original = strata_core::composite::Compositor::new(1)
        .tile(base.document().unwrap(), coordinate)
        .unwrap()[0];
    let before = base.document().unwrap().to_archive().unwrap();
    run(&mut assigned, "color.assign", json!({"space":"display_p3"}));
    run(
        &mut converted,
        "color.convert",
        json!({"space":"display_p3"}),
    );
    let a = strata_core::composite::Compositor::new(1)
        .tile(assigned.document().unwrap(), coordinate)
        .unwrap()[0];
    let c = strata_core::composite::Compositor::new(1)
        .tile(converted.document().unwrap(), coordinate)
        .unwrap()[0];
    assert!((0..3).any(|i| (a[i] - original[i]).abs() > 0.001));
    assert!((0..3).all(|i| (c[i] - original[i]).abs() < 0.0001));
    assert!(assigned.document().unwrap().to_archive().unwrap().tiles == before.tiles);
    assert!(converted.document().unwrap().to_archive().unwrap().tiles != before.tiles);
    let source = base.document().unwrap().layers()[1]
        .pixels()
        .tile(coordinate)
        .unwrap()
        .unwrap();
    let result = converted.document().unwrap().layers()[1]
        .pixels()
        .tile(coordinate)
        .unwrap()
        .unwrap();
    let (PixelBuffer::Rgba16(source), PixelBuffer::Rgba16(result)) = (&*source, &*result) else {
        panic!()
    };
    assert!(
        source
            .chunks_exact(4)
            .zip(result.chunks_exact(4))
            .all(|(a, b)| a[3] == b[3])
    );
}
#[test]
fn invalid_profiles_and_locks_are_atomic_but_pixel_locks_allow_assignment() {
    let mut e = fixture(16);
    let layer = e.snapshot().active_layer;
    run(
        &mut e,
        "layer.set_locks",
        json!({"layer":{"id":layer},"locks":{"pixels":true,"transparency":true,"position":true}}),
    );
    let original = e.document().unwrap().to_archive().unwrap();
    let revision = e.revision();
    assert!(
        e.execute(
            e.request(Source::User),
            "color.convert",
            json!({"space":"display_p3"}),
            |_| Ok(())
        )
        .is_err()
    );
    assert_eq!(e.revision(), revision);
    run(&mut e, "color.assign", json!({"space":"display_p3"}));
    assert!(e.document().unwrap().to_archive().unwrap().tiles == original.tiles);
    for params in [
        json!({"space":"cmyk"}),
        json!({"space":"srgb","unexpected":true}),
    ] {
        let revision = e.revision();
        assert!(
            e.execute(e.request(Source::User), "color.assign", params, |_| Ok(()))
                .is_err()
        );
        assert_eq!(e.revision(), revision);
    }
    run(
        &mut e,
        "layer.set_locks",
        json!({"layer":{"id":layer},"locks":{"all":true}}),
    );
    let revision = e.revision();
    assert!(
        e.execute(
            e.request(Source::User),
            "color.assign",
            json!({"space":"srgb"}),
            |_| Ok(())
        )
        .is_err()
    );
    assert_eq!(e.revision(), revision);
}
#[test]
fn conversion_checks_independent_cancellation_before_publishing_any_layer() {
    use strata_pixels::memory::MemoryLimits;
    let pool = MemoryPool::new(MemoryLimits {
        cpu_bytes: 128 * 1024 * 1024,
        gpu_bytes: 0,
        disk_bytes: 128 * 1024 * 1024,
    });
    let mut e = Editor::with_memory_pool(pool.clone());
    run(
        &mut e,
        "file.new",
        json!({"width":2048,"height":2048,"bit_depth":16,"working_space":"srgb","background":[20000,30000,40000,32768]}),
    );
    let state = e.document().unwrap().state_id();
    let icc = e.document().unwrap().working_icc().to_vec();
    let request = e.request(Source::User);
    let source = e.clone();
    let worker = std::thread::spawn(move || {
        source.prepare(request, "color.convert", json!({"space":"display_p3"}))
    });
    let start = std::time::Instant::now();
    while pool.stats().transient_bytes < 2 * 1024 * 1024
        && start.elapsed() < std::time::Duration::from_secs(2)
    {
        std::thread::yield_now();
    }
    e.cancel_pending_work();
    assert!(matches!(
        worker.join().unwrap(),
        Err(strata_core::commands::CommandError::Expired)
    ));
    assert_eq!(e.document().unwrap().state_id(), state);
    assert_eq!(e.document().unwrap().working_icc(), icc);
    assert_eq!(e.snapshot().history.len(), 0);
}
