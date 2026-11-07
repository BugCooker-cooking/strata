//! Editable source plus a versioned, deterministic portable glyph raster.
use crate::DocumentError;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use strata_pixels::{
    PixelBuffer, PixelFormat, TileCoord, TiledPixels,
    memory::{MemoryKind, MemoryPool},
};
pub use strata_text::{TextMetrics, TextSpec};
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TextDefinition {
    pub version: u32,
    pub spec: std::sync::Arc<TextSpec>,
    /// Straight encoded document working RGB codes at document depth.
    pub color: [u16; 4],
}
impl TextDefinition {
    pub fn validate(&self, format: PixelFormat) -> Result<(), DocumentError> {
        if self.version != strata_text::VERSION
            || (format == PixelFormat::Rgba8 && self.color.iter().any(|v| *v > 255))
        {
            return Err(DocumentError::Invalid("text version/color depth".into()));
        }
        self.spec.validate().map_err(text_error)
    }
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
    definition.validate(format)?;
    let rendered =
        strata_text::render(&definition.spec, pool.clone(), cancel).map_err(text_error)?;
    let mut output =
        TiledPixels::with_pool(format, crate::document::tile_limit(format), pool.clone());
    if let Some(mask) = rendered.coverage {
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
