use super::*;
use crate::NativeDecoder;
use strata_io::PlatformDecoder;
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFDataCreate(allocator: Ref, bytes: *const u8, len: isize) -> Ref;
    fn CFStringCreateWithCString(
        allocator: Ref,
        value: *const std::ffi::c_char,
        encoding: u32,
    ) -> Ref;
    fn CFNumberCreate(allocator: Ref, kind: isize, value: *const i32) -> Ref;
    fn CFDictionaryCreate(
        allocator: Ref,
        keys: *const Ref,
        values: *const Ref,
        count: isize,
        key_callbacks: Ref,
        value_callbacks: Ref,
    ) -> Ref;
    static kCFTypeDictionaryKeyCallBacks: [usize; 6];
    static kCFTypeDictionaryValueCallBacks: [usize; 5];
}
#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGColorSpaceCreateWithICCData(data: Ref) -> Ref;
    fn CGDataProviderCreateWithData(
        info: *mut c_void,
        bytes: *const c_void,
        len: usize,
        release: Option<unsafe extern "C" fn(*mut c_void, *const c_void, usize)>,
    ) -> Ref;
    fn CGImageCreate(
        width: usize,
        height: usize,
        depth: usize,
        bits: usize,
        stride: usize,
        space: Ref,
        info: u32,
        provider: Ref,
        decode: *const f64,
        interpolate: bool,
        intent: i32,
    ) -> Ref;
}
#[link(name = "ImageIO", kind = "framework")]
unsafe extern "C" {
    fn CGImageDestinationCreateWithURL(url: Ref, kind: Ref, count: usize, options: Ref) -> Ref;
    fn CGImageDestinationAddImage(destination: Ref, image: Ref, options: Ref);
    fn CGImageDestinationFinalize(destination: Ref) -> bool;
}
fn fixture(path: &Path, orientation: i32) {
    let icc = strata_color::working_profile("display_p3").unwrap();
    let pixels: Vec<u8> = (0..32 * 16)
        .flat_map(|i| {
            if i < 32 * 8 {
                [220, 30, 20, 255]
            } else {
                [20, 30, 220, 255]
            }
        })
        .collect();
    // SAFETY: test buffers outlive synchronous encoding; all Create results are
    // checked and released once. CoreGraphics retains its provider and space.
    unsafe {
        let data = Owned(CFDataCreate(ptr::null(), icc.as_ptr(), icc.len() as isize));
        let space = Owned(CGColorSpaceCreateWithICCData(data.0));
        assert!(!space.0.is_null());
        let provider = Owned(CGDataProviderCreateWithData(
            ptr::null_mut(),
            pixels.as_ptr().cast(),
            pixels.len(),
            None,
        ));
        assert!(!provider.0.is_null());
        let image = Owned(CGImageCreate(
            32,
            16,
            8,
            32,
            32 * 4,
            space.0,
            0x4005,
            provider.0,
            ptr::null(),
            false,
            0,
        ));
        assert!(!image.0.is_null());
        let encoded_path = path.with_extension("heic");
        let name = encoded_path.as_os_str().as_bytes();
        let url = Owned(CFURLCreateFromFileSystemRepresentation(
            ptr::null(),
            name.as_ptr(),
            name.len() as isize,
            false,
        ));
        assert!(!url.0.is_null());
        let kind = Owned(CFStringCreateWithCString(
            ptr::null(),
            c"public.heic".as_ptr(),
            0x08000100,
        ));
        assert!(!kind.0.is_null());
        let destination = Owned(CGImageDestinationCreateWithURL(
            url.0,
            kind.0,
            1,
            ptr::null(),
        ));
        assert!(!destination.0.is_null());
        let number = Owned(CFNumberCreate(ptr::null(), 3, &orientation));
        assert!(!number.0.is_null());
        let properties = Owned(CFDictionaryCreate(
            ptr::null(),
            [kCGImagePropertyOrientation].as_ptr(),
            [number.0].as_ptr(),
            1,
            ptr::addr_of!(kCFTypeDictionaryKeyCallBacks).cast(),
            ptr::addr_of!(kCFTypeDictionaryValueCallBacks).cast(),
        ));
        assert!(!properties.0.is_null());
        CGImageDestinationAddImage(destination.0, image.0, properties.0);
        assert!(
            CGImageDestinationFinalize(destination.0),
            "system HEIC encoder unavailable"
        );
        std::fs::copy(&encoded_path, path).unwrap();
    }
}
#[test]
fn native_heic_retains_p3_normalizes_orientation_and_uses_signature_not_extension() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("misnamed.png");
    fixture(&path, 6);
    let image = NativeDecoder.decode(&path, 1024 * 1024).unwrap();
    assert_eq!((image.width, image.height), (16, 32));
    assert!(
        strata_color::resolve_profile(image.icc_profile.as_deref())
            .unwrap()
            .name
            .contains("P3")
    );
    let PixelBuffer::Rgba8(pixels) = &image.pixels else {
        panic!("8-bit source was changed")
    };
    let left = &pixels[(16 * 16 + 2) * 4..][..4];
    let right = &pixels[(16 * 16 + 13) * 4..][..4];
    assert!(left[2] > left[0] + 100, "left={left:?}");
    assert!(right[0] > right[2] + 100, "right={right:?}");
    let mut editor = strata_core::Editor::with_decoder(std::sync::Arc::new(NativeDecoder));
    let read = editor.grant_read(&path, strata_core::Source::User).unwrap();
    let request = editor.request(strata_core::Source::User);
    editor
        .execute(
            request,
            "file.open",
            serde_json::json!({"resource":read}),
            |_| Ok(()),
        )
        .unwrap();
    assert_eq!(
        editor.document().unwrap().export_pixels().unwrap(),
        image.pixels
    );
    let request = editor.request(strata_core::Source::User);
    editor
        .execute(
            request,
            "file.place",
            serde_json::json!({"resource":read}),
            |_| Ok(()),
        )
        .unwrap();
    assert_eq!(editor.snapshot().history.len(), 1);
    let request = editor.request(strata_core::Source::User);
    editor
        .execute(request, "edit.undo", serde_json::json!({}), |_| Ok(()))
        .unwrap();
    assert_eq!(
        editor.document().unwrap().export_pixels().unwrap(),
        image.pixels
    );
    assert!(matches!(
        NativeDecoder.decode(&path, 16),
        Err(IoError::ResourceLimit { .. })
    ));
    assert!(matches!(
        NativeDecoder.decoded_bytes(&path, 16),
        Err(IoError::ResourceLimit { .. })
    ));
    std::fs::write(&path, b"\0\0\0\x18ftypheic\0\0\0\0heic").unwrap();
    assert!(NativeDecoder.decode(&path, 1024 * 1024).is_err());
}
#[test]
fn all_exif_orientations_preserve_integer_samples_and_channel_order() {
    let source = PixelBuffer::Rgba16((1u16..=6).flat_map(|i| [i, 32768, 32769, 65535]).collect());
    for (orientation, expected) in [
        (1, vec![1, 2, 3, 4, 5, 6]),
        (2, vec![3, 2, 1, 6, 5, 4]),
        (3, vec![6, 5, 4, 3, 2, 1]),
        (4, vec![4, 5, 6, 1, 2, 3]),
        (5, vec![1, 4, 2, 5, 3, 6]),
        (6, vec![4, 1, 5, 2, 6, 3]),
        (7, vec![6, 3, 5, 2, 4, 1]),
        (8, vec![3, 6, 2, 5, 1, 4]),
    ] {
        let (w, h, pixels) = orient(3, 2, source.clone(), orientation);
        assert_eq!((w, h), if orientation >= 5 { (2, 3) } else { (3, 2) });
        let PixelBuffer::Rgba16(data) = pixels else {
            panic!()
        };
        assert_eq!(
            data.chunks_exact(4).map(|p| p[0]).collect::<Vec<_>>(),
            expected
        );
        assert!(
            data.chunks_exact(4)
                .all(|p| p[1..] == [32768, 32769, 65535])
        );
    }
}

