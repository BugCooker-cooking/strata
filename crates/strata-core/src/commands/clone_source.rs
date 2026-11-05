use super::*;
use crate::{Document, DocumentError, LayerId, composite::Compositor};
use std::collections::HashMap;
use strata_pixels::{
    TileCoord,
    memory::{MemoryKind, Reservation},
};
pub(super) struct CloneSampler {
    document: Document,
    layer: LayerId,
    sample: SelectionSample,
    offset: [f64; 2],
    tiles: HashMap<TileCoord, Vec<[f32; 4]>>,
    compositor: Compositor,
    tile_loads: usize,
    tile_load_limit: usize,
    _budget: Reservation,
}
impl CloneSampler {
    pub fn new(
        doc: &Document,
        layer: LayerId,
        p: &CloneSource,
        first: &BrushPoint,
    ) -> Result<Self, CommandError> {
        let destination = p.destination.unwrap_or([first.x, first.y]);
        if p.source
            .iter()
            .chain(&destination)
            .any(|v| !v.is_finite() || v.abs() > 1e12)
        {
            return Err(CommandError::Invalid(
                "clone anchors must be finite canvas positions ±1e12".into(),
            ));
        }
        let f = doc.frame();
        if p.source[0] < f.x as f64
            || p.source[1] < f.y as f64
            || p.source[0] >= f.x as f64 + f64::from(f.width)
            || p.source[1] >= f.y as f64 + f64::from(f.height)
        {
            return Err(CommandError::Invalid(
                "clone source anchor must lie inside the artboard".into(),
            ));
        }
        let budget = doc
            .memory_pool()
            .reserve(MemoryKind::Transient, 10 * 1024 * 1024)
            .map_err(DocumentError::from)?;
        Ok(Self {
            document: doc.clone(),
            layer,
            sample: p.sample,
            offset: [p.source[0] - destination[0], p.source[1] - destination[1]],
            tiles: HashMap::new(),
            compositor: Compositor::new(1),
            tile_loads: 0,
            tile_load_limit: usize::MAX,
            _budget: budget,
        })
    }
    pub fn limit_tile_loads(&mut self, limit: usize) {
        self.tile_load_limit = limit;
    }
    fn point(
        &mut self,
        x: i64,
        y: i64,
        cancel: &dyn Fn() -> bool,
    ) -> Result<[f64; 4], CommandError> {
        if cancel() {
            return Err(CommandError::Expired);
        }
        let f = self.document.frame();
        if x < f.x
            || y < f.y
            || i128::from(x) >= i128::from(f.x) + i128::from(f.width)
            || i128::from(y) >= i128::from(f.y) + i128::from(f.height)
        {
            return Ok([0.0; 4]);
        }
        let layer = self
            .document
            .layers()
            .iter()
            .find(|l| l.id() == self.layer)
            .ok_or(DocumentError::LayerNotFound)?;
        let (x, y) = if matches!(self.sample, SelectionSample::CurrentLayer) {
            (
                x.checked_sub(layer.info().x)
                    .ok_or(DocumentError::ResourceLimit)?,
                y.checked_sub(layer.info().y)
                    .ok_or(DocumentError::ResourceLimit)?,
            )
        } else {
            (x, y)
        };
        let (coord, [px, py]) = TileCoord::locate(x, y);
        if !self.tiles.contains_key(&coord) {
            if self.tile_loads >= self.tile_load_limit {
                return Err(CommandError::Invalid(format!(
                    "texture search exceeds {} decoded source tiles; shrink the brush or search radius",
                    self.tile_load_limit
                )));
            }
            self.tile_loads += 1;
            if self.tiles.len() >= 8 {
                self.tiles.clear();
            }
            let values = match self.sample {
                SelectionSample::Merged => self
                    .compositor
                    .tile_prefix_cancellable(
                        &self.document,
                        coord,
                        self.document.layers().len(),
                        cancel,
                    )?
                    .iter()
                    .copied()
                    .collect(),
                SelectionSample::CurrentLayer => {
                    if let Some(tile) = layer.pixels().tile(coord).map_err(DocumentError::from)? {
                        strata_color::to_linear_p3(&tile, 256, 256, self.document.working_icc())
                            .map_err(DocumentError::from)?
                    } else {
                        vec![[0.0; 4]; 256 * 256]
                    }
                }
            };
            self.tiles.insert(coord, values);
        }
        Ok(self.tiles[&coord][usize::from(py) * 256 + usize::from(px)].map(f64::from))
    }
    pub fn color(
        &mut self,
        x: f64,
        y: f64,
        cancel: &dyn Fn() -> bool,
    ) -> Result<[f64; 4], CommandError> {
        let x = x + self.offset[0] - 0.5;
        let y = y + self.offset[1] - 0.5;
        let x0 = x.floor() as i64;
        let y0 = y.floor() as i64;
        let fx = x - x0 as f64;
        let fy = y - y0 as f64;
        let mut result = [0.0; 4];
        for (x, wx) in [(x0, 1.0 - fx), (x0 + 1, fx)] {
            for (y, wy) in [(y0, 1.0 - fy), (y0 + 1, fy)] {
                if wx == 0.0 || wy == 0.0 {
                    continue;
                }
                let p = self.point(x, y, cancel)?;
                let w = wx * wy;
                for c in 0..3 {
                    result[c] += p[c] * p[3] * w;
                }
                result[3] += p[3] * w;
            }
        }
        if result[3] > 0.0 {
            for c in 0..3 {
                result[c] /= result[3];
            }
        }
        Ok(result)
    }
    pub fn destination_color(
        &mut self,
        x: f64,
        y: f64,
        cancel: &dyn Fn() -> bool,
    ) -> Result<[f64; 4], CommandError> {
        self.color(x - self.offset[0], y - self.offset[1], cancel)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn search_sampling_has_a_separate_bounded_tile_decode_budget() {
        let mut e = crate::Editor::new();
        e.execute(e.request(Source::Test),"file.new",serde_json::json!({"width":512,"height":16,"bit_depth":16,"background":[20000,30000,40000,65535]}),|_|Ok(())).unwrap();
        let doc = e.document().unwrap();
        let layer = doc.active_layer().unwrap();
        let source = CloneSource {
            source: [1.5, 8.5],
            destination: None,
            sample: SelectionSample::CurrentLayer,
        };
        let mut sampler = CloneSampler::new(
            doc,
            layer,
            &source,
            &BrushPoint {
                x: 1.5,
                y: 8.5,
                pressure: 1.0,
            },
        )
        .unwrap();
        sampler.limit_tile_loads(1);
        assert!(sampler.destination_color(1.5, 8.5, &|| false).is_ok());
        assert!(
            sampler
                .destination_color(300.5, 8.5, &|| false)
                .unwrap_err()
                .to_string()
                .contains("decoded source tiles")
        );
        assert!(sampler.destination_color(1.5, 8.5, &|| false).is_ok());
    }
}
