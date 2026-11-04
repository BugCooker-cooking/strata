use strata_pixels::{
    PixelBuffer, PixelFormat, PixelValue, TileCoord, TiledPixels,
    memory::{MemoryKind, MemoryLimits, MemoryPool},
};
const TILE: u64 = 256 * 256 * 8;
fn pool(cpu: u64, disk: u64) -> MemoryPool {
    MemoryPool::new(MemoryLimits {
        cpu_bytes: cpu,
        gpu_bytes: TILE,
        disk_bytes: disk,
    })
}

#[test]
fn shared_global_budget_spills_multiple_documents_and_pins_readers_without_losing_history() {
    let pool = pool(TILE * 3, TILE * 20);
    let mut a = TiledPixels::with_pool(PixelFormat::Rgba16, 20, pool.clone());
    let mut b = TiledPixels::with_pool(PixelFormat::Rgba16, 20, pool.clone());
    let original = PixelValue::Rgba16([32768, 32769, 50001, 0]);
    a.set_pixel(0, 0, original).unwrap();
    let historical = a.clone();
    let pin = a.tile(TileCoord::new(0, 0).unwrap()).unwrap().unwrap();
    for i in 0..10 {
        b.set_pixel(i * 256, 0, PixelValue::Rgba16([i as u16 + 1, 2, 3, 65535]))
            .unwrap();
    }
    assert!(pool.stats().disk_bytes > 0);
    assert!(pool.stats().peak_cpu_bytes <= TILE * 3);
    assert_eq!(
        pin.as_ref(),
        historical
            .tile(TileCoord::new(0, 0).unwrap())
            .unwrap()
            .unwrap()
            .as_ref()
    );
    a.set_pixel(0, 0, PixelValue::Rgba16([7; 4])).unwrap();
    drop(pin);
    pool.trim().unwrap();
    assert_eq!(pool.stats().source_bytes, 0);
    assert_eq!(historical.pixel(0, 0).unwrap(), original);
    assert_eq!(a.pixel(0, 0).unwrap(), PixelValue::Rgba16([7; 4]));
    for i in 0..10 {
        assert_eq!(
            b.pixel(i * 256, 0).unwrap(),
            PixelValue::Rgba16([i as u16 + 1, 2, 3, 65535])
        );
    }
    assert!(pool.stats().reloads > 0);
    drop((a, b, historical));
    assert_eq!(pool.stats().cpu_bytes, 0);
    assert_eq!(pool.stats().disk_bytes, 0);
}
#[test]
fn disk_full_and_pinned_budget_failure_leave_existing_samples_unchanged() {
    let pool = pool(TILE, 0);
    let mut pixels = TiledPixels::with_pool(PixelFormat::Rgba16, 2, pool.clone());
    pixels
        .set_pixel(0, 0, PixelValue::Rgba16([1, 2, 3, 4]))
        .unwrap();
    let before = pixels.tile_version(TileCoord::new(0, 0).unwrap());
    assert!(
        pixels
            .set_pixel(256, 0, PixelValue::Rgba16([5; 4]))
            .is_err()
    );
    assert_eq!(pixels.tile_count(), 1);
    assert_eq!(pixels.tile_version(TileCoord::new(0, 0).unwrap()), before);
    assert_eq!(
        pixels.pixel(0, 0).unwrap(),
        PixelValue::Rgba16([1, 2, 3, 4])
    );
    let pin = pixels.tile(TileCoord::new(0, 0).unwrap()).unwrap().unwrap();
    assert!(pool.reserve(MemoryKind::Transient, 1).is_err());
    assert_eq!(pin.format(), PixelFormat::Rgba16);
    assert_eq!(pool.stats().disk_bytes, 0);
}
#[test]
fn backing_extents_are_reused_and_gpu_budget_is_independent() {
    let pool = pool(TILE, TILE * 2);
    for _ in 0..5 {
        let mut pixels = TiledPixels::with_pool(PixelFormat::Rgba16, 2, pool.clone());
        pixels
            .insert_tile(
                TileCoord::new(0, 0).unwrap(),
                PixelBuffer::Rgba16(vec![3; 256 * 256 * 4]),
            )
            .unwrap();
        pool.trim().unwrap();
        assert_eq!(pool.stats().disk_bytes, TILE);
        let gpu = pool.reserve(MemoryKind::Gpu, TILE).unwrap();
        assert!(pool.reserve(MemoryKind::Gpu, 1).is_err());
        assert_eq!(pixels.pixel(0, 0).unwrap(), PixelValue::Rgba16([3; 4]));
        drop((pixels, gpu));
        assert_eq!(pool.stats().disk_bytes, 0);
    }
}

