//! Linear Display-P3 composition. Source samples stay untouched. The CPU path
//! uses f64 blending; output quantization happens only at the export boundary.
use crate::{BlendMode, Document, DocumentError, LayerKind};
use std::{
    collections::HashMap,
    hash::{Hash, Hasher},
    sync::Arc,
};
use strata_pixels::memory::{CacheSlot, MemoryKind, Reservation};
use strata_pixels::{CoverageRect, PixelBuffer, PixelFormat, TILE_EDGE, TileCoord};

pub type LinearTile = Arc<LinearPixels>; // straight linear P3, row-major
pub struct LinearPixels {
    pixels: Vec<[f32; 4]>,
    _reservation: Reservation,
}
impl LinearPixels {
    pub fn new(
        pixels: Vec<[f32; 4]>,
        pool: &strata_pixels::memory::MemoryPool,
    ) -> Result<Self, strata_pixels::memory::MemoryError> {
        let reservation = pool.reserve(MemoryKind::CpuCache, (pixels.capacity() * 16) as u64)?;
        Ok(Self {
            pixels,
            _reservation: reservation,
        })
    }
}
impl std::ops::Deref for LinearPixels {
    type Target = Vec<[f32; 4]>;
    fn deref(&self) -> &Self::Target {
        &self.pixels
    }
}

pub fn blend(back: [f64; 4], front: [f64; 4], mode: BlendMode, opacity: f64) -> [f64; 4] {
    let ab = back[3];
    let asrc = front[3] * opacity;
    let alpha = asrc + ab * (1.0 - asrc);
    if alpha == 0.0 {
        return [0.0; 4];
    }
    let mut out = [0.0; 4];
    out[3] = alpha;
    let rgb = crate::blending::color(
        [back[0], back[1], back[2]],
        [front[0], front[1], front[2]],
        mode,
    );
    for i in 0..3 {
        let b = back[i];
        let s = front[i];
        let mixed = rgb[i];
        out[i] = ((1.0 - asrc) * ab * b + (1.0 - ab) * asrc * s + ab * asrc * mixed) / alpha;
    }
    out
}

#[derive(Clone)]
pub struct Compositor {
    cache: HashMap<TileCoord, (u64, Arc<CacheSlot<LinearPixels>>, u64)>,
    clock: u64,
    capacity: usize,
    pub recomputed: u64,
}
impl Compositor {
    pub fn signature(doc: &Document, coord: TileCoord) -> u64 {
        fingerprint(doc, coord, doc.layers().len())
    }
    pub fn signature_prefix(doc: &Document, coord: TileCoord, layers: usize) -> u64 {
        fingerprint(doc, coord, layers)
    }
    pub fn new(capacity: usize) -> Self {
        Self {
            cache: HashMap::new(),
            clock: 0,
            capacity: capacity.max(1),
            recomputed: 0,
        }
    }
    pub fn clear(&mut self) {
        self.cache.clear();
    }
    pub fn tile(&mut self, doc: &Document, coord: TileCoord) -> Result<LinearTile, DocumentError> {
        self.tile_prefix(doc, coord, doc.layers().len())
    }
    /// Display-only prefix composition; the immutable document stays complete.
    pub fn tile_prefix(
        &mut self,
        doc: &Document,
        coord: TileCoord,
        layers: usize,
    ) -> Result<LinearTile, DocumentError> {
        self.tile_prefix_cancellable(doc, coord, layers, &|| false)
    }
    pub fn tile_prefix_cancellable(
        &mut self,
        doc: &Document,
        coord: TileCoord,
        layers: usize,
        cancel: &dyn Fn() -> bool,
    ) -> Result<LinearTile, DocumentError> {
        check_cancel(cancel)?;
        self.clock += 1;
        let key = fingerprint(doc, coord, layers);
        if let Some((old, tile, used)) = self.cache.get_mut(&coord)
            && *old == key
            && let Some(tile) = tile.get()
        {
            *used = self.clock;
            return Ok(tile);
        }
        let pool = doc.memory_pool();
        let reservation = pool.reserve(MemoryKind::CpuCache, 256 * 256 * 16)?;
        let tile = Arc::new(LinearPixels {
            pixels: compose_tile(doc, coord, layers, cancel)?,
            _reservation: reservation,
        });
        // Pin before global registration: a pressure callback may trim caches
        // concurrently, including a just-created slot.
        let slot = CacheSlot::from_arc(&pool, tile.clone());
        self.recomputed += 1;
        if self.cache.len() >= self.capacity
            && let Some(victim) = self.cache.iter().min_by_key(|(_, v)| v.2).map(|(k, _)| *k)
        {
            self.cache.remove(&victim);
        }
        self.cache.insert(coord, (key, slot, self.clock));
        Ok(tile)
    }
    pub fn cached_tiles(&self) -> usize {
        self.cache.len()
    }
}

