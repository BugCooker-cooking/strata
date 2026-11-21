//! Vector masks reference editable work paths. Raster and vector coverage
//! are sampled independently and multiplied only at composition time.
use crate::{
    Document, DocumentError, Layer, LayerInfo, PathId,
    geometry::{GeometryError, Homography, Point},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::hash::{Hash, Hasher};
use strata_pixels::CoverageRect;

const IDENTITY: [f64; 9] = [1., 0., 0., 0., 1., 0., 0., 0., 1.];
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VectorMask {
    pub path: PathId,
    pub enabled: bool,
    pub inverted: bool,
    pub linked: bool,
    pub anchor: [i64; 2],
    pub matrix: [f64; 9],
}
fn invalid(e: GeometryError) -> DocumentError {
    DocumentError::Invalid(e.to_string())
}
impl VectorMask {
    pub(crate) fn new(path: PathId, info: &LayerInfo, linked: bool) -> Self {
        Self {
            path,
            enabled: true,
            inverted: false,
            linked,
            anchor: [info.x, info.y],
            matrix: IDENTITY,
        }
    }
    pub(crate) fn effective(&self, info: &LayerInfo) -> Result<Homography, DocumentError> {
        let m = Homography::new(self.matrix).map_err(invalid)?;
        if !self.linked || self.anchor == [info.x, info.y] {
            return Ok(m);
        }
        let offset = [
            (i128::from(info.x) - i128::from(self.anchor[0])) as f64,
            (i128::from(info.y) - i128::from(self.anchor[1])) as f64,
        ];
        m.then(&Homography::affine([1., 0., 0., 1., offset[0], offset[1]]).map_err(invalid)?)
            .map_err(invalid)
    }
    pub(crate) fn relink(&mut self, info: &LayerInfo, linked: bool) -> Result<(), DocumentError> {
        if self.linked != linked {
            self.matrix = self.effective(info)?.values();
            self.anchor = [info.x, info.y];
            self.linked = linked;
        }
        Ok(())
    }
    /// Whole-image edits also transform the path's coordinate system. A
    /// layer-only transform moves the placement while retaining the source.
    pub(crate) fn transform(
        target: &mut LayerInfo,
        old: &LayerInfo,
        transform: &Homography,
        whole_image: bool,
    ) -> Result<(), DocumentError> {
        let Some(mask) = &old.vector_mask else {
            return Ok(());
        };
        if !whole_image && !mask.linked {
            return Ok(());
        }
        let current = mask.effective(old)?;
        let matrix = if whole_image && current.values() == IDENTITY {
            IDENTITY
        } else if whole_image {
            transform
                .inverted()
                .then(&current)
                .and_then(|m| m.then(transform))
                .map_err(invalid)?
                .values()
        } else {
            current.then(transform).map_err(invalid)?.values()
        };
        let new = target.vector_mask.as_mut().expect("retained layer mask");
        new.matrix = matrix;
        new.anchor = [target.x, target.y];
        Ok(())
    }
    pub(crate) fn source<'a>(
        &self,
        doc: &'a Document,
    ) -> Result<&'a crate::WorkPath, DocumentError> {
        doc.paths()
            .iter()
            .find(|p| p.id == self.path)
            .map(AsRef::as_ref)
            .ok_or_else(|| {
                DocumentError::Invalid("vector mask refers to a missing work path".into())
            })
    }
    pub(crate) fn contours(
        &self,
        doc: &Document,
        info: &LayerInfo,
    ) -> Result<Vec<Vec<Point>>, DocumentError> {
        let m = self.effective(info)?;
        self.source(doc)?
            .mapped_polygons((m.values() != IDENTITY).then_some(&m))
    }
}
pub(crate) fn hash_mask(doc: &Document, layer: &Layer, hash: &mut impl Hasher) {
    let mask = layer.info.vector_mask.as_ref().filter(|m| m.enabled);
    mask.is_some().hash(hash);
    if let Some(m) = mask {
        m.path.hash(hash);
        m.inverted.hash(hash);
        m.linked.hash(hash);
        m.anchor.hash(hash);
        m.matrix.map(f64::to_bits).hash(hash);
        if let Ok(path) = m.source(doc) {
            path.open_contours.hash(hash);
            for contour in path.contours() {
                contour.len().hash(hash);
                for a in contour {
                    for p in [a.position, a.incoming, a.outgoing] {
                        p.map(f64::to_bits).hash(hash);
                    }
                }
            }
        }
    }
}
/// Shared even-odd coverage at four subpixel samples per axis. The output
/// starts zeroed and covers either an artboard selection or a single tile.
pub(crate) fn rasterize(
    contours: &[Vec<Point>],
    bounds: CoverageRect,
    values: &mut [u16],
    cancel: &dyn Fn() -> bool,
) -> Result<(), DocumentError> {
    bounds.validate()?;
    let mut low = f64::INFINITY;
    let mut high = f64::NEG_INFINITY;
    let mut edges = Vec::new();
    for contour in contours {
        for (&a, &b) in contour.iter().zip(contour.iter().cycle().skip(1)) {
            low = low.min(a[1]);
            high = high.max(a[1]);
            if a[1] != b[1] {
                edges.push((a, b));
            }
        }
    }
    let min_y = (low.floor() as i64).max(bounds.y);
    let max_y = (high.ceil() as i64).min(bounds.y + i64::from(bounds.height));
    let mut crossings = Vec::with_capacity(edges.len());
    for y in min_y..max_y {
        if cancel() {
            return Err(DocumentError::Cancelled);
        }
        for sy in 0..4 {
            let scan = y as f64 + (f64::from(sy) + 0.5) / 4.0;
            crossings.clear();
            for (a, b) in &edges {
                if (a[1] <= scan && b[1] > scan) || (b[1] <= scan && a[1] > scan) {
                    crossings.push(a[0] + (scan - a[1]) * (b[0] - a[0]) / (b[1] - a[1]));
                }
            }
            crossings.sort_by(f64::total_cmp);
            for span in crossings.chunks_exact(2) {
                let left = span[0].max(bounds.x as f64);
                let right = span[1].min(bounds.x as f64 + f64::from(bounds.width));
                if right <= left {
                    continue;
                }
                // Count subpixel centers on [left, right) in local coordinates.
                let subpixel =
                    |x: f64| ((x - bounds.x as f64) * 4.0 - 0.5).ceil().max(0.0) as usize;
                let start = subpixel(left).min(bounds.width as usize * 4);
                let end = subpixel(right).min(bounds.width as usize * 4);
                if start == end {
                    continue;
                }
                let row = (y - bounds.y) as usize * bounds.width as usize;
                let first = start / 4;
                let last = (end - 1) / 4;
                if first == last {
                    values[row + first] += (end - start) as u16;
                } else {
                    values[row + first] += (4 - start % 4) as u16;
                    for v in &mut values[row + first + 1..row + last] {
                        *v += 4;
                    }
                    values[row + last] += ((end - 1) % 4 + 1) as u16;
                }
            }
        }
    }
    for value in values {
        *value = (u32::from(*value) * 65535 / 16) as u16;
    }
    Ok(())
}
