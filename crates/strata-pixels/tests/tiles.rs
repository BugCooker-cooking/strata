use strata_pixels::{
    PixelBuffer, PixelFormat, PixelValue, TILE_EDGE, TileCoord, TileError, TiledPixels,
};

#[test]
fn negative_coordinates_use_euclidean_tiles() {
    for (x, tile, local) in [
        (-257, -2, 255),
        (-256, -1, 0),
        (-1, -1, 255),
        (0, 0, 0),
        (255, 0, 255),
        (256, 1, 0),
    ] {
        let (coord, [dx, dy]) = TileCoord::locate(x, x);
        assert_eq!((coord.x(), coord.y()), (tile, tile));
        assert_eq!((dx, dy), (local, local));
        let [ox, oy] = coord.origin();
        assert_eq!((ox + i64::from(dx), oy + i64::from(dy)), (x, x));
    }
}

#[test]
fn extreme_pixel_coordinates_roundtrip_without_overflow() {
    for x in [i64::MIN, i64::MIN + 255, i64::MAX - 255, i64::MAX] {
        let (coord, [dx, dy]) = TileCoord::locate(x, x);
        let [ox, oy] = coord.origin();
        assert_eq!([ox + i64::from(dx), oy + i64::from(dy)], [x, x]);
        assert_eq!(TileCoord::new(coord.x(), coord.y()).unwrap(), coord);
    }
    assert!(TileCoord::new(i64::MAX, 0).is_err());
    assert!(TileCoord::new(0, i64::MIN).is_err());
}

#[test]
fn absent_pixels_and_zero_writes_allocate_no_tiles() {
    let mut pixels = TiledPixels::new(PixelFormat::Rgba16, 0);
    assert_eq!(pixels.pixel(i64::MIN, i64::MAX), PixelValue::Rgba16([0; 4]));
    assert!(!pixels.set_pixel(-1, 0, PixelValue::Rgba16([0; 4])).unwrap());
    assert_eq!(pixels.tile_count(), 0);
    assert_eq!(pixels.referenced_bytes(), 0);
}

#[test]
fn writes_cross_tile_boundaries_at_both_depths() {
    for format in [PixelFormat::Rgba8, PixelFormat::Rgba16] {
        let mut pixels = TiledPixels::new(format, 5);
        let positions = [
            (-257, -1),
            (-256, -1),
            (-1, -1),
            (0, 0),
            (255, 255),
            (256, 256),
        ];
        for (i, &(x, y)) in positions.iter().enumerate() {
            let value = match format {
                PixelFormat::Rgba8 => PixelValue::Rgba8([i as u8 + 1, 128, 129, 255]),
                PixelFormat::Rgba16 => PixelValue::Rgba16([i as u16 + 1, 32768, 32769, 65535]),
            };
            assert!(pixels.set_pixel(x, y, value).unwrap());
            assert_eq!(pixels.pixel(x, y), value);
        }
        assert_eq!(pixels.tile_count(), 4);
        assert_eq!(
            pixels.referenced_bytes(),
            4 * 256 * 256 * 4 * format.bytes_per_sample() as u64
        );
        assert_eq!(
            pixels.pixel(-1, 0),
            match format {
                PixelFormat::Rgba8 => PixelValue::Rgba8([0; 4]),
                PixelFormat::Rgba16 => PixelValue::Rgba16([0; 4]),
            }
        );
    }
}

#[test]
fn transparent_nonzero_rgb_is_retained() {
    let mut pixels = TiledPixels::new(PixelFormat::Rgba16, 1);
    let hidden = PixelValue::Rgba16([1, 32768, 32769, 0]);
    pixels.set_pixel(-1, -1, hidden).unwrap();
    assert_eq!(pixels.pixel(-1, -1), hidden);
    assert_eq!(pixels.tile_count(), 1);
}

#[test]
fn wrong_depth_and_tile_limit_fail_without_mutating() {
    let mut pixels = TiledPixels::new(PixelFormat::Rgba8, 1);
    let value = PixelValue::Rgba8([1, 2, 3, 4]);
    pixels.set_pixel(0, 0, value).unwrap();
    assert!(matches!(
        pixels.set_pixel(0, 0, PixelValue::Rgba16([5; 4])),
        Err(TileError::FormatMismatch { .. })
    ));
    assert_eq!(
        pixels.set_pixel(256, 0, value),
        Err(TileError::TileLimit { limit: 1 })
    );
    assert_eq!(pixels.pixel(0, 0), value);
    assert_eq!(pixels.pixel(256, 0), PixelValue::Rgba8([0; 4]));
    assert_eq!(pixels.tile_count(), 1);
    // The cap limits new blocks, not writes into an existing block.
    assert!(pixels.set_pixel(255, 255, value).unwrap());
}