fn fingerprint(doc: &Document, coord: TileCoord, layers: usize) -> u64 {
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    doc.id().hash(&mut hash);
    doc.working_icc().hash(&mut hash);
    let [ox, oy] = coord.origin();
    for layer in doc.layers().iter().take(layers) {
        if !layer.info.visible || layer.info.opacity == 0.0 {
            continue;
        }
        let Some(x) = ox.checked_sub(layer.info.x) else {
            continue;
        };
        let Some(y) = oy.checked_sub(layer.info.y) else {
            continue;
        };
        let (first, _) = TileCoord::locate(x, y);
        let (last, _) = TileCoord::locate(x.saturating_add(255), y.saturating_add(255));
        let mut touched =
            !matches!(layer.info.kind, LayerKind::Pixel) || !layer.info.styles.is_empty();
        for tx in first.x()..=last.x() {
            for ty in first.y()..=last.y() {
                if let Ok(c) = TileCoord::new(tx, ty)
                    && layer.pixels.contains_tile(c)
                {
                    touched = true;
                    layer.pixels.tile_version(c).hash(&mut hash);
                    c.hash(&mut hash);
                }
            }
        }
        if touched {
            layer.info.id.hash(&mut hash);
            layer.info.parent.hash(&mut hash);
            layer.info.clipped.hash(&mut hash);
            layer.info.generated_size.hash(&mut hash);
            layer.info.generated_crop.hash(&mut hash);
            layer.info.generated_geometry.hash(&mut hash);
            layer.info.spatial_bounds.hash(&mut hash);
            layer.info.pass_through.hash(&mut hash);
            if !layer.info.styles.is_empty() {
                serde_json::to_vec(&layer.info.styles)
                    .expect("validated style")
                    .hash(&mut hash);
                for candidate in doc.layers().iter().take(layers).filter(|candidate| {
                    candidate.id() == layer.id() || doc.is_descendant(candidate.id(), layer.id())
                }) {
                    for c in candidate.pixels.tile_coords() {
                        c.hash(&mut hash);
                        candidate.pixels.tile_version(*c).hash(&mut hash);
                    }
                    if let Some(mask) = &candidate.mask {
                        mask.layout().hash(&mut hash);
                        for c in mask.tile_coords() {
                            c.hash(&mut hash);
                            mask.tile_version(*c).hash(&mut hash);
                        }
                    }
                    serde_json::to_vec(candidate.info())
                        .expect("validated layer")
                        .hash(&mut hash);
                }
            }
            layer.info.x.hash(&mut hash);
            layer.info.y.hash(&mut hash);
            layer.info.opacity.to_bits().hash(&mut hash);
            layer.info.fill_opacity.to_bits().hash(&mut hash);
            std::mem::discriminant(&layer.info.blend_mode).hash(&mut hash);
            std::mem::discriminant(&layer.info.kind).hash(&mut hash);
            if let LayerKind::Exposure { stops } = layer.info.kind {
                stops.to_bits().hash(&mut hash);
            }
            match &layer.info.kind {
                LayerKind::Adjustment { adjustment } => {
                    serde_json::to_vec(adjustment)
                        .expect("validated tonal parameters")
                        .hash(&mut hash);
                    if matches!(adjustment, crate::TonalAdjustment::ShadowsHighlights { .. }) {
                        // Neighborhood inputs include earlier blocks beyond this
                        // tile. Hash their immutable versions without recursively
                        // hashing other spatial layers or later exposure edits.
                        serde_json::to_vec(doc.frame())
                            .expect("frame metadata")
                            .hash(&mut hash);
                        for candidate in doc.layers().iter().take_while(|l| l.id() != layer.id()) {
                            serde_json::to_vec(candidate.info())
                                .expect("layer metadata")
                                .hash(&mut hash);
                            for c in candidate.pixels.tile_coords() {
                                c.hash(&mut hash);
                                candidate.pixels.tile_version(*c).hash(&mut hash);
                            }
                            if let Some(mask) = &candidate.mask {
                                mask.layout().hash(&mut hash);
                                for c in mask.tile_coords() {
                                    c.hash(&mut hash);
                                    mask.tile_version(*c).hash(&mut hash);
                                }
                            }
                        }
                    }
                }
                LayerKind::Solid { color } => color.hash(&mut hash),
                LayerKind::Gradient {
                    start,
                    end,
                    angle,
                    scale,
                    radial,
                } => {
                    start.hash(&mut hash);
                    end.hash(&mut hash);
                    angle.to_bits().hash(&mut hash);
                    scale.to_bits().hash(&mut hash);
                    radial.hash(&mut hash);
                }
                LayerKind::HueSaturation {
                    hue,
                    saturation,
                    lightness,
                } => {
                    hue.to_bits().hash(&mut hash);
                    saturation.to_bits().hash(&mut hash);
                    lightness.to_bits().hash(&mut hash);
                }
                _ => {}
            }
            if let Some(mask) = &layer.mask {
                mask.layout().hash(&mut hash);
                if let Some(meta) = &layer.info.mask {
                    let mx = i128::from(ox) - i128::from(layer.info.x) + i128::from(meta.anchor_x);
                    let my = i128::from(oy) - i128::from(layer.info.y) + i128::from(meta.anchor_y);
                    for tx in mx.div_euclid(256)..=(mx + 255).div_euclid(256) {
                        for ty in my.div_euclid(256)..=(my + 255).div_euclid(256) {
                            if let (Ok(tx), Ok(ty)) = (i64::try_from(tx), i64::try_from(ty))
                                && let Ok(c) = TileCoord::new(tx, ty)
                            {
                                mask.tile_version(c).hash(&mut hash);
                                c.hash(&mut hash);
                            }
                        }
                    }
                }
                layer
                    .info
                    .mask
                    .as_ref()
                    .map(|m| (m.enabled, m.anchor_x, m.anchor_y))
                    .hash(&mut hash);
            }
        }
    }
    hash.finish()
}

