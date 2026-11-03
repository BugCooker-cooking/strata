//! ImageIO is used only here. Decode the primary image, retain its RGB profile
//! and integer depth, normalize EXIF orientation, then return straight RGBA.
use std::{ffi::c_void, os::unix::ffi::OsStrExt, path::Path, ptr};
use strata_io::{DecodedImage, IoError};
use strata_pixels::{PixelBuffer, PixelFormat};
#[cfg(test)]
#[path = "imageio_tests.rs"]
mod tests;

type Ref = *const c_void;

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRelease(value: Ref);
    fn CFURLCreateFromFileSystemRepresentation(
        allocator: Ref,
        bytes: *const u8,
        len: isize,
        directory: bool,
    ) -> Ref;
    fn CFDictionaryGetValue(dict: Ref, key: Ref) -> Ref;
    fn CFNumberGetValue(number: Ref, kind: isize, value: *mut i64) -> bool;
    fn CFNumberGetTypeID() -> usize;
    fn CFGetTypeID(value: Ref) -> usize;
    fn CFDataGetLength(data: Ref) -> isize;
    fn CFDataGetBytePtr(data: Ref) -> *const u8;
}
#[link(name = "ImageIO", kind = "framework")]
unsafe extern "C" {
    fn CGImageSourceCreateWithURL(url: Ref, options: Ref) -> Ref;
    fn CGImageSourceCopyPropertiesAtIndex(source: Ref, index: usize, options: Ref) -> Ref;
    fn CGImageSourceCreateImageAtIndex(source: Ref, index: usize, options: Ref) -> Ref;
    fn CGImageSourceGetPrimaryImageIndex(source: Ref) -> usize;
    static kCGImagePropertyPixelWidth: Ref;
    static kCGImagePropertyPixelHeight: Ref;
    static kCGImagePropertyDepth: Ref;
    static kCGImagePropertyOrientation: Ref;
    static kCGImagePropertyDPIWidth: Ref;
}
#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGImageGetWidth(image: Ref) -> usize;
    fn CGImageGetHeight(image: Ref) -> usize;
    fn CGImageGetBitsPerComponent(image: Ref) -> usize;
    fn CGImageGetBitsPerPixel(image: Ref) -> usize;
    fn CGImageGetBitmapInfo(image: Ref) -> u32;
    fn CGImageGetBytesPerRow(image: Ref) -> usize;
    fn CGImageGetDataProvider(image: Ref) -> Ref;
    fn CGDataProviderCopyData(provider: Ref) -> Ref;
    fn CGImageGetColorSpace(image: Ref) -> Ref;
    fn CGColorSpaceGetModel(space: Ref) -> i32;
    fn CGColorSpaceCopyICCData(space: Ref) -> Ref;

}

struct Owned(Ref);
impl Drop for Owned {
    fn drop(&mut self) {
        // SAFETY: each Owned contains a non-null Create/Copy result, released once.
        unsafe { CFRelease(self.0) }
    }
}
fn number(dict: Ref, key: Ref) -> Option<i64> {
    // SAFETY: ImageIO property dictionaries contain CFNumbers for these keys.
    unsafe {
        let value = CFDictionaryGetValue(dict, key);
        let mut out = 0;
        (!value.is_null()
            && CFGetTypeID(value) == CFNumberGetTypeID()
            && CFNumberGetValue(value, 4, &mut out))
        .then_some(out)
    }
}

