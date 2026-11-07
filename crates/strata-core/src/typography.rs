//! Editable source plus a versioned, deterministic portable glyph raster.
use crate::DocumentError;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use strata_pixels::{
    CoverageRect, PixelBuffer, PixelFormat, TileCoord, TiledMask, TiledPixels,
    memory::{MemoryKind, MemoryPool},
};
pub use strata_text::{TextMetrics, TextSpec};
/// Retained local-coordinate operations replay from shaped source, never from a
/// previously published bitmap. Crop barriers preserve actual deletion order.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum TextOperation {
    Transform {
        matrix: [f64; 9],
        interpolation: crate::commands::TransformInterpolation,
    },
    Crop {
        bounds: Option<CoverageRect>,
    },
    Resize {
        origin: [i64; 2],
        source: [u32; 2],
        target: [u32; 2],
        interpolation: crate::commands::ResizeInterpolation,
    },
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TextDefinition {
    pub version: u32,
    pub spec: std::sync::Arc<TextSpec>,
    /// Straight encoded document working RGB codes at document depth.
    pub color: [u16; 4],
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[schemars(length(max = 32))]
    pub operations: Vec<TextOperation>,
}
impl TextDefinition {
    pub fn validate(&self, format: PixelFormat) -> Result<(), DocumentError> {
        if self.version != strata_text::VERSION
            || (format == PixelFormat::Rgba8 && self.color.iter().any(|v| *v > 255))
        {
            return Err(DocumentError::Invalid("text version/color depth".into()));
        }
        self.spec.validate().map_err(text_error)?;
        if self.operations.len() > 32 {
            return Err(DocumentError::ResourceLimit);
        }
        for op in &self.operations {
            match op {
                TextOperation::Transform { matrix, .. } => {
                    crate::geometry::Homography::new(*matrix)
                        .map_err(|e| DocumentError::Invalid(e.to_string()))?;
                }
                TextOperation::Crop { bounds: Some(b) } => b.validate()?,
                TextOperation::Crop { bounds: None } => {}
                TextOperation::Resize {
                    origin,
                    source,
                    target,
                    ..
                } => {
                    if origin.iter().any(|v| v.unsigned_abs() > 1_000_000_000_000)
                        || source
                            .iter()
                            .chain(target)
                            .any(|v| !(1..=100000).contains(v))
                    {
                        return Err(DocumentError::Invalid("text resize geometry".into()));
                    }
                }
            }
        }
        Ok(())
    }
    pub(crate) fn retain_crop(&mut self, crop: Option<&crate::GeneratedCrop>) {
        if let Some(crop) = crop {
            self.crop(crop.bounds);
        }
    }
    pub(crate) fn crop(&mut self, bounds: Option<CoverageRect>) {
        let bounds = if let Some(TextOperation::Crop { bounds: old }) = self.operations.last() {
            old.and_then(|a| bounds.and_then(|b| intersection(a, b)))
        } else {
            bounds
        };
        if matches!(self.operations.last(), Some(TextOperation::Crop { .. })) {
            self.operations.pop();
        }
        self.operations.push(TextOperation::Crop { bounds });
    }
    pub(crate) fn transform(
        &mut self,
        matrix: crate::geometry::Homography,
        interpolation: crate::commands::TransformInterpolation,
    ) -> Result<(), DocumentError> {
        let matrix =
            if let Some(TextOperation::Transform { matrix: old, .. }) = self.operations.last() {
                crate::geometry::Homography::new(*old)
                    .and_then(|old| old.then(&matrix))
                    .map_err(|e| DocumentError::Invalid(e.to_string()))?
            } else {
                matrix
            };
        if matches!(
            self.operations.last(),
            Some(TextOperation::Transform { .. })
        ) {
            self.operations.pop();
        }
        // Adjacent transforms compose before sampling. Returning to exact
        // identity regenerates original glyph coverage without resampling blur.
        if matrix.integer_translation() != Some([0, 0]) {
            self.operations.push(TextOperation::Transform {
                matrix: matrix.values(),
                interpolation,
            });
        }
        Ok(())
    }
}
pub(crate) fn transformed_layer(
    layer: &crate::Layer,
    transform: &crate::geometry::Homography,
    interpolation: crate::commands::TransformInterpolation,
    origin: [i64; 2],
    format: PixelFormat,
    pool: MemoryPool,
    cancel: &dyn Fn() -> bool,
) -> Result<crate::Layer, DocumentError> {
    let crate::LayerKind::Text { text } = &layer.info.kind else {
        return Err(DocumentError::Invalid("editable text required".into()));
    };
    let mut text = text.clone();
    text.retain_crop(layer.info.generated_crop.as_ref());
    text.transform(
        local_transform(transform, [layer.info.x, layer.info.y], origin)?,
        interpolation,
    )?;
    let (pixels, _) = raster(&text, format, None, pool, cancel)?;
    let mut result = layer.clone();
    result.pixels = pixels;
    result.info.kind = crate::LayerKind::Text { text };
    result.info.generated_crop = None;
    result.info.x = origin[0];
    result.info.y = origin[1];
    Ok(result)
}
pub(crate) struct ResizeText<'a> {
    pub frame: &'a crate::Frame,
    pub size: [u32; 2],
    pub interpolation: crate::commands::ResizeInterpolation,
}
pub(crate) fn resized_layer(
    layer: &crate::Layer,
    sizing: ResizeText<'_>,
    format: PixelFormat,
    pool: MemoryPool,
    work: &mut crate::geometry_resize::Work,
    cancel: &dyn Fn() -> bool,
) -> Result<crate::Layer, DocumentError> {
    let ResizeText {
        frame,
        size,
        interpolation,
    } = sizing;
    let crate::LayerKind::Text { text } = &layer.info.kind else {
        return Err(DocumentError::Invalid("editable text required".into()));
    };
    let mut text = text.clone();
    text.retain_crop(layer.info.generated_crop.as_ref());
    text.operations.push(TextOperation::Resize {
        origin: [
            frame
                .x
                .checked_sub(layer.info.x)
                .ok_or(DocumentError::ResourceLimit)?,
            frame
                .y
                .checked_sub(layer.info.y)
                .ok_or(DocumentError::ResourceLimit)?,
        ],
        source: [frame.width, frame.height],
        target: size,
        interpolation,
    });
    let (pixels, _) = raster_with_work(&text, format, None, pool, work, cancel)?;
    let mut result = layer.clone();
    result.pixels = pixels;
    result.info.kind = crate::LayerKind::Text { text };
    result.info.generated_crop = None;
    Ok(result)
}
pub(crate) fn local_transform(
    transform: &crate::geometry::Homography,
    offset: [i64; 2],
    origin: [i64; 2],
) -> Result<crate::geometry::Homography, DocumentError> {
    use crate::geometry::Homography;
    let [x, y] = offset.map(|v| v as f64);
    let [ox, oy] = origin.map(|v| v as f64);
    Homography::affine([1., 0., 0., 1., x, y])
        .and_then(|t| t.then(transform))
        .and_then(|t| t.then(&Homography::affine([1., 0., 0., 1., -ox, -oy])?))
        .map_err(|e| DocumentError::Invalid(e.to_string()))
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
fn replay(
    mut mask: Option<TiledMask>,
    operations: &[TextOperation],
    pool: MemoryPool,
    resize_work: &mut crate::geometry_resize::Work,
    cancel: &dyn Fn() -> bool,
) -> Result<Option<TiledMask>, DocumentError> {
    let mut visits = 0u64;
    for op in operations {
        if cancel() {
            return Err(DocumentError::Cancelled);
        }
        let Some(source) = mask else { return Ok(None) };
        mask = match op {
            TextOperation::Crop { bounds } => bounds
                .and_then(|b| intersection(source.layout().bounds, b))
                .map(|bounds| crate::geometry_masks::rebind(&source, bounds, pool.clone(), cancel))
                .transpose()?,
            TextOperation::Transform {
                matrix,
                interpolation,
            } => {
                let t = crate::geometry::Homography::new(*matrix)
                    .map_err(|e| DocumentError::Invalid(e.to_string()))?;
                let sampling = match interpolation {
                    crate::commands::TransformInterpolation::Nearest => {
                        crate::geometry_pixels::Sampling::Nearest
                    }
                    crate::commands::TransformInterpolation::Bilinear => {
                        crate::geometry_pixels::Sampling::Bilinear
                    }
                };
                let b = crate::geometry_pixels::transformed_bounds(
                    source.layout().bounds,
                    &t,
                    sampling,
                )?;
                visits += u64::from(b.width) * u64::from(b.height);
                if visits > 64_000_000 {
                    return Err(DocumentError::ResourceLimit);
                }
                Some(crate::geometry_masks::transform_mask(
                    &source,
                    [0, 0],
                    &t,
                    sampling,
                    pool.clone(),
                    cancel,
                )?)
            }
            TextOperation::Resize {
                origin,
                source: input,
                target,
                interpolation,
            } => {
                let frame = crate::Frame {
                    name: String::new(),
                    x: origin[0],
                    y: origin[1],
                    width: input[0],
                    height: input[1],
                };
                let geometry = crate::geometry_resize::Geometry::new(&frame, *target);
                let b = geometry.bounds(source.layout().bounds)?;
                visits += u64::from(b.width) * u64::from(b.height);
                if visits > 64_000_000 {
                    return Err(DocumentError::ResourceLimit);
                }
                Some(crate::geometry_resize::mask(
                    &source,
                    [0, 0],
                    geometry,
                    *interpolation,
                    pool.clone(),
                    resize_work,
                    cancel,
                )?)
            }
        };
    }
    Ok(mask)
}
/// A transparent black glyph has no nonzero RGBA cache codes, but it still has
/// editable source geometry. Only this uncommon empty-cache query needs layout.
pub(crate) fn ink_bounds(
    definition: &TextDefinition,
    format: PixelFormat,
    crop: Option<&crate::GeneratedCrop>,
    pool: MemoryPool,
    cancel: &dyn Fn() -> bool,
) -> Result<Option<CoverageRect>, DocumentError> {
    let mut definition = definition.clone();
    definition.retain_crop(crop);
    definition.validate(format)?;
    let rendered =
        strata_text::render(&definition.spec, pool.clone(), cancel).map_err(text_error)?;
    replay(
        rendered.coverage,
        &definition.operations,
        pool,
        &mut crate::geometry_resize::Work::default(),
        cancel,
    )?
    .as_ref()
    .map(|mask| crate::geometry_masks::coverage_bounds(mask, cancel))
    .transpose()
    .map(Option::flatten)
}
pub(crate) fn text_error(error: strata_text::TextError) -> DocumentError {
    match error {
        strata_text::TextError::Cancelled => DocumentError::Cancelled,
        strata_text::TextError::Limit => DocumentError::ResourceLimit,
        other => DocumentError::Invalid(other.to_string()),
    }
}
pub(crate) fn raster(
    definition: &TextDefinition,
    format: PixelFormat,
    crop: Option<&crate::GeneratedCrop>,
    pool: MemoryPool,
    cancel: &dyn Fn() -> bool,
) -> Result<(TiledPixels, TextMetrics), DocumentError> {
    raster_with_work(
        definition,
        format,
        crop,
        pool,
        &mut crate::geometry_resize::Work::default(),
        cancel,
    )
}
fn raster_with_work(
    definition: &TextDefinition,
    format: PixelFormat,
    crop: Option<&crate::GeneratedCrop>,
    pool: MemoryPool,
    work: &mut crate::geometry_resize::Work,
    cancel: &dyn Fn() -> bool,
) -> Result<(TiledPixels, TextMetrics), DocumentError> {
    definition.validate(format)?;
    let rendered =
        strata_text::render(&definition.spec, pool.clone(), cancel).map_err(text_error)?;
    let mut output =
        TiledPixels::with_pool(format, crate::document::tile_limit(format), pool.clone());
    let coverage = replay(
        rendered.coverage,
        &definition.operations,
        pool.clone(),
        work,
        cancel,
    )?;
    if let Some(mask) = coverage {
        for (coord, coverage) in mask.tiles() {
            if cancel() {
                return Err(DocumentError::Cancelled);
            }
            let _scratch = pool.reserve(MemoryKind::Transient, 256 * 256 * 16)?;
            let coverage = coverage?;
            let [ox, oy] = coord.origin();
            let mut tile = match format {
                PixelFormat::Rgba8 => PixelBuffer::Rgba8(vec![0; 256 * 256 * 4]),
                PixelFormat::Rgba16 => PixelBuffer::Rgba16(vec![0; 256 * 256 * 4]),
            };
            for row in 0..256 {
                if cancel() {
                    return Err(DocumentError::Cancelled);
                }
                for col in 0..256 {
                    let i = row * 256 + col;
                    if coverage[i] == 0
                        || crop.is_some_and(|c| {
                            c.bounds
                                .is_none_or(|b| !b.contains(ox + col as i64, oy + row as i64))
                        })
                    {
                        continue;
                    }
                    let mut color = definition.color;
                    color[3] =
                        ((u64::from(color[3]) * u64::from(coverage[i]) + 32767) / 65535) as u16;
                    match &mut tile {
                        PixelBuffer::Rgba8(v) => {
                            for c in 0..4 {
                                v[4 * i + c] = color[c] as u8
                            }
                        }
                        PixelBuffer::Rgba16(v) => v[4 * i..4 * i + 4].copy_from_slice(&color),
                    }
                }
            }
            output.insert_tile(TileCoord::new(coord.x(), coord.y())?, tile)?;
        }
    }
    Ok((output, rendered.metrics))
}