fn compose_tile(
    doc: &Document,
    coord: TileCoord,
    layers: usize,
    cancel: &dyn Fn() -> bool,
) -> Result<Vec<[f32; 4]>, DocumentError> {
    check_cancel(cancel)?;
    let _scratch = doc
        .memory_pool()
        .reserve(MemoryKind::Transient, 256 * 256 * (32 + 16 + 2))?;
    let [ox, oy] = coord.origin();
    // The common imported-photo case has no layer mixing. Avoid a full f64
    // accumulator and per-pixel coordinate/blend work for aligned source tiles.
    let mut visible = doc
        .layers()
        .iter()
        .take(layers)
        .filter(|layer| layer.info.visible && layer.info.opacity > 0.0);
    if let Some(layer) = visible.next()
        && visible.next().is_none()
        && layer.info.parent.is_none()
        && layer.info.opacity == 1.0
        && layer.info.fill_opacity == 1.0
        && layer.info.blend_mode == BlendMode::Normal
        && matches!(layer.info.kind, LayerKind::Pixel)
        && layer.mask.is_none()
        && layer.info.styles.is_empty()
        && let Some(sx) = ox.checked_sub(layer.info.x)
        && let Some(sy) = oy.checked_sub(layer.info.y)
        && sx.rem_euclid(256) == 0
        && sy.rem_euclid(256) == 0
    {
        let (source_coord, _) = TileCoord::locate(sx, sy);
        if let Some(source) = layer.pixels.tile(source_coord)? {
            let mut linear =
                strata_color::to_linear_p3(&source, TILE_EDGE, TILE_EDGE, doc.working_icc())?;
            for pixel in &mut linear {
                if pixel[3] == 0.0 {
                    *pixel = [0.0; 4];
                }
            }
            return Ok(linear);
        }
    }
    let out = compose_stack(doc, coord, layers, None, cancel)?;
    // Avoid retaining the larger f64 allocation through an in-place collect.
    let mut result = Vec::with_capacity(out.len());
    for pixel in out {
        result.push(pixel.map(|v| v as f32));
    }
    Ok(result)
}