#[test]
fn system_generated_high_depth_heic_keeps_integer_depth() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("high-depth.heic");
    let icc = strata_color::working_profile("display_p3").unwrap();
    let pixels: Vec<u32> = (0..128 * 64)
        .map(|i| {
            let r = 100 + (i as u32 & 7);
            let g = 512 + (i as u32 & 1);
            let b = 780;
            let a = 3;
            (a << 30) | (r << 20) | (g << 10) | b
        })
        .collect();
    // SAFETY: all borrowed sample/profile buffers live through synchronous
    // encoding; owned CF objects are checked and released exactly once.
    unsafe {
        let data = Owned(CFDataCreate(ptr::null(), icc.as_ptr(), icc.len() as isize));
        let space = Owned(CGColorSpaceCreateWithICCData(data.0));
        assert!(!space.0.is_null());
        let provider = Owned(CGDataProviderCreateWithData(
            ptr::null_mut(),
            pixels.as_ptr().cast(),
            pixels.len() * 4,
            None,
        ));
        assert!(!provider.0.is_null());
        let image = Owned(CGImageCreate(
            128,
            64,
            10,
            32,
            128 * 4,
            space.0,
            0x32006,
            provider.0,
            ptr::null(),
            false,
            0,
        ));
        assert!(!image.0.is_null());
        let name = path.as_os_str().as_bytes();
        let url = Owned(CFURLCreateFromFileSystemRepresentation(
            ptr::null(),
            name.as_ptr(),
            name.len() as isize,
            false,
        ));
        let kind = Owned(CFStringCreateWithCString(
            ptr::null(),
            c"public.heic".as_ptr(),
            0x08000100,
        ));
        let destination = Owned(CGImageDestinationCreateWithURL(
            url.0,
            kind.0,
            1,
            ptr::null(),
        ));
        assert!(!destination.0.is_null());
        let ten = 10;
        let depth = Owned(CFNumberCreate(ptr::null(), 3, &ten));
        let properties = Owned(CFDictionaryCreate(
            ptr::null(),
            [kCGImagePropertyDepth].as_ptr(),
            [depth.0].as_ptr(),
            1,
            ptr::addr_of!(kCFTypeDictionaryKeyCallBacks).cast(),
            ptr::addr_of!(kCFTypeDictionaryValueCallBacks).cast(),
        ));
        CGImageDestinationAddImage(destination.0, image.0, properties.0);
        assert!(CGImageDestinationFinalize(destination.0));
    }
    let decoded = NativeDecoder.decode(&path, 1024 * 1024).unwrap();
    assert_eq!(
        decoded.pixels.format(),
        PixelFormat::Rgba16,
        "high-depth input was decoded at 8-bit"
    );
    assert_eq!((decoded.width, decoded.height), (128, 64));
    let PixelBuffer::Rgba16(pixels) = decoded.pixels else {
        panic!("missing high-depth pixels")
    };
    for pixel in pixels.chunks_exact(4).step_by(127) {
        assert!(
            pixel[0].abs_diff(6600) < 1500,
            "red channel/range mismatch: {pixel:?}"
        );
        assert!(
            pixel[1].abs_diff(32800) < 1500,
            "green channel/range mismatch: {pixel:?}"
        );
        assert!(
            pixel[2].abs_diff(50000) < 1500,
            "blue channel/range mismatch: {pixel:?}"
        );
        assert_eq!(pixel[3], 65535);
    }
}