pub(super) fn decoded_bytes(path: &Path, limit: u64) -> Result<u64, IoError> {
    let resource = || IoError::ResourceLimit {
        path: path.to_owned(),
        limit,
    };
    let failed = || IoError::Decode {
        path: path.to_owned(),
        reason: "ImageIO could not read HEIC dimensions".into(),
    };
    // SAFETY: owned Create/Copy results are checked before property access.
    unsafe {
        let name = path.as_os_str().as_bytes();
        let url = CFURLCreateFromFileSystemRepresentation(
            ptr::null(),
            name.as_ptr(),
            name.len() as isize,
            false,
        );
        if url.is_null() {
            return Err(failed());
        }
        let url = Owned(url);
        let source = CGImageSourceCreateWithURL(url.0, ptr::null());
        if source.is_null() {
            return Err(failed());
        }
        let source = Owned(source);
        let props = CGImageSourceCopyPropertiesAtIndex(
            source.0,
            CGImageSourceGetPrimaryImageIndex(source.0),
            ptr::null(),
        );
        if props.is_null() {
            return Err(failed());
        }
        let props = Owned(props);
        let width = number(props.0, kCGImagePropertyPixelWidth)
            .and_then(|v| u32::try_from(v).ok())
            .ok_or_else(resource)?;
        let height = number(props.0, kCGImagePropertyPixelHeight)
            .and_then(|v| u32::try_from(v).ok())
            .ok_or_else(resource)?;
        let format = if number(props.0, kCGImagePropertyDepth).unwrap_or(8) > 8 {
            PixelFormat::Rgba16
        } else {
            PixelFormat::Rgba8
        };
        let bytes = format.byte_count(width, height).map_err(|_| resource())? as u64;
        if bytes > limit {
            return Err(resource());
        }
        Ok(bytes)
    }
}

pub(super) fn decode(path: &Path, limit: u64) -> Result<DecodedImage, IoError> {
    let failed = |reason: &str| IoError::Decode {
        path: path.to_owned(),
        reason: reason.into(),
    };
    let resource = || IoError::ResourceLimit {
        path: path.to_owned(),
        limit,
    };
    let own = |raw: Ref| {
        if raw.is_null() {
            Err(failed("ImageIO could not decode the HEIC primary image"))
        } else {
            Ok(Owned(raw))
        }
    };
    // SAFETY: paths and all backing buffers outlive their synchronous API calls;
    // dimensions, row lengths and color models are checked before drawing.
    unsafe {
        let name = path.as_os_str().as_bytes();
        let url = own(CFURLCreateFromFileSystemRepresentation(
            ptr::null(),
            name.as_ptr(),
            name.len() as isize,
            false,
        ))?;
        let source = own(CGImageSourceCreateWithURL(url.0, ptr::null()))?;
        let index = CGImageSourceGetPrimaryImageIndex(source.0);
        let properties = own(CGImageSourceCopyPropertiesAtIndex(
            source.0,
            index,
            ptr::null(),
        ))?;
        let width = number(properties.0, kCGImagePropertyPixelWidth)
            .and_then(|v| u32::try_from(v).ok())
            .ok_or_else(resource)?;
        let height = number(properties.0, kCGImagePropertyPixelHeight)
            .and_then(|v| u32::try_from(v).ok())
            .ok_or_else(resource)?;
        let depth = number(properties.0, kCGImagePropertyDepth).unwrap_or(8);
        let format = if depth > 8 {
            PixelFormat::Rgba16
        } else {
            PixelFormat::Rgba8
        };
        if format.byte_count(width, height).map_err(|_| resource())? as u64 > limit {
            return Err(resource());
        }
        let orientation = number(properties.0, kCGImagePropertyOrientation).unwrap_or(1);
        let image = own(CGImageSourceCreateImageAtIndex(
            source.0,
            index,
            ptr::null(),
        ))?;
        if CGImageGetWidth(image.0) != width as usize
            || CGImageGetHeight(image.0) != height as usize
        {
            return Err(failed("ImageIO dimensions changed during decoding"));
        }
        let space = CGImageGetColorSpace(image.0);
        if space.is_null() || CGColorSpaceGetModel(space) != 1 {
            return Err(IoError::Unsupported {
                path: path.to_owned(),
                reason: "HEIC requires an RGB image and profile".into(),
            });
        }
        let bits = CGImageGetBitsPerComponent(image.0);
        if !matches!(bits, 8 | 10 | 16) {
            return Err(failed(&format!(
                "unsupported HEIC component depth {bits}, bpp {}, bitmap {}",
                CGImageGetBitsPerPixel(image.0),
                CGImageGetBitmapInfo(image.0)
            )));
        }
        let format = if bits > 8 {
            PixelFormat::Rgba16
        } else {
            PixelFormat::Rgba8
        };
        if format.byte_count(width, height).map_err(|_| resource())? as u64 > limit {
            return Err(resource());
        }
        let profile = CGColorSpaceCopyICCData(space);
        let icc_profile = if profile.is_null() {
            None
        } else {
            let profile = Owned(profile);
            let len = CFDataGetLength(profile.0);
            if !(1..=4 * 1024 * 1024).contains(&len) {
                return Err(resource());
            }
            Some(std::slice::from_raw_parts(CFDataGetBytePtr(profile.0), len as usize).to_vec())
        };
        let pixels =
            integer_pixels(image.0, width, height, limit).map_err(|reason| failed(&reason))?;
        let (width, height, pixels) = orient(width, height, pixels, orientation);
        Ok(DecodedImage {
            resolution_dpi: number(properties.0, kCGImagePropertyDPIWidth)
                .and_then(|v| u32::try_from(v).ok())
                .filter(|v| (1..=2400).contains(v))
                .unwrap_or(72),
            width,
            height,
            pixels,
            icc_profile,
        })
    }
}