pub(crate) fn compose_stack(
    doc: &Document,
    coord: TileCoord,
    layers: usize,
    parent: Option<crate::LayerId>,
    cancel: &dyn Fn() -> bool,
) -> Result<Vec<[f64; 4]>, DocumentError> {
    check_cancel(cancel)?;
    compose_with_backdrop(
        doc,
        coord,
        layers,
        parent,
        vec![[0.0; 4]; 256 * 256],
        cancel,
    )
}
fn check_cancel(cancel: &dyn Fn() -> bool) -> Result<(), DocumentError> {
    if cancel() {
        Err(DocumentError::Cancelled)
    } else {
        Ok(())
    }
}
/// Reconstruct the surface this layer actually sees, before its own mask,
/// opacity and parent-group interpolation. Clipped chains start independently.
fn input_before_layer(
    doc: &Document,
    coord: TileCoord,
    target: crate::LayerId,
    cancel: &dyn Fn() -> bool,
) -> Result<Vec<[f64; 4]>, DocumentError> {
    check_cancel(cancel)?;
    let index = doc
        .layers()
        .iter()
        .position(|l| l.id() == target)
        .ok_or(DocumentError::LayerNotFound)?;
    let layer = &doc.layers()[index];
    let parent = layer.info.parent;
    if layer.info.clipped {
        let siblings = doc
            .layers()
            .iter()
            .take(index)
            .filter(|l| l.info.parent == parent)
            .collect::<Vec<_>>();
        let base = siblings
            .iter()
            .rposition(|l| !l.info.clipped)
            .ok_or_else(|| DocumentError::Invalid("missing clipping base".into()))?;
        return compose_sequence(
            doc,
            coord,
            index,
            &siblings[base..],
            vec![[0.0; 4]; 256 * 256],
            true,
            cancel,
        );
    }
    let backdrop = if let Some(parent) = parent {
        let group = doc
            .layers()
            .iter()
            .find(|l| l.id() == parent)
            .ok_or(DocumentError::LayerNotFound)?;
        if group.info.pass_through
            && group.info.blend_mode == BlendMode::Normal
            && !group.info.clipped
            && group.info.styles.is_empty()
        {
            // A pass-through group used as a clipping-chain base is still
            // evaluated on that chain's independent transparent surface.
            let group_index = doc
                .layers()
                .iter()
                .position(|l| l.id() == parent)
                .expect("resolved group");
            let chain_base = doc
                .layers()
                .iter()
                .skip(group_index + 1)
                .find(|l| l.info.parent == group.info.parent)
                .is_some_and(|next| next.info.clipped);
            if chain_base {
                vec![[0.0; 4]; 256 * 256]
            } else {
                input_before_layer(doc, coord, parent, cancel)?
            }
        } else {
            vec![[0.0; 4]; 256 * 256]
        }
    } else {
        vec![[0.0; 4]; 256 * 256]
    };
    compose_with_backdrop(doc, coord, index, parent, backdrop, cancel)
}
fn compose_with_backdrop(
    doc: &Document,
    coord: TileCoord,
    layers: usize,
    parent: Option<crate::LayerId>,
    mut out: Vec<[f64; 4]>,
    cancel: &dyn Fn() -> bool,
) -> Result<Vec<[f64; 4]>, DocumentError> {
    check_cancel(cancel)?;
    let siblings: Vec<_> = doc
        .layers()
        .iter()
        .take(layers)
        .filter(|layer| layer.info.parent == parent)
        .collect();
    if !siblings.iter().any(|layer| layer.info.clipped) {
        return compose_sequence(doc, coord, layers, &siblings, out, false, cancel);
    }
    let mut begin = 0;
    while begin < siblings.len() {
        let mut end = begin + 1;
        while end < siblings.len() && siblings[end].info.clipped {
            end += 1;
        }
        if end == begin + 1 {
            out = compose_sequence(
                doc,
                coord,
                layers,
                &siblings[begin..end],
                out,
                false,
                cancel,
            )?;
        } else {
            let _chain_scratch = doc
                .memory_pool()
                .reserve(MemoryKind::Transient, 256 * 256 * 32)?;
            let surface = compose_sequence(
                doc,
                coord,
                layers,
                &siblings[begin..end],
                vec![[0.0; 4]; 256 * 256],
                true,
                cancel,
            )?;
            let mode = siblings[begin].info.blend_mode;
            for (back, front) in out.iter_mut().zip(surface) {
                *back = blend(*back, front, mode, 1.0);
            }
        }
        begin = end;
    }
    Ok(out)
}
fn compose_sequence(
    doc: &Document,
    coord: TileCoord,
    layers: usize,
    siblings: &[&crate::Layer],
    mut out: Vec<[f64; 4]>,
    clipping: bool,
    cancel: &dyn Fn() -> bool,
) -> Result<Vec<[f64; 4]>, DocumentError> {
    let [ox, oy] = coord.origin();
    for (ordinal, layer) in siblings.iter().copied().enumerate() {
        check_cancel(cancel)?;
        if !layer.info.visible || layer.info.opacity == 0.0 {
            continue;
        }
        if !layer.info.styles.is_empty() {
            let styled = crate::styles::render(doc, layer, coord, layers, cancel)?;
            for (index, front) in styled.into_iter().enumerate() {
                out[index] = blend_inside(
                    out[index],
                    front,
                    layer.info.blend_mode,
                    layer.info.opacity,
                    clipping && ordinal > 0,
                );
            }
            continue;
        }
        if matches!(layer.info.kind, LayerKind::Group) {
            let _group_scratch = doc
                .memory_pool()
                .reserve(MemoryKind::Transient, 256 * 256 * 34)?;
            let mask = layer_mask_tile(layer, ox, oy)?;
            if layer.info.pass_through
                && layer.info.blend_mode == BlendMode::Normal
                && !(clipping && ordinal > 0)
            {
                let group = compose_with_backdrop(
                    doc,
                    coord,
                    layers,
                    Some(layer.id()),
                    out.clone(),
                    cancel,
                )?;
                for (index, front) in group.into_iter().enumerate() {
                    let amount = f64::from(mask.as_ref().map_or(65535, |m| m[index])) / 65535.0
                        * layer.info.opacity
                        * layer.info.fill_opacity;
                    let back = out[index];
                    let alpha = back[3] + (front[3] - back[3]) * amount;
                    if alpha > 0.0 {
                        for c in 0..3 {
                            out[index][c] = (back[c] * back[3] * (1.0 - amount)
                                + front[c] * front[3] * amount)
                                / alpha;
                        }
                        out[index][3] = alpha;
                    } else {
                        out[index] = [0.0; 4];
                    }
                }
                continue;
            }
            let group = compose_stack(doc, coord, layers, Some(layer.id()), cancel)?;
            for (index, mut front) in group.into_iter().enumerate() {
                front[3] *= f64::from(mask.as_ref().map_or(65535, |m| m[index])) / 65535.0;
                let opacity = layer.info.opacity * layer.info.fill_opacity;
                if layer.info.blend_mode == BlendMode::Dissolve {
                    front[3] = if dissolve_noise(
                        layer.id(),
                        ox + (index % 256) as i64,
                        oy + (index / 256) as i64,
                    ) < front[3] * opacity
                    {
                        1.0
                    } else {
                        0.0
                    };
                    out[index] = blend_inside(
                        out[index],
                        front,
                        BlendMode::Normal,
                        1.0,
                        clipping && ordinal > 0,
                    );
                } else {
                    out[index] = blend_inside(
                        out[index],
                        front,
                        layer.info.blend_mode,
                        opacity,
                        clipping && ordinal > 0,
                    );
                }
            }
            continue;
        }
        let mask = layer_mask_tile(layer, ox, oy)?;
        if let LayerKind::Adjustment { adjustment } = &layer.info.kind {
            if adjustment.is_identity() && layer.info.blend_mode == BlendMode::Normal {
                continue;
            }
            if mask
                .as_ref()
                .is_some_and(|values| values.iter().all(|v| *v == 0))
            {
                continue;
            }
            let _scratch = doc
                .memory_pool()
                .reserve(MemoryKind::Transient, 256 * 256 * 64)?;
            let result = if matches!(adjustment, crate::TonalAdjustment::ShadowsHighlights { .. }) {
                crate::adjustments::spatial::render(
                    doc,
                    coord,
                    &out,
                    adjustment,
                    layer.info.spatial_bounds,
                    |c| input_before_layer(doc, c, layer.id(), cancel),
                    cancel,
                )?
            } else {
                let source = out.iter().map(|v| v.map(|c| c as f32)).collect::<Vec<_>>();
                adjustment.transform(&source, doc.working_icc())?
            };
            for (index, pixel) in out.iter_mut().enumerate() {
                let back = [pixel[0], pixel[1], pixel[2]];
                let front = [
                    f64::from(result[index][0]),
                    f64::from(result[index][1]),
                    f64::from(result[index][2]),
                ];
                let mixed = crate::blending::color(back, front, layer.info.blend_mode);
                let mut amount = f64::from(mask.as_ref().map_or(65535, |m| m[index])) / 65535.0
                    * layer.info.opacity
                    * layer.info.fill_opacity;
                if layer.info.blend_mode == BlendMode::Dissolve {
                    amount = if dissolve_noise(
                        layer.id(),
                        ox + (index % 256) as i64,
                        oy + (index / 256) as i64,
                    ) < amount
                    {
                        1.0
                    } else {
                        0.0
                    };
                }
                for c in 0..3 {
                    pixel[c] += (mixed[c] - back[c]) * amount;
                }
            }
            continue;
        }
        if let LayerKind::HueSaturation {
            hue,
            saturation,
            lightness,
        } = layer.info.kind
        {
            for (index, pixel) in out.iter_mut().enumerate() {
                let back = [pixel[0], pixel[1], pixel[2]];
                let adjusted = adjust_hsl(back, hue, saturation, lightness);
                let mixed = crate::blending::color(back, adjusted, layer.info.blend_mode);
                let amount = f64::from(mask.as_ref().map_or(65535, |m| m[index])) / 65535.0
                    * layer.info.opacity
                    * layer.info.fill_opacity;
                for c in 0..3 {
                    pixel[c] += (mixed[c] - back[c]) * amount;
                }
            }
            continue;
        }
        if matches!(
            layer.info.kind,
            LayerKind::Solid { .. } | LayerKind::Gradient { .. }
        ) {
            let _fill_scratch = doc
                .memory_pool()
                .reserve(MemoryKind::Transient, 256 * 256 * 32)?;
            let values = generated_fill(doc, layer, coord)?;
            for (index, mut front) in values.into_iter().enumerate() {
                front[3] *= f64::from(mask.as_ref().map_or(65535, |m| m[index])) / 65535.0;
                let opacity = layer.info.opacity * layer.info.fill_opacity;
                if layer.info.blend_mode == BlendMode::Dissolve {
                    front[3] = if dissolve_noise(
                        layer.id(),
                        ox + (index % 256) as i64,
                        oy + (index / 256) as i64,
                    ) < front[3] * opacity
                    {
                        1.0
                    } else {
                        0.0
                    };
                    out[index] = blend_inside(
                        out[index],
                        front,
                        BlendMode::Normal,
                        1.0,
                        clipping && ordinal > 0,
                    );
                } else {
                    out[index] = blend_inside(
                        out[index],
                        front,
                        layer.info.blend_mode,
                        opacity,
                        clipping && ordinal > 0,
                    );
                }
            }
            continue;
        }
        if let LayerKind::Exposure { stops } = layer.info.kind {
            let gain = 2f64.powf(stops);
            for y in 0..256usize {
                for x in 0..256usize {
                    let index = y * 256 + x;
                    let mut amount = f64::from(mask.as_ref().map_or(65535, |m| m[index])) / 65535.0
                        * layer.info.opacity
                        * layer.info.fill_opacity;
                    if layer.info.blend_mode == BlendMode::Dissolve {
                        amount =
                            if dissolve_noise(layer.id(), ox + x as i64, oy + y as i64) < amount {
                                1.0
                            } else {
                                0.0
                            };
                    }
                    let back = [out[index][0], out[index][1], out[index][2]];
                    let adjusted = back.map(|v| v * gain);
                    let mixed = crate::blending::color(back, adjusted, layer.info.blend_mode);
                    for c in 0..3 {
                        out[index][c] += (mixed[c] - back[c]) * amount;
                    }
                }
            }
            continue;
        }
        let Some(sx) = ox.checked_sub(layer.info.x) else {
            continue;
        };
        let Some(sy) = oy.checked_sub(layer.info.y) else {
            continue;
        };
        let (first, _) = TileCoord::locate(sx, sy);
        let (last, _) = TileCoord::locate(sx.saturating_add(255), sy.saturating_add(255));
        for tx in first.x()..=last.x() {
            for ty in first.y()..=last.y() {
                let source_coord = TileCoord::new(tx, ty)?;
                let Some(source) = layer.pixels.tile(source_coord)? else {
                    continue;
                };
                let linear =
                    strata_color::to_linear_p3(&source, TILE_EDGE, TILE_EDGE, doc.working_icc())?;
                let [px, py] = source_coord.origin();
                for y in 0..256usize {
                    for x in 0..256usize {
                        let Some(lx) = sx.checked_add(x as i64).and_then(|v| v.checked_sub(px))
                        else {
                            continue;
                        };
                        let Some(ly) = sy.checked_add(y as i64).and_then(|v| v.checked_sub(py))
                        else {
                            continue;
                        };
                        if !(0..256).contains(&lx) || !(0..256).contains(&ly) {
                            continue;
                        }
                        let index = y * 256 + x;
                        let mut front = linear[ly as usize * 256 + lx as usize].map(f64::from);
                        front[3] *= f64::from(mask.as_ref().map_or(65535, |m| m[index])) / 65535.0;
                        let opacity = layer.info.opacity * layer.info.fill_opacity;
                        if layer.info.blend_mode == BlendMode::Dissolve {
                            front[3] = if dissolve_noise(layer.id(), ox + x as i64, oy + y as i64)
                                < front[3] * opacity
                            {
                                1.0
                            } else {
                                0.0
                            };
                            out[index] = blend_inside(
                                out[index],
                                front,
                                BlendMode::Normal,
                                1.0,
                                clipping && ordinal > 0,
                            );
                        } else {
                            out[index] = blend_inside(
                                out[index],
                                front,
                                layer.info.blend_mode,
                                opacity,
                                clipping && ordinal > 0,
                            );
                        }
                    }
                }
            }
        }
    }
    Ok(out)
}