#[test]
fn integer_16bit_bitmap_import_preserves_adjacent_codes_and_hidden_rgb() {
    let icc = strata_color::working_profile("display_p3").unwrap();
    let pixels = [32768u16, 32769, 50001, 0, 1, 2, 3, 12345];
    // SAFETY: backing samples and profile outlive all synchronous reads, and
    // each checked Create result is released once by Owned.
    unsafe {
        let data = Owned(CFDataCreate(ptr::null(), icc.as_ptr(), icc.len() as isize));
        let space = Owned(CGColorSpaceCreateWithICCData(data.0));
        assert!(!space.0.is_null());
        let provider = Owned(CGDataProviderCreateWithData(
            ptr::null_mut(),
            pixels.as_ptr().cast(),
            pixels.len() * 2,
            None,
        ));
        assert!(!provider.0.is_null());
        let image = Owned(CGImageCreate(
            2,
            1,
            16,
            64,
            16,
            space.0,
            0x1003,
            provider.0,
            ptr::null(),
            false,
            0,
        ));
        assert!(!image.0.is_null());
        assert_eq!(
            integer_pixels(image.0, 2, 1, 1024).unwrap(),
            PixelBuffer::Rgba16(pixels.to_vec())
        );
    }
}

#[test]
fn straight_8bit_bitmap_import_preserves_low_alpha_and_hidden_rgb() {
    let icc = strata_color::working_profile("display_p3").unwrap();
    let pixels = [172u8, 83, 29, 1, 180, 80, 20, 0];
    // SAFETY: sample/profile buffers outlive synchronous provider reads; all
    // checked Create results are owned and released once.
    unsafe {
        let data = Owned(CFDataCreate(ptr::null(), icc.as_ptr(), icc.len() as isize));
        let space = Owned(CGColorSpaceCreateWithICCData(data.0));
        assert!(!space.0.is_null());
        let provider = Owned(CGDataProviderCreateWithData(
            ptr::null_mut(),
            pixels.as_ptr().cast(),
            pixels.len(),
            None,
        ));
        assert!(!provider.0.is_null());
        let image = Owned(CGImageCreate(
            2,
            1,
            8,
            32,
            8,
            space.0,
            0x4003,
            provider.0,
            ptr::null(),
            false,
            0,
        ));
        assert!(!image.0.is_null());
        assert_eq!(
            integer_pixels(image.0, 2, 1, 1024).unwrap(),
            PixelBuffer::Rgba8(pixels.to_vec())
        );
    }
}