/// Read integer decoder samples directly: drawing into a premultiplied 8-bit
/// context would round low-alpha colors before the document ever receives them.
fn integer_pixels(image: Ref, width: u32, height: u32, limit: u64) -> Result<PixelBuffer, String> {
    // SAFETY: image is retained by the caller; the immutable provider, integer
    // layout, stride and complete backing length are checked before indexing.
    unsafe {
        let bits = CGImageGetBitsPerComponent(image);
        let bpp = CGImageGetBitsPerPixel(image);
        let info = CGImageGetBitmapInfo(image);
        let alpha = info & 31;
        let order = info & 0x7000;
        let pixel_format = info & 0xf0000;
        if info & 0x100 != 0 {
            return Err("floating-point/HDR HEIC is not an integer RGB document".into());
        }
        let supported = match bits {
            8 => {
                pixel_format == 0
                    && ((bpp == 24 && alpha == 0 && matches!(order, 0 | 0x4000))
                        || (bpp == 32
                            && matches!(alpha, 1..=6)
                            && matches!(order, 0 | 0x2000 | 0x4000)))
            }
            10 => {
                bpp == 32
                    && pixel_format == 0x30000
                    && matches!(alpha, 2 | 4 | 6)
                    && matches!(order, 0 | 0x2000 | 0x4000)
            }
            16 => {
                pixel_format == 0
                    && ((bpp == 48 && alpha == 0) || (bpp == 64 && matches!(alpha, 1..=6)))
                    && matches!(order, 0 | 0x1000 | 0x3000)
            }
            _ => false,
        };
        if !supported {
            return Err(format!(
                "unsupported integer HEIC bitmap layout: depth={bits}, bpp={bpp}, flags={info:#x}"
            ));
        }
        let format = if bits == 8 {
            PixelFormat::Rgba8
        } else {
            PixelFormat::Rgba16
        };
        let count = format
            .sample_count(width, height)
            .map_err(|e| e.to_string())?;
        if format
            .byte_count(width, height)
            .map_err(|e| e.to_string())? as u64
            > limit
        {
            return Err("HEIC output exceeds decode limit".into());
        }
        let stride = CGImageGetBytesPerRow(image);
        let row_bytes = (width as usize)
            .checked_mul(bpp / 8)
            .ok_or("row size overflow")?;
        let length = stride
            .checked_mul(height as usize)
            .filter(|&n| n as u64 <= limit.saturating_mul(2))
            .ok_or("HEIC source stride exceeds limit")?;
        if stride < row_bytes {
            return Err("HEIC source row is truncated".into());
        }
        let provider = CGImageGetDataProvider(image);
        if provider.is_null() {
            return Err("missing HEIC data provider".into());
        }
        let data = CGDataProviderCopyData(provider);
        if data.is_null() {
            return Err("HEIC data provider failed".into());
        }
        let data = Owned(data);
        let length_available = CFDataGetLength(data.0);
        let pointer = CFDataGetBytePtr(data.0);
        if length_available < 0 || (length_available as usize) < length || pointer.is_null() {
            return Err("truncated HEIC source data".into());
        }
        let bytes = std::slice::from_raw_parts(pointer, length);
        let mut output = match format {
            PixelFormat::Rgba8 => PixelBuffer::Rgba8(Vec::with_capacity(count)),
            PixelFormat::Rgba16 => PixelBuffer::Rgba16(Vec::with_capacity(count)),
        };
        for y in 0..height as usize {
            for x in 0..width as usize {
                let offset = y * stride + x * (bpp / 8);
                let first = matches!(alpha, 2 | 4 | 6);
                let start = usize::from(first);
                let mut pixel = match bits {
                    8 => {
                        let read = |component: usize| {
                            u16::from(
                                bytes[offset
                                    + if bpp == 32 && order == 0x2000 {
                                        3 - component
                                    } else {
                                        component
                                    }],
                            )
                        };
                        [
                            read(start),
                            read(start + 1),
                            read(start + 2),
                            if matches!(alpha, 0 | 5 | 6) {
                                255
                            } else {
                                read(if first { 0 } else { 3 })
                            },
                        ]
                    }
                    10 => {
                        let b = bytes[offset..offset + 4].try_into().unwrap();
                        let word = if order == 0x2000 {
                            u32::from_le_bytes(b)
                        } else {
                            u32::from_be_bytes(b)
                        };
                        let shift = if first { 0 } else { 2 };
                        let scale = |v: u32| ((v * 65535 + 511) / 1023) as u16;
                        [
                            scale((word >> (20 + shift)) & 1023),
                            scale((word >> (10 + shift)) & 1023),
                            scale((word >> shift) & 1023),
                            if matches!(alpha, 5 | 6) {
                                65535
                            } else {
                                ((if first { word >> 30 } else { word & 3 }) * 21845) as u16
                            },
                        ]
                    }
                    16 => {
                        let read = |component: usize| {
                            let at = offset + component * 2;
                            let b = [bytes[at], bytes[at + 1]];
                            if order == 0x1000 {
                                u16::from_le_bytes(b)
                            } else {
                                u16::from_be_bytes(b)
                            }
                        };
                        [
                            read(start),
                            read(start + 1),
                            read(start + 2),
                            if matches!(alpha, 0 | 5 | 6) {
                                65535
                            } else {
                                read(if first { 0 } else { 3 })
                            },
                        ]
                    }
                    _ => unreachable!(),
                };
                if matches!(alpha, 1 | 2) && pixel[3] > 0 {
                    let maximum = if bits == 8 { 255 } else { 65535 };
                    for c in 0..3 {
                        pixel[c] = ((u64::from(pixel[c]) * maximum + u64::from(pixel[3]) / 2)
                            / u64::from(pixel[3]))
                        .min(maximum) as u16;
                    }
                }
                match &mut output {
                    PixelBuffer::Rgba8(v) => v.extend(pixel.map(|v| v as u8)),
                    PixelBuffer::Rgba16(v) => v.extend(pixel),
                }
            }
        }
        Ok(output)
    }
}