pub fn layer_mask_tile(
    layer: &crate::Layer,
    x: i64,
    y: i64,
) -> Result<Option<Vec<u16>>, DocumentError> {
    let Some(mask) = &layer.mask else {
        return Ok(None);
    };
    let Some(meta) = &layer.info.mask else {
        return Ok(None);
    };
    if !meta.enabled {
        return Ok(None);
    }
    let mx = i128::from(x) - i128::from(layer.info.x) + i128::from(meta.anchor_x);
    let my = i128::from(y) - i128::from(layer.info.y) + i128::from(meta.anchor_y);
    let bounds = mask.layout().bounds;
    if mx + 256 <= i128::from(bounds.x)
        || my + 256 <= i128::from(bounds.y)
        || mx >= i128::from(bounds.x) + i128::from(bounds.width)
        || my >= i128::from(bounds.y) + i128::from(bounds.height)
    {
        return Ok(Some(vec![0; 256 * 256]));
    }
    let (Ok(mx), Ok(my)) = (i64::try_from(mx), i64::try_from(my)) else {
        return Ok(Some(vec![0; 256 * 256]));
    };
    Ok(Some(mask.region_tile(mx, my)?))
}
pub fn layer_mask_signature(layer: &crate::Layer, coord: TileCoord) -> u64 {
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    layer.info.x.hash(&mut hash);
    layer.info.y.hash(&mut hash);
    if let (Some(mask), Some(meta)) = (&layer.mask, &layer.info.mask) {
        meta.enabled.hash(&mut hash);
        meta.anchor_x.hash(&mut hash);
        meta.anchor_y.hash(&mut hash);
        mask.layout().hash(&mut hash);
        let [ox, oy] = coord.origin();
        let mx = i128::from(ox) - i128::from(layer.info.x) + i128::from(meta.anchor_x);
        let my = i128::from(oy) - i128::from(layer.info.y) + i128::from(meta.anchor_y);
        for tx in mx.div_euclid(256)..=(mx + 255).div_euclid(256) {
            for ty in my.div_euclid(256)..=(my + 255).div_euclid(256) {
                if let (Ok(tx), Ok(ty)) = (i64::try_from(tx), i64::try_from(ty))
                    && let Ok(c) = TileCoord::new(tx, ty)
                {
                    c.hash(&mut hash);
                    mask.tile_version(c).hash(&mut hash);
                }
            }
        }
    }
    hash.finish()
}

