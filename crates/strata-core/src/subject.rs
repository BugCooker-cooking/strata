use crate::{Document, DocumentError};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use strata_pixels::{
    CoverageRect, PixelBuffer, PixelFormat, TileCoord, TiledMask,
    memory::{MemoryKind, Reservation},
};

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SubjectParams {
    #[serde(default)]
    pub region: Option<CoverageRect>,
    #[serde(default)]
    pub accept_ambiguous: bool,
    #[serde(default)]
    pub mode: crate::commands::SelectionMode,
}
pub struct SubjectSelection {
    pub mask: TiledMask,
    pub diagnostics: Value,
    pub needs_confirmation: bool,
}
pub trait SubjectSelector: Send + Sync {
    fn select(
        &self,
        document: &Document,
        params: &SubjectParams,
        cancel: &(dyn Fn() -> bool + Sync),
    ) -> Result<SubjectSelection, String>;
}
pub struct CanvasPreview {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    _allocation: Reservation,
}
impl Document {
    pub fn preview_srgb(
        &self,
        region: Option<CoverageRect>,
        max_size: u32,
        cancel: &dyn Fn() -> bool,
    ) -> Result<CanvasPreview, DocumentError> {
        let frame = self.frame();
        let region = region.unwrap_or(CoverageRect {
            x: frame.x,
            y: frame.y,
            width: frame.width,
            height: frame.height,
        });
        region.validate()?;
        if !(32..=1024).contains(&max_size)
            || region.x < frame.x
            || region.y < frame.y
            || i128::from(region.x) + i128::from(region.width)
                > i128::from(frame.x) + i128::from(frame.width)
            || i128::from(region.y) + i128::from(region.height)
                > i128::from(frame.y) + i128::from(frame.height)
        {
            return Err(DocumentError::Invalid(
                "preview must be inside the artboard, max_size 32..1024".into(),
            ));
        }
        let scale = (f64::from(max_size) / f64::from(region.width.max(region.height))).min(1.0);
        let width = (f64::from(region.width) * scale).round().max(1.0) as u32;
        let height = (f64::from(region.height) * scale).round().max(1.0) as u32;
        let count = (width * height) as usize;
        let allocation = self
            .pool
            .reserve(MemoryKind::Transient, count as u64 * 64)?;
        let mut coords = Vec::with_capacity(count);
        let mut linear = vec![[0.0; 4]; count];
        for y in 0..height {
            for x in 0..width {
                let sx = region.x
                    + (((u64::from(x) * 2 + 1) * u64::from(region.width)) / (u64::from(width) * 2))
                        as i64;
                let sy = region.y
                    + (((u64::from(y) * 2 + 1) * u64::from(region.height))
                        / (u64::from(height) * 2)) as i64;
                let (coord, [px, py]) = TileCoord::locate(sx, sy);
                coords.push((
                    coord,
                    usize::from(py) * 256 + usize::from(px),
                    (y * width + x) as usize,
                ));
            }
        }
        coords.sort_unstable_by_key(|c| c.0);
        let mut compositor = crate::composite::Compositor::new(1);
        let mut last = None;
        for (coord, source, dest) in coords {
            if last.as_ref().is_none_or(|(c, _)| *c != coord) {
                if cancel() {
                    return Err(DocumentError::Invalid("preview cancelled".into()));
                }
                last = Some((coord, compositor.tile(self, coord)?));
            }
            linear[dest] = last.as_ref().expect("preview tile").1[source];
        }
        let srgb = strata_color::working_profile("srgb")?;
        let PixelBuffer::Rgba8(rgba) =
            strata_color::from_linear_p3(&linear, PixelFormat::Rgba8, &srgb)?
        else {
            unreachable!()
        };
        Ok(CanvasPreview {
            width,
            height,
            rgba,
            _allocation: allocation,
        })
    }
}
