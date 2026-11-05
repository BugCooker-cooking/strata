//! Finite coverage publication for local transforms. Callers prepare coverage
//! and transformed samples from immutable input; selection is included once.
use super::*;
use crate::{Document, DocumentError, LayerKind};
use serde_json::json;
use strata_pixels::{PixelBuffer, PixelFormat, TileCoord, TiledMask, memory::MemoryKind};
fn check(cancel: &dyn Fn() -> bool) -> Result<(), CommandError> {
    if cancel() {
        Err(CommandError::Expired)
    } else {
        Ok(())
    }
}
fn raw(buffer: &PixelBuffer, i: usize) -> [f64; 4] {
    match buffer {
        PixelBuffer::Rgba8(v) => std::array::from_fn(|c| f64::from(v[i * 4 + c]) / 255.0),
        PixelBuffer::Rgba16(v) => std::array::from_fn(|c| f64::from(v[i * 4 + c]) / 65535.0),
    }
}
pub(super) fn target(
    doc: &Document,
    layer: &LayerRef,
) -> Result<(crate::LayerId, usize), CommandError> {
    let target = doc.resolve(layer)?;
    let index = doc.layers().iter().position(|l| l.id() == target).unwrap();
    let info = doc.layers[index].info.clone();
    if !matches!(info.kind, LayerKind::Pixel)
        || doc.layers().iter().any(|l| {
            (l.id() == target || doc.is_descendant(target, l.id()))
                && (l.info().locks.all || l.info().locks.pixels)
        })
    {
        return Err(CommandError::Invalid(
            "local transform needs an unlocked pixel layer and ancestors".into(),
        ));
    }
    Ok((target, index))
}
pub(super) fn apply(
    doc: &mut Document,
    layer: LayerRef,
    coverage: &TiledMask,
    mut transform: impl FnMut([f32; 4], i64, i64) -> Result<[f32; 4], CommandError>,
    cancel: &dyn Fn() -> bool,
) -> Result<Value, CommandError> {
    check(cancel)?;
    let (target, index) = target(doc, &layer)?;
    let info = doc.layers[index].info.clone();
    let result = |tiles| json!({"layer_id":target,"target":"pixels","touched_tiles":tiles});
    let Some(b) = coverage.content_bounds().map_err(DocumentError::from)? else {
        return Ok(result(0));
    };
    let f = doc.frame();
    let x = b.x.max(f.x);
    let y = b.y.max(f.y);
    let r = (b.x + i64::from(b.width)).min(f.x + i64::from(f.width));
    let bottom = (b.y + i64::from(b.height)).min(f.y + i64::from(f.height));
    if x >= r || y >= bottom {
        return Ok(result(0));
    }
    let (left, top, right, bottom) = (
        x.checked_sub(info.x),
        y.checked_sub(info.y),
        (r - 1).checked_sub(info.x),
        (bottom - 1).checked_sub(info.y),
    );
    let (Some(left), Some(top), Some(right), Some(bottom)) = (left, top, right, bottom) else {
        return Err(DocumentError::ResourceLimit.into());
    };
    let (first, _) = TileCoord::locate(left, top);
    let (last, _) = TileCoord::locate(right, bottom);
    let mut touched = 0;
    for ty in first.y()..=last.y() {
        for tx in first.x()..=last.x() {
            check(cancel)?;
            let coord = TileCoord::new(tx, ty).map_err(DocumentError::from)?;
            let [ox, oy] = coord.origin();
            let gx = ox.checked_add(info.x).ok_or(DocumentError::ResourceLimit)?;
            let gy = oy.checked_add(info.y).ok_or(DocumentError::ResourceLimit)?;
            let _budget = doc
                .memory_pool()
                .reserve(MemoryKind::Transient, 256 * 256 * 100)
                .map_err(DocumentError::from)?;
            let weights = coverage.region_tile(gx, gy).map_err(DocumentError::from)?;
            if weights.iter().all(|v| *v == 0) {
                continue;
            }
            let mut pixels = doc.layers[index]
                .pixels
                .tile(coord)
                .map_err(DocumentError::from)?
                .map(|v| (*v).clone())
                .unwrap_or_else(|| match doc.format {
                    PixelFormat::Rgba8 => PixelBuffer::Rgba8(vec![0; 256 * 256 * 4]),
                    PixelFormat::Rgba16 => PixelBuffer::Rgba16(vec![0; 256 * 256 * 4]),
                });
            let mut linear = strata_color::to_linear_p3(&pixels, 256, 256, doc.working_icc())
                .map_err(DocumentError::from)?;
            let mut changed = vec![false; 256 * 256];
            let mut alphas = vec![0.0; 256 * 256];
            for row in 0..256 {
                check(cancel)?;
                for col in 0..256 {
                    let i = row * 256 + col;
                    let sx = ox + col as i64;
                    let sy = oy + row as i64;
                    if weights[i] == 0 || sx < left || sx > right || sy < top || sy > bottom {
                        continue;
                    }
                    let old = linear[i];
                    let codes = raw(&pixels, i);
                    if info.locks.transparency && codes[3] == 0.0 {
                        continue;
                    }
                    let after = transform(old, gx + col as i64, gy + row as i64)?;
                    if after == old {
                        continue;
                    }
                    if after.iter().any(|v| !v.is_finite()) || !(0.0..=1.0).contains(&after[3]) {
                        return Err(CommandError::Invalid(
                            "local transform produced invalid RGBA".into(),
                        ));
                    }
                    let w = f64::from(weights[i]) / 65535.0;
                    let alpha = if info.locks.transparency || after[3] == old[3] {
                        codes[3]
                    } else {
                        codes[3] + (f64::from(after[3]) - codes[3]) * w
                    };
                    if alpha == 0.0 {
                        if codes[3] > 0.0 {
                            alphas[i] = 0.0;
                            changed[i] = true;
                        }
                        continue;
                    }
                    for c in 0..3 {
                        linear[i][c] = if info.locks.transparency {
                            (f64::from(old[c]) + (f64::from(after[c]) - f64::from(old[c])) * w)
                                as f32
                        } else {
                            ((f64::from(old[c]) * codes[3] * (1.0 - w)
                                + f64::from(after[c]) * f64::from(after[3]) * w)
                                / alpha) as f32
                        };
                    }
                    linear[i][3] = alpha as f32;
                    alphas[i] = alpha;
                    changed[i] = true;
                }
            }
            if !changed.iter().any(|v| *v) {
                continue;
            }
            let encoded = strata_color::from_linear_p3(&linear, doc.format, doc.working_icc())
                .map_err(DocumentError::from)?;
            for (i, changed) in changed.into_iter().enumerate() {
                if changed {
                    if alphas[i] > 0.0 {
                        super::brush::copy_color(&mut pixels, &encoded, i);
                    }
                    super::brush::set_alpha(&mut pixels, i, alphas[i]);
                }
            }
            doc.layers[index]
                .pixels
                .insert_tile(coord, pixels)
                .map_err(DocumentError::from)?;
            touched += 1;
        }
    }
    Ok(result(touched))
}