impl Document {
    pub fn histogram(&self) -> Result<crate::commands::HistogramResult, DocumentError> {
        let width = self.frame.width.min(256);
        let height = self.frame.height.min(256);
        let _scratch = self.pool.reserve(
            MemoryKind::Transient,
            u64::from(width) * u64::from(height) * 32,
        )?;
        let mut samples = Vec::with_capacity((width * height) as usize);
        for y in 0..height {
            for x in 0..width {
                let sx = self.frame.x
                    + (((u64::from(x) * 2 + 1) * u64::from(self.frame.width))
                        / (u64::from(width) * 2)) as i64;
                let sy = self.frame.y
                    + (((u64::from(y) * 2 + 1) * u64::from(self.frame.height))
                        / (u64::from(height) * 2)) as i64;
                let (coord, [px, py]) = TileCoord::locate(sx, sy);
                samples.push((coord, usize::from(py) * 256 + usize::from(px)));
            }
        }
        samples.sort_unstable_by_key(|s| s.0);
        let mut compositor = Compositor::new(1);
        let mut last = None;
        let mut luminance = vec![0; 256];
        let mut nontransparent_pixels = 0;
        for (coord, index) in samples {
            if last.as_ref().is_none_or(|(c, _)| *c != coord) {
                last = Some((coord, compositor.tile(self, coord)?));
            }
            let p = last.as_ref().expect("sample tile").1[index];
            if p[3] > 0.0 {
                let l = (p[0] * 0.2289746 + p[1] * 0.6917385 + p[2] * 0.0792869).clamp(0.0, 1.0);
                luminance[(l * 255.0).round() as usize] += 1;
                nontransparent_pixels += 1;
            }
        }
        Ok(crate::commands::HistogramResult {
            luminance,
            sampled_pixels: (width * height) as usize,
            nontransparent_pixels,
            method: "bounded pixel-center samples; linear Display P3 luminance".into(),
        })
    }
    /// Export from source/CPU composition, never from an 8-bit display texture.
    pub fn export_pixels(&self) -> Result<PixelBuffer, DocumentError> {
        self.export_pixels_cancellable(&|| false)
    }
    pub fn export_pixels_cancellable(
        &self,
        cancel: &dyn Fn() -> bool,
    ) -> Result<PixelBuffer, DocumentError> {
        self.export_region_pixels(
            CoverageRect {
                x: self.frame.x,
                y: self.frame.y,
                width: self.frame.width,
                height: self.frame.height,
            },
            cancel,
        )
    }
    /// Render a requested window without changing the canonical artboard used
    /// for spatial filters, linked masks or legacy generated-layer geometry.
    pub(crate) fn export_region_pixels(
        &self,
        region: CoverageRect,
        cancel: &dyn Fn() -> bool,
    ) -> Result<PixelBuffer, DocumentError> {
        check_cancel(cancel)?;
        self.validate()?;
        region.validate()?;
        if self.format.byte_count(region.width, region.height)? as u64 > crate::MAX_FLAT_IMAGE_BYTES
        {
            return Err(DocumentError::ResourceLimit);
        }
        let count = self.format.sample_count(region.width, region.height)?;
        // Exact identity path, including hidden RGB at alpha=0.
        let visible: Vec<_> = self
            .layers
            .iter()
            .filter(|l| l.info.visible && l.info.opacity > 0.0
                && !matches!(&l.info.kind,LayerKind::Adjustment{adjustment}if adjustment.is_identity() && l.info.blend_mode==BlendMode::Normal))
            .collect();
        if visible.len() == 1
            && visible[0].info.parent.is_none()
            && visible[0].info.opacity == 1.0
            && visible[0].info.fill_opacity == 1.0
            && visible[0].info.blend_mode == BlendMode::Normal
            && matches!(visible[0].info.kind, LayerKind::Pixel)
            && visible[0].mask.is_none()
            && visible[0].info.styles.is_empty()
        {
            let l = visible[0];
            let mut out = match self.format {
                PixelFormat::Rgba8 => PixelBuffer::Rgba8(vec![0; count]),
                PixelFormat::Rgba16 => PixelBuffer::Rgba16(vec![0; count]),
            };
            for (coord, tile) in l.pixels.tiles() {
                check_cancel(cancel)?;
                let tile = tile?;
                let [ox, oy] = coord.origin();
                for y in 0..256i64 {
                    let Some(dy) = oy
                        .checked_add(y)
                        .and_then(|v| v.checked_add(l.info.y))
                        .and_then(|v| v.checked_sub(region.y))
                    else {
                        continue;
                    };
                    if !(0..i64::from(region.height)).contains(&dy) {
                        continue;
                    }
                    for x in 0..256i64 {
                        let Some(dx) = ox
                            .checked_add(x)
                            .and_then(|v| v.checked_add(l.info.x))
                            .and_then(|v| v.checked_sub(region.x))
                        else {
                            continue;
                        };
                        if !(0..i64::from(region.width)).contains(&dx) {
                            continue;
                        }
                        let i = (dy as usize * region.width as usize + dx as usize) * 4;
                        let src = (y as usize * 256 + x as usize) * 4;
                        match (&mut out, &*tile) {
                            (PixelBuffer::Rgba8(v), PixelBuffer::Rgba8(p)) => {
                                v[i..i + 4].copy_from_slice(&p[src..src + 4])
                            }
                            (PixelBuffer::Rgba16(v), PixelBuffer::Rgba16(p)) => {
                                v[i..i + 4].copy_from_slice(&p[src..src + 4])
                            }
                            _ => unreachable!(),
                        }
                    }
                }
            }
            return Ok(out);
        }
        let mut out = match self.format {
            PixelFormat::Rgba8 => PixelBuffer::Rgba8(vec![0; count]),
            PixelFormat::Rgba16 => PixelBuffer::Rgba16(vec![0; count]),
        };
        let mut compositor = Compositor::new(1);
        let (first, _) = TileCoord::locate(region.x, region.y);
        let (last, _) = TileCoord::locate(
            region.x + i64::from(region.width) - 1,
            region.y + i64::from(region.height) - 1,
        );
        for tx in first.x()..=last.x() {
            for ty in first.y()..=last.y() {
                let coord = TileCoord::new(tx, ty)?;
                let linear =
                    compositor.tile_prefix_cancellable(self, coord, self.layers().len(), cancel)?;
                let converted =
                    strata_color::from_linear_p3(&linear, self.format, self.working_icc())?;
                let [ox, oy] = coord.origin();
                for y in 0..256i64 {
                    for x in 0..256i64 {
                        let dx = ox + x - region.x;
                        let dy = oy + y - region.y;
                        if dx < 0
                            || dy < 0
                            || dx >= i64::from(region.width)
                            || dy >= i64::from(region.height)
                        {
                            continue;
                        }
                        let dest = (dy as usize * region.width as usize + dx as usize) * 4;
                        let src = (y as usize * 256 + x as usize) * 4;
                        match (&mut out, &converted) {
                            (PixelBuffer::Rgba8(v), PixelBuffer::Rgba8(p)) => {
                                v[dest..dest + 4].copy_from_slice(&p[src..src + 4])
                            }
                            (PixelBuffer::Rgba16(v), PixelBuffer::Rgba16(p)) => {
                                v[dest..dest + 4].copy_from_slice(&p[src..src + 4])
                            }
                            _ => unreachable!(),
                        }
                    }
                }
            }
        }
        Ok(out)
    }
}
fn blend_inside(
    back: [f64; 4],
    front: [f64; 4],
    mode: BlendMode,
    opacity: f64,
    clipped: bool,
) -> [f64; 4] {
    if !clipped {
        return blend(back, front, mode, opacity);
    }
    if back[3] == 0.0 {
        return [0.0; 4];
    }
    let mixed = crate::blending::color(
        [back[0], back[1], back[2]],
        [front[0], front[1], front[2]],
        mode,
    );
    let amount = front[3] * opacity;
    [
        back[0] + (mixed[0] - back[0]) * amount,
        back[1] + (mixed[1] - back[1]) * amount,
        back[2] + (mixed[2] - back[2]) * amount,
        back[3],
    ]
}

