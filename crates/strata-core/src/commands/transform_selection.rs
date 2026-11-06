//! Lift coverage once, warp an immutable 16-bit source and composite it over a
//! source-over complement. The old selection is never a destination write clip.
use super::*;
use crate::{
    Document, DocumentError,
    geometry::Homography,
    geometry_pixels::{self, PixelSource, Sampling},
};
use strata_pixels::{PixelBuffer, PixelFormat, TiledPixels, memory::MemoryKind};
fn check(cancel: &dyn Fn() -> bool) -> Result<(), DocumentError> {
    if cancel() {
        Err(DocumentError::Cancelled)
    } else {
        Ok(())
    }
}
fn codes(p: &PixelBuffer, i: usize) -> [u16; 4] {
    match p {
        PixelBuffer::Rgba8(v) => std::array::from_fn(|c| u16::from(v[i * 4 + c]) * 257),
        PixelBuffer::Rgba16(v) => std::array::from_fn(|c| v[i * 4 + c]),
    }
}
fn write(p: &mut PixelBuffer, i: usize, value: [u16; 4]) {
    match p {
        PixelBuffer::Rgba8(v) => {
            for c in 0..4 {
                v[i * 4 + c] = ((u32::from(value[c]) + 128) / 257) as u8;
            }
        }
        PixelBuffer::Rgba16(v) => p16(v, i, value),
    }
}
fn p16(v: &mut [u16], i: usize, value: [u16; 4]) {
    v[i * 4..i * 4 + 4].copy_from_slice(&value)
}
fn blank(format: PixelFormat) -> PixelBuffer {
    match format {
        PixelFormat::Rgba8 => PixelBuffer::Rgba8(vec![0; 256 * 256 * 4]),
        PixelFormat::Rgba16 => PixelBuffer::Rgba16(vec![0; 256 * 256 * 4]),
    }
}
pub(super) fn apply(
    doc: &mut Document,
    index: usize,
    transform: &Homography,
    sampling: Sampling,
    cancel: &dyn Fn() -> bool,
) -> Result<Value, DocumentError> {
    let selection = doc
        .selection
        .as_ref()
        .ok_or_else(|| {
            DocumentError::Invalid(
                "selected-pixel transform needs a nonempty document selection".into(),
            )
        })?
        .clone();
    let bounds = crate::geometry_masks::coverage_bounds(&selection, cancel)?
        .ok_or_else(|| DocumentError::Invalid("selection is empty".into()))?;
    let layer = doc.layers[index].clone();
    let offset = [layer.info.x, layer.info.y];
    let output_bounds = geometry_pixels::transformed_bounds(bounds, transform, sampling)?;
    let answer = || serde_json::json!({"layer_id":layer.id(),"scope":"selection","output_bounds":output_bounds,"source_artboard_clipped":false,"mask_transformed":false,"selection_transformed":true});
    if transform.integer_translation() == Some([0, 0]) {
        check(cancel)?;
        return Ok(answer());
    }
    let pool = doc.memory_pool();
    let mut lifted = TiledPixels::with_pool(
        PixelFormat::Rgba16,
        (crate::MAX_DOCUMENT_BYTES / (256 * 256 * 8)) as usize,
        pool.clone(),
    );
    let mut remainder = layer.pixels.clone();
    let mut visits = 0u64;
    for (coord, tile) in layer.pixels.tiles() {
        check(cancel)?;
        let [ox, oy] = coord.origin();
        let gx = ox
            .checked_add(offset[0])
            .ok_or(DocumentError::ResourceLimit)?;
        let gy = oy
            .checked_add(offset[1])
            .ok_or(DocumentError::ResourceLimit)?;
        if i128::from(gx) + 256 <= i128::from(bounds.x)
            || i128::from(gy) + 256 <= i128::from(bounds.y)
            || i128::from(gx) >= i128::from(bounds.x) + i128::from(bounds.width)
            || i128::from(gy) >= i128::from(bounds.y) + i128::from(bounds.height)
        {
            continue;
        }
        visits += 256 * 256;
        if visits > 64_000_000 {
            return Err(DocumentError::ResourceLimit);
        }
        let _scratch = pool.reserve(MemoryKind::Transient, 256 * 256 * 24)?;
        let tile = tile?;
        let weights = selection.region_tile(gx, gy)?;
        let mut rest = (*tile).clone();
        let mut selected = vec![0u16; 256 * 256 * 4];
        let mut changed = false;
        for row in 0..256 {
            check(cancel)?;
            for col in 0..256 {
                let i = row * 256 + col;
                let pixel = codes(&tile, i);
                let weight = weights[i];
                if weight == 0 || pixel[3] == 0 {
                    continue;
                }
                let selected_alpha =
                    ((u64::from(pixel[3]) * u64::from(weight) + 32767) / 65535) as u16;
                if selected_alpha == 0 {
                    continue;
                }
                let mut lifted_pixel = pixel;
                lifted_pixel[3] = selected_alpha;
                p16(&mut selected, i, lifted_pixel);
                // Solve s + r*(1-s) = a, rather than applying the old selection
                // again at the destination. Identity can reconstruct original alpha.
                let a = f64::from(pixel[3]) / 65535.;
                let s = f64::from(selected_alpha) / 65535.;
                let residual = if selected_alpha == 65535 {
                    0.
                } else {
                    (a - s) / (1. - s)
                };
                let mut remain = pixel;
                remain[3] = (residual.clamp(0., 1.) * 65535.).round() as u16;
                write(&mut rest, i, remain);
                changed = true;
            }
        }
        if changed {
            lifted.insert_tile(*coord, PixelBuffer::Rgba16(selected))?;
            remainder.insert_tile(*coord, rest)?;
        }
    }
    let source_bounds =
        geometry_pixels::content_bounds(&lifted, offset, cancel)?.ok_or_else(|| {
            DocumentError::Invalid("selection contains no nontransparent source pixels".into())
        })?;
    let warped = geometry_pixels::transform_pixels_at(
        PixelSource {
            pixels: &lifted,
            offset,
            bounds: source_bounds,
            icc: doc.working_icc(),
        },
        transform,
        sampling,
        offset,
        pool.clone(),
        cancel,
    )?;
    for (coord, tile) in warped.tiles() {
        check(cancel)?;
        let _scratch = pool.reserve(MemoryKind::Transient, 256 * 256 * 96)?;
        let front = tile?;
        let mut back = remainder
            .tile(*coord)?
            .map(|p| (*p).clone())
            .unwrap_or_else(|| blank(doc.format));
        let mut linear = strata_color::to_linear_p3(&back, 256, 256, doc.working_icc())?;
        let source = strata_color::to_linear_p3(&front, 256, 256, doc.working_icc())?;
        let mut mixed = vec![false; 256 * 256];
        let mut any = false;
        let mut alphas = vec![0u16; 256 * 256];
        for row in 0..256 {
            check(cancel)?;
            for col in 0..256 {
                let i = row * 256 + col;
                let f = codes(&front, i);
                if f[3] == 0 {
                    continue;
                };
                let b = codes(&back, i);
                let sa = f64::from(f[3]) / 65535.;
                let ba = f64::from(b[3]) / 65535.;
                let alpha = sa + ba * (1. - sa);
                alphas[i] = (alpha * 65535.).round() as u16;
                if b[3] == 0 || f[3] == 65535 || f[..3] == b[..3] {
                    let mut out = f;
                    out[3] = alphas[i];
                    write(&mut back, i, out);
                } else {
                    for c in 0..3 {
                        linear[i][c] = ((f64::from(source[i][c]) * sa
                            + f64::from(linear[i][c]) * ba * (1. - sa))
                            / alpha) as f32;
                    }
                    linear[i][3] = alpha as f32;
                    mixed[i] = true;
                }
                any = true;
            }
        }
        if mixed.iter().any(|v| *v) {
            let encoded = strata_color::from_linear_p3(&linear, doc.format, doc.working_icc())?;
            for (i, mix) in mixed.into_iter().enumerate() {
                if mix {
                    let mut out = codes(&encoded, i);
                    out[3] = alphas[i];
                    write(&mut back, i, out);
                }
            }
        }
        if any {
            remainder.insert_tile(*coord, back)?;
        }
    }
    // Document selections are rebound to the current frame after the warp.
    // Restrict only this temporary coverage to its occupied support so a
    // projective horizon outside the selected region cannot reject valid work.
    // Linked layer masks keep their full finite domains in the whole-layer path.
    let selected_coverage =
        crate::geometry_masks::rebind(&selection, bounds, pool.clone(), cancel)?;
    let transformed = crate::geometry_masks::transform_mask(
        &selected_coverage,
        [0, 0],
        transform,
        sampling,
        pool.clone(),
        cancel,
    )?;
    let f = doc.frame();
    let transformed = crate::geometry_masks::rebind(
        &transformed,
        strata_pixels::CoverageRect {
            x: f.x,
            y: f.y,
            width: f.width,
            height: f.height,
        },
        pool,
        cancel,
    )?;
    check(cancel)?;
    doc.layers[index].pixels = remainder;
    doc.previous_selection = Some(selection);
    doc.selection = Some(transformed);
    Ok(answer())
}