#[test]
fn copy_on_write_preserves_snapshot_and_shares_untouched_tile() {
    let mut pixels = TiledPixels::new(PixelFormat::Rgba16, 2);
    let a = TileCoord::new(-1, -1).unwrap();
    let b = TileCoord::new(1, 1).unwrap();
    let original = PixelValue::Rgba16([32768, 32769, 50001, 65535]);
    pixels.set_pixel(-1, -1, original).unwrap();
    pixels.set_pixel(256, 256, original).unwrap();
    let snapshot = pixels.clone();
    assert!(std::ptr::eq(
        pixels.tile(a).unwrap(),
        snapshot.tile(a).unwrap()
    ));
    pixels
        .set_pixel(-1, -1, PixelValue::Rgba16([7; 4]))
        .unwrap();
    assert_eq!(snapshot.pixel(-1, -1), original);
    assert_eq!(pixels.pixel(-1, -1), PixelValue::Rgba16([7; 4]));
    assert!(!std::ptr::eq(
        pixels.tile(a).unwrap(),
        snapshot.tile(a).unwrap()
    ));
    assert!(std::ptr::eq(
        pixels.tile(b).unwrap(),
        snapshot.tile(b).unwrap()
    ));
}

#[test]
fn equal_write_does_not_copy_or_report_change() {
    let mut pixels = TiledPixels::new(PixelFormat::Rgba8, 1);
    let value = PixelValue::Rgba8([1, 2, 3, 4]);
    pixels.set_pixel(0, 0, value).unwrap();
    let snapshot = pixels.clone();
    assert!(!pixels.set_pixel(0, 0, value).unwrap());
    let coord = TileCoord::new(0, 0).unwrap();
    assert!(std::ptr::eq(
        pixels.tile(coord).unwrap(),
        snapshot.tile(coord).unwrap()
    ));
}

#[test]
fn clearing_last_nonzero_pixel_reclaims_tile_but_keeps_snapshot() {
    let mut pixels = TiledPixels::new(PixelFormat::Rgba8, 1);
    pixels
        .set_pixel(0, 0, PixelValue::Rgba8([1, 0, 0, 0]))
        .unwrap();
    pixels
        .set_pixel(1, 0, PixelValue::Rgba8([0, 0, 0, 1]))
        .unwrap();
    let snapshot = pixels.clone();
    pixels.set_pixel(0, 0, PixelValue::Rgba8([0; 4])).unwrap();
    assert_eq!(pixels.tile_count(), 1);
    pixels.set_pixel(1, 0, PixelValue::Rgba8([0; 4])).unwrap();
    assert_eq!(pixels.tile_count(), 0);
    assert_eq!(snapshot.tile_count(), 1);
    assert_eq!(snapshot.pixel(0, 0), PixelValue::Rgba8([1, 0, 0, 0]));
    assert!(pixels.set_pixel(256, 0, PixelValue::Rgba8([1; 4])).unwrap());
}

#[test]
fn tile_iteration_is_stable_and_samples_are_row_major() {
    let mut pixels = TiledPixels::new(PixelFormat::Rgba16, 2);
    pixels
        .set_pixel(256, 0, PixelValue::Rgba16([9; 4]))
        .unwrap();
    pixels
        .set_pixel(1, 2, PixelValue::Rgba16([1, 2, 3, 4]))
        .unwrap();
    let coords: Vec<_> = pixels.tiles().map(|(coord, _)| *coord).collect();
    assert_eq!(
        coords,
        [TileCoord::new(0, 0).unwrap(), TileCoord::new(1, 0).unwrap()]
    );
    let PixelBuffer::Rgba16(samples) = pixels.tile(coords[0]).unwrap() else {
        panic!("wrong format")
    };
    let index = (2 * TILE_EDGE as usize + 1) * 4;
    assert_eq!(&samples[index..index + 4], &[1, 2, 3, 4]);
    assert_eq!(samples.len(), 256 * 256 * 4);
}