pub(crate) fn dissolve_noise(layer: crate::LayerId, x: i64, y: i64) -> f64 {
    let seed = layer
        .stable_bytes()
        .iter()
        .fold(0xcbf29ce484222325u64, |state, byte| {
            (state ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
        });
    let mut n = seed ^ (x as u64).wrapping_mul(0x9e3779b97f4a7c15) ^ (y as u64).rotate_left(29);
    n = (n ^ (n >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    n = (n ^ (n >> 27)).wrapping_mul(0x94d049bb133111eb);
    n ^= n >> 31;
    ((n >> 11) as f64) * (1.0 / (1u64 << 53) as f64)
}

pub(crate) fn generated_fill(
    doc: &Document,
    layer: &crate::Layer,
    coord: TileCoord,
) -> Result<Vec<[f64; 4]>, DocumentError> {
    let (start, end, angle, scale, radial) = match layer.info.kind {
        LayerKind::Solid { color } => (color, color, 0.0, 1.0, false),
        LayerKind::Gradient {
            start,
            end,
            angle,
            scale,
            radial,
        } => (start, end, angle, scale, radial),
        _ => unreachable!("fill layer only"),
    };
    let source = if doc.bit_depth() == 8 {
        PixelBuffer::Rgba8(start.into_iter().chain(end).map(|v| v as u8).collect())
    } else {
        PixelBuffer::Rgba16(start.into_iter().chain(end).collect())
    };
    let endpoints = strata_color::to_linear_p3(&source, 2, 1, doc.working_icc())?;
    let start = endpoints[0].map(f64::from);
    let end = endpoints[1].map(f64::from);
    let [ox, oy] = coord.origin();
    let [width, height] = layer
        .info
        .generated_size
        .unwrap_or([doc.frame.width, doc.frame.height]);
    let w = f64::from(width);
    let h = f64::from(height);
    let angle = angle.to_radians();
    let axis = [angle.cos(), angle.sin()];
    let span = (w * axis[0].abs() + h * axis[1].abs()) * scale;
    let mut out = vec![[0.0; 4]; 256 * 256];
    let geometry = layer.info.generated_geometry.as_ref();
    let mapping = geometry
        .map(crate::GeneratedGeometry::mapping)
        .transpose()?;
    if let Some(geometry) = geometry
        && geometry
            .bounds([layer.info.x, layer.info.y])?
            .is_none_or(|b| !b.intersects_tile(coord))
    {
        return Ok(out);
    }
    for y in 0..256usize {
        for x in 0..256usize {
            let sx = i128::from(ox) + x as i128 - i128::from(layer.info.x);
            let sy = i128::from(oy) + y as i128 - i128::from(layer.info.y);
            let center = [sx as f64 + 0.5, sy as f64 + 0.5];
            let [u, v] = if let (Some(geometry), Some(mapping)) = (geometry, mapping) {
                if !geometry.contains(center) {
                    continue;
                }
                let Ok(point) = mapping.unmap(center) else {
                    continue;
                };
                point
            } else {
                if sx < 0 || sy < 0 || sx >= i128::from(width) || sy >= i128::from(height) {
                    continue;
                }
                if let Some(crop) = &layer.info.generated_crop
                    && crop
                        .bounds
                        .is_none_or(|b| !b.contains(sx as i64, sy as i64))
                {
                    continue;
                }
                center
            };
            if u < 0.0 || v < 0.0 || u >= w || v >= h {
                continue;
            }
            let px = u - w / 2.0;
            let py = v - h / 2.0;
            let t = if matches!(layer.info.kind, LayerKind::Solid { .. }) {
                0.0
            } else if radial {
                (px * px + py * py).sqrt() / (w.min(h) * 0.5 * scale)
            } else {
                0.5 + (px * axis[0] + py * axis[1]) / span
            };
            let t = t.clamp(0.0, 1.0);
            let alpha = start[3] * (1.0 - t) + end[3] * t;
            let mut value = [0.0; 4];
            value[3] = alpha;
            if alpha > 0.0 {
                for c in 0..3 {
                    value[c] = (start[c] * start[3] * (1.0 - t) + end[c] * end[3] * t) / alpha;
                }
            }
            out[y * 256 + x] = value;
        }
    }
    Ok(out)
}
fn adjust_hsl(rgb: [f64; 3], hue: f64, saturation: f64, lightness: f64) -> [f64; 3] {
    if hue == 0.0 && saturation == 0.0 && lightness == 0.0 {
        return rgb;
    }
    let encoded = rgb
        .map(|v| {
            if v <= 0.0031308 {
                v * 12.92
            } else {
                1.055 * v.powf(1.0 / 2.4) - 0.055
            }
        })
        .map(|v| v.clamp(0.0, 1.0));
    let max = encoded.into_iter().fold(f64::NEG_INFINITY, f64::max);
    let min = encoded.into_iter().fold(f64::INFINITY, f64::min);
    let delta = max - min;
    let mut l = (max + min) / 2.0;
    let mut s = if delta == 0.0 {
        0.0
    } else {
        delta / (1.0 - (2.0 * l - 1.0).abs()).max(f64::EPSILON)
    };
    let mut h = if delta == 0.0 {
        0.0
    } else if max == encoded[0] {
        ((encoded[1] - encoded[2]) / delta).rem_euclid(6.0)
    } else if max == encoded[1] {
        (encoded[2] - encoded[0]) / delta + 2.0
    } else {
        (encoded[0] - encoded[1]) / delta + 4.0
    };
    h = (h + hue / 60.0).rem_euclid(6.0);
    let sat = saturation / 100.0;
    let light = lightness / 100.0;
    s = if sat >= 0.0 {
        s + (1.0 - s) * sat
    } else {
        s * (1.0 + sat)
    };
    l = if light >= 0.0 {
        l + (1.0 - l) * light
    } else {
        l * (1.0 + light)
    };
    let chroma = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = chroma * (1.0 - (h.rem_euclid(2.0) - 1.0).abs());
    let m = l - chroma / 2.0;
    let channels = match h.floor() as u8 {
        0 => [chroma, x, 0.0],
        1 => [x, chroma, 0.0],
        2 => [0.0, chroma, x],
        3 => [0.0, x, chroma],
        4 => [x, 0.0, chroma],
        _ => [chroma, 0.0, x],
    };
    channels.map(|c| c + m).map(|v| {
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    })
}