#[test]
fn concurrent_documents_reload_and_evict_without_exceeding_the_shared_pool() {
    let pool = pool(TILE * 8, TILE * 256);
    let workers: Vec<_> = (0..4)
        .map(|worker| {
            let pool = pool.clone();
            std::thread::spawn(move || {
                let mut pixels = TiledPixels::with_pool(PixelFormat::Rgba16, 32, pool);
                for index in 0..32 {
                    pixels
                        .set_pixel(
                            index * 256,
                            0,
                            PixelValue::Rgba16([worker, index as u16, 32769, 65535]),
                        )
                        .unwrap();
                }
                for _ in 0..3 {
                    for index in 0..32 {
                        assert_eq!(
                            pixels.pixel(index * 256, 0).unwrap(),
                            PixelValue::Rgba16([worker, index as u16, 32769, 65535])
                        );
                    }
                }
            })
        })
        .collect();
    for worker in workers {
        worker.join().unwrap();
    }
    assert!(pool.stats().peak_cpu_bytes <= TILE * 8);
    assert_eq!(pool.stats().cpu_bytes, 0);
    assert_eq!(pool.stats().disk_bytes, 0);
}

#[test]
fn recently_used_composite_cache_survives_cold_source_admission() {
    let pool = pool(TILE * 4, TILE * 16);
    let mut pixels = TiledPixels::with_pool(PixelFormat::Rgba16, 8, pool.clone());
    for x in 0..3 {
        pixels
            .insert_tile(
                TileCoord::new(x, 0).unwrap(),
                PixelBuffer::Rgba16(vec![x as u16 + 1; 256 * 256 * 4]),
            )
            .unwrap();
    }
    let reservation = pool.reserve(MemoryKind::CpuCache, TILE).unwrap();
    let cache =
        strata_pixels::memory::CacheSlot::new(&pool, (vec![77u8; TILE as usize], reservation));
    assert!(cache.get().is_some());
    pixels
        .insert_tile(
            TileCoord::new(3, 0).unwrap(),
            PixelBuffer::Rgba16(vec![4; 256 * 256 * 4]),
        )
        .unwrap();
    let hot = cache
        .get()
        .expect("new cold source must not evict a more recently used composite");
    assert_eq!(hot.0[0], 77);
    assert!(pool.stats().disk_bytes >= TILE);
    assert!(pool.stats().peak_cpu_bytes <= TILE * 4);
}

#[test]
fn cache_registration_keeps_an_existing_reader_pinned_through_pressure() {
    let pool = pool(TILE * 2, TILE * 4);
    let reservation = pool.reserve(MemoryKind::CpuCache, TILE).unwrap();
    let reader = std::sync::Arc::new((vec![19u8; TILE as usize], reservation));
    let slot = strata_pixels::memory::CacheSlot::from_arc(&pool, reader.clone());
    pool.trim().unwrap();
    assert_eq!(slot.get().unwrap().0[0], 19);
    assert_eq!(pool.stats().cpu_cache_bytes, TILE);
    drop(reader);
    pool.trim().unwrap();
    assert!(slot.get().is_none());
    assert_eq!(pool.stats().cpu_cache_bytes, 0);
}
