//! Explicit destructive crop keeps editable layer definitions and exact inside
//! codes. Generated fills get finite support, independently of gradient axes.
use crate::{Document, DocumentError, GeneratedCrop, LayerKind};
use strata_pixels::{CoverageRect, PixelBuffer, memory::MemoryKind};
fn check(cancel: &dyn Fn() -> bool) -> Result<(), DocumentError> {
    if cancel() {
        Err(DocumentError::Cancelled)
    } else {
        Ok(())
    }
}
fn intersection(a: CoverageRect, b: CoverageRect) -> Option<CoverageRect> {
    let x = a.x.max(b.x);
    let y = a.y.max(b.y);
    let r = (i128::from(a.x) + i128::from(a.width)).min(i128::from(b.x) + i128::from(b.width));
    let bottom =
        (i128::from(a.y) + i128::from(a.height)).min(i128::from(b.y) + i128::from(b.height));
    (i128::from(x) < r && i128::from(y) < bottom).then(|| CoverageRect {
        x,
        y,
        width: (r - i128::from(x)) as u32,
        height: (bottom - i128::from(y)) as u32,
    })
}
pub(super) fn apply(
    doc: &mut Document,
    bounds: CoverageRect,
    cancel: &dyn Fn() -> bool,
) -> Result<(), DocumentError> {
    check(cancel)?;
    if doc.layers.iter().any(|l| {
        let v = l.info().locks;
        v.all || v.pixels || v.transparency
    }) {
        return Err(DocumentError::Invalid("destructive crop would delete locked source or mask samples; unlock affected pixel/transparency/all locks first".into()));
    }
    super::canvas::freeze_references(doc);
    let source = doc.clone();
    let pool = doc.memory_pool();
    let mut visits = 0u64;
    for (index, old) in source.layers.iter().enumerate() {
        check(cancel)?;
        let info = old.info();
        // New editable kinds must define their finite destructive-crop policy.
        match &info.kind {
            LayerKind::Pixel
            | LayerKind::Text { .. }
            | LayerKind::Group
            | LayerKind::Solid { .. }
            | LayerKind::Gradient { .. }
            | LayerKind::Exposure { .. }
            | LayerKind::HueSaturation { .. }
            | LayerKind::Adjustment { .. } => {}
        }
        let local = CoverageRect {
            x: bounds
                .x
                .checked_sub(info.x)
                .ok_or(DocumentError::ResourceLimit)?,
            y: bounds
                .y
                .checked_sub(info.y)
                .ok_or(DocumentError::ResourceLimit)?,
            width: bounds.width,
            height: bounds.height,
        };
        local.validate()?;
        for coord in old.pixels.tile_coords() {
            check(cancel)?;
            let [ox, oy] = coord.origin();
            let tilebounds = CoverageRect {
                x: ox,
                y: oy,
                width: 256,
                height: 256,
            };
            let inside = intersection(tilebounds, local);
            if inside == Some(tilebounds) {
                continue;
            }
            let _scratch = pool.reserve(MemoryKind::Transient, 256 * 256 * 8)?;
            let mut tile = if inside.is_none() {
                match doc.format {
                    strata_pixels::PixelFormat::Rgba8 => PixelBuffer::Rgba8(vec![0; 256 * 256 * 4]),
                    strata_pixels::PixelFormat::Rgba16 => {
                        PixelBuffer::Rgba16(vec![0; 256 * 256 * 4])
                    }
                }
            } else {
                visits += 256 * 256;
                if visits > 64_000_000 {
                    return Err(DocumentError::ResourceLimit);
                }
                (*old.pixels.tile(*coord)?.expect("existing source tile")).clone()
            };
            if inside.is_some() {
                for y in 0..256 {
                    check(cancel)?;
                    for x in 0..256 {
                        if !local.contains(ox + x as i64, oy + y as i64) {
                            let i = (y * 256 + x) * 4;
                            match &mut tile {
                                PixelBuffer::Rgba8(v) => v[i..i + 4].fill(0),
                                PixelBuffer::Rgba16(v) => v[i..i + 4].fill(0),
                            }
                        }
                    }
                }
            }
            doc.layers[index].pixels.insert_tile(*coord, tile)?;
        }
        if let Some(mask) = old.mask() {
            let meta = info.mask.as_ref().unwrap();
            let maskbounds = CoverageRect {
                x: bounds
                    .x
                    .checked_sub(info.x)
                    .and_then(|x| x.checked_add(meta.anchor_x))
                    .ok_or(DocumentError::ResourceLimit)?,
                y: bounds
                    .y
                    .checked_sub(info.y)
                    .and_then(|y| y.checked_add(meta.anchor_y))
                    .ok_or(DocumentError::ResourceLimit)?,
                width: bounds.width,
                height: bounds.height,
            };
            let mask = crate::geometry_masks::rebind(mask, maskbounds, pool.clone(), cancel)?;
            doc.layers[index].info.mask.as_mut().unwrap().layout = mask.layout();
            doc.layers[index].mask = Some(mask);
        }
        if matches!(
            info.kind,
            LayerKind::Solid { .. } | LayerKind::Gradient { .. }
        ) {
            let target = &mut doc.layers[index].info;
            if let Some(geometry) = &mut target.generated_geometry {
                geometry.crop(local);
                geometry.validate([info.x, info.y])?;
            } else {
                let crop = source
                    .generated_canvas_bounds(old)?
                    .and_then(|b| intersection(b, bounds))
                    .map(|mut b| {
                        b.x -= info.x;
                        b.y -= info.y;
                        b
                    });
                target.generated_crop = Some(GeneratedCrop { bounds: crop });
            }
        }
        if let LayerKind::Text { text } = &info.kind {
            let mut text = text.clone();
            text.retain_crop(info.generated_crop.as_ref());
            text.crop(Some(local));
            text.validate(doc.format)?;
            doc.layers[index].info.kind = LayerKind::Text { text };
            doc.layers[index].info.generated_crop = None;
        }
    }
    check(cancel)
}