fn orient(
    width: u32,
    height: u32,
    pixels: PixelBuffer,
    orientation: i64,
) -> (u32, u32, PixelBuffer) {
    if !(2..=8).contains(&orientation) {
        return (width, height, pixels);
    }
    let (w, h) = if orientation >= 5 {
        (height, width)
    } else {
        (width, height)
    };
    fn rearrange<T: Copy + Default>(
        source: &[T],
        width: u32,
        height: u32,
        w: u32,
        orientation: i64,
    ) -> Vec<T> {
        let mut out = vec![T::default(); source.len()];
        for y in 0..height {
            for x in 0..width {
                let (dx, dy) = match orientation {
                    2 => (width - 1 - x, y),
                    3 => (width - 1 - x, height - 1 - y),
                    4 => (x, height - 1 - y),
                    5 => (y, x),
                    6 => (height - 1 - y, x),
                    7 => (height - 1 - y, width - 1 - x),
                    8 => (y, width - 1 - x),
                    _ => unreachable!(),
                };
                let src = (y as usize * width as usize + x as usize) * 4;
                let dst = (dy as usize * w as usize + dx as usize) * 4;
                out[dst..dst + 4].copy_from_slice(&source[src..src + 4]);
            }
        }
        out
    }
    let pixels = match pixels {
        PixelBuffer::Rgba8(v) => PixelBuffer::Rgba8(rearrange(&v, width, height, w, orientation)),
        PixelBuffer::Rgba16(v) => PixelBuffer::Rgba16(rearrange(&v, width, height, w, orientation)),
    };
    (w, h, pixels)
}
