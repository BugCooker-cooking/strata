use strata_pixels::{
    CoverageRect, MaskLayout, TileCoord, TiledMask,
    memory::{MemoryLimits, MemoryPool},
};
const BYTES: u64 = 256 * 256 * 2;
fn layout(default: u16) -> MaskLayout {
    MaskLayout {
        bounds: CoverageRect {
            x: -256,
            y: -256,
            width: 1024,
            height: 1024,
        },
        default_coverage: default,
        inverted: false,
    }
}
fn pool(cpu: u64, disk: u64) -> MemoryPool {
    MemoryPool::new(MemoryLimits {
        cpu_bytes: cpu,
        gpu_bytes: 0,
        disk_bytes: disk,
    })
}

#[test]
fn sparse_defaults_inverse_and_negative_bounds_preserve_coverage() {
    let mut mask = TiledMask::new(layout(65535), 8, pool(BYTES * 3, BYTES * 8)).unwrap();
    assert_eq!(mask.tile_count(), 0);
    assert_eq!(mask.referenced_bytes(), 0);
    assert_eq!(mask.sample(-256, -256).unwrap(), 65535);
    assert_eq!(mask.sample(-257, 0).unwrap(), 0);
    let mut samples = vec![65535; 256 * 256];
    samples[0] = 32768;
    samples[1] = 32769;
    mask.insert_tile(TileCoord::new(-1, -1).unwrap(), samples)
        .unwrap();
    let original = mask.clone();
    mask.invert();
    assert_eq!(mask.sample(-256, -256).unwrap(), 32767);
    assert_eq!(mask.sample(-255, -256).unwrap(), 32766);
    assert_eq!(mask.sample(0, 0).unwrap(), 0);
    assert_eq!(mask.sample(-257, 0).unwrap(), 0);
    assert_eq!(original.sample(-256, -256).unwrap(), 32768);
    assert_eq!(mask.referenced_bytes(), BYTES);
}

#[test]
fn copy_on_write_and_invalid_encoding_never_change_old_samples() {
    let mut mask = TiledMask::new(layout(0), 1, pool(BYTES * 3, BYTES * 8)).unwrap();
    let coord = TileCoord::new(0, 0).unwrap();
    mask.insert_tile(coord, vec![32768; 256 * 256]).unwrap();
    let old = mask.clone();
    let version = mask.tile_version(coord);
    mask.insert_tile(coord, vec![32769; 256 * 256]).unwrap();
    assert_ne!(mask.tile_version(coord), version);
    assert_eq!(old.sample(0, 0).unwrap(), 32768);
    assert_eq!(mask.sample(0, 0).unwrap(), 32769);
    assert!(mask.insert_encoded_tile(coord, vec![1; 3]).is_err());
    assert!(
        mask.insert_encoded_tile(TileCoord::new(1, 0).unwrap(), vec![1; 256 * 256])
            .is_err()
    );
    assert_eq!(mask.sample(0, 0).unwrap(), 32769);
    let mut bounded = TiledMask::new(
        MaskLayout {
            bounds: CoverageRect {
                x: 2,
                y: 2,
                width: 2,
                height: 2,
            },
            default_coverage: 0,
            inverted: false,
        },
        1,
        pool(BYTES * 2, BYTES * 4),
    )
    .unwrap();
    assert!(
        bounded
            .insert_encoded_tile(coord, vec![1; 256 * 256])
            .is_err()
    );
    bounded.insert_tile(coord, vec![65535; 256 * 256]).unwrap();
    assert_eq!(bounded.sample(2, 2).unwrap(), 65535);
    assert_eq!(bounded.sample(0, 0).unwrap(), 0);
    assert!(
        bounded
            .insert_encoded_tile(TileCoord::new(1, 1).unwrap(), vec![0; 256 * 256])
            .is_err()
    );
}

#[test]
fn global_source_budget_spills_independent_masks_and_retains_pinned_undo_blocks() {
    let pool = pool(BYTES * 3, BYTES * 24);
    let mut mask = TiledMask::new(layout(0), 16, pool.clone()).unwrap();
    let coord = TileCoord::new(0, 0).unwrap();
    mask.insert_tile(coord, vec![32768; 256 * 256]).unwrap();
    let undo = mask.clone();
    let pinned = undo.encoded_tile(coord).unwrap().unwrap();
    let mut others = Vec::new();
    for i in 1..=10 {
        let mut next = TiledMask::new(layout(0), 1, pool.clone()).unwrap();
        next.insert_tile(coord, vec![i; 256 * 256]).unwrap();
        others.push(next);
    }
    assert!(pool.stats().source_evictions > 0);
    assert!(pool.stats().peak_cpu_bytes <= BYTES * 3);
    mask.insert_tile(coord, vec![32769; 256 * 256]).unwrap();
    drop(pinned);
    pool.trim_sources().unwrap();
    assert_eq!(pool.stats().source_bytes, 0);
    assert_eq!(undo.sample(0, 0).unwrap(), 32768);
    assert_eq!(mask.sample(0, 0).unwrap(), 32769);
    for (i, other) in others.iter().enumerate() {
        assert_eq!(other.sample(0, 0).unwrap(), i as u16 + 1);
    }
    drop((mask, undo, others));
    assert_eq!(pool.stats().cpu_bytes, 0);
    assert_eq!(pool.stats().disk_bytes, 0);
}

#[test]
fn full_budget_failure_is_atomic_and_decoded_regions_match_individual_samples() {
    let constrained = pool(BYTES, 0);
    let mut mask = TiledMask::new(layout(0), 2, constrained.clone()).unwrap();
    let coord = TileCoord::new(0, 0).unwrap();
    mask.insert_tile(coord, vec![42; 256 * 256]).unwrap();
    let pin = mask.encoded_tile(coord).unwrap().unwrap();
    let version = mask.tile_version(coord);
    assert!(mask.insert_tile(coord, vec![43; 256 * 256]).is_err());
    assert_eq!(mask.tile_version(coord), version);
    assert_eq!(pin[0], 42);
    drop(pin);
    let mut mask = TiledMask::new(layout(65535), 8, pool(BYTES * 4, BYTES * 8)).unwrap();
    let samples: Vec<_> = (0..256 * 256).map(|i| (i % 65536) as u16).collect();
    mask.insert_tile(TileCoord::new(-1, -1).unwrap(), samples)
        .unwrap();
    let decoded = mask.region_tile(-255, -1).unwrap();
    for y in 0..256 {
        for x in 0..256 {
            assert_eq!(
                decoded[y * 256 + x],
                mask.sample(-255 + x as i64, -1 + y as i64).unwrap()
            );
        }
    }
    assert!(mask.region_tile(i64::MAX, 0).is_err());
    assert!(
        TiledMask::new(
            MaskLayout {
                bounds: CoverageRect {
                    x: i64::MAX,
                    y: 0,
                    width: 1,
                    height: 1
                },
                default_coverage: 0,
                inverted: false
            },
            1,
            constrained
        )
        .is_err()
    );
}
