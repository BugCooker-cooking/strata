use crate::{DocumentId, Layer, LayerId, LayerRef, StateId};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use strata_pixels::{PixelBuffer, PixelFormat, TiledMask, TiledPixels};

pub const MAX_DOCUMENT_BYTES: u64 = 16 * 1024 * 1024 * 1024;
pub const MAX_FLAT_IMAGE_BYTES: u64 = 512 * 1024 * 1024;
pub const MAX_LAYERS: usize = 128;
pub(crate) fn default_resolution_dpi() -> u32 {
    72
}

#[derive(Debug, thiserror::Error)]
pub enum DocumentError {
    #[error("composition cancelled")]
    Cancelled,
    #[error("invalid document: {0}")]
    Invalid(String),
    #[error("document resource limit exceeded")]
    ResourceLimit,
    #[error("layer not found")]
    LayerNotFound,
    #[error("ambiguous layer name; use one of these IDs: {0:?}")]
    Ambiguous(Vec<LayerId>),
    #[error(transparent)]
    Pixels(#[from] strata_pixels::PixelError),
    #[error(transparent)]
    Tiles(#[from] strata_pixels::TileError),
    #[error(transparent)]
    Color(#[from] strata_color::ColorError),
    #[error(transparent)]
    Memory(#[from] strata_pixels::memory::MemoryError),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Frame {
    pub name: String,
    pub x: i64,
    pub y: i64,
    pub width: u32,
    pub height: u32,
}

/// Immutable public snapshot. Only the core's command implementation mutates it.
/// ```compile_fail
/// fn bypass(doc: &mut strata_core::Document) { doc.layers.clear(); }
/// ```
#[derive(Clone, Debug)]
pub struct Document {
    pub(crate) selection: Option<TiledMask>,
    pub(crate) previous_selection: Option<TiledMask>,
    pub(crate) resolution_dpi: u32,
    pub(crate) preset: Option<String>,
    pub(crate) pool: strata_pixels::memory::MemoryPool,
    pub(crate) id: DocumentId,
    pub(crate) state: StateId,
    pub(crate) frame: Frame,
    pub(crate) layers: Vec<Layer>, // bottom to top
    pub(crate) active: Option<LayerId>,
    pub(crate) format: PixelFormat,
    pub(crate) working_icc: Vec<u8>,
    pub(crate) source_icc: Option<Vec<u8>>,
    pub(crate) profile_name: String,
    pub(crate) profile_fallback: bool,
}
impl Document {
    pub fn resolution_dpi(&self) -> u32 {
        self.resolution_dpi
    }
    pub fn preset(&self) -> Option<&str> {
        self.preset.as_deref()
    }
    pub fn memory_pool(&self) -> strata_pixels::memory::MemoryPool {
        self.pool.clone()
    }
    pub fn id(&self) -> DocumentId {
        self.id
    }
    pub fn state_id(&self) -> StateId {
        self.state
    }
    pub fn frame(&self) -> &Frame {
        &self.frame
    }
    pub fn layers(&self) -> &[Layer] {
        &self.layers
    }
    pub fn selection(&self) -> Option<&TiledMask> {
        self.selection.as_ref()
    }
    pub fn previous_selection(&self) -> Option<&TiledMask> {
        self.previous_selection.as_ref()
    }
    pub fn source_bytes(&self) -> u64 {
        self.source_blocks().map(|(_, bytes)| bytes).sum()
    }
    pub(crate) fn source_blocks(&self) -> impl Iterator<Item = (u64, u64)> + '_ {
        let pixels = self.layers.iter().flat_map(|layer| {
            layer.pixels.tile_coords().filter_map(move |coord| {
                layer.pixels.tile_version(*coord).map(|version| {
                    (
                        version,
                        256 * 256 * 4 * layer.pixels.format().bytes_per_sample() as u64,
                    )
                })
            })
        });
        let masks = self
            .layers
            .iter()
            .filter_map(|layer| layer.mask.as_ref())
            .chain(self.selection.iter())
            .chain(self.previous_selection.iter())
            .flat_map(|mask| {
                mask.tile_coords().filter_map(move |coord| {
                    mask.tile_version(*coord)
                        .map(|version| (version, 256 * 256 * 2))
                })
            });
        pixels.chain(masks)
    }
    pub fn active_layer(&self) -> Option<LayerId> {
        self.active
    }
    pub fn is_descendant(&self, child: LayerId, parent: LayerId) -> bool {
        let mut next = self
            .layers
            .iter()
            .find(|l| l.id() == child)
            .and_then(|l| l.info.parent);
        for _ in 0..32 {
            let Some(id) = next else { return false };
            if id == parent {
                return true;
            }
            next = self
                .layers
                .iter()
                .find(|l| l.id() == id)
                .and_then(|l| l.info.parent);
        }
        false
    }
    pub fn layer_depth(&self, child: LayerId) -> usize {
        let mut next = self
            .layers
            .iter()
            .find(|l| l.id() == child)
            .and_then(|l| l.info.parent);
        let mut depth = 0;
        while let Some(id) = next {
            depth += 1;
            if depth > 16 {
                break;
            }
            next = self
                .layers
                .iter()
                .find(|l| l.id() == id)
                .and_then(|l| l.info.parent);
        }
        depth
    }
    pub fn effective_visibility(&self, id: LayerId) -> bool {
        let mut next = Some(id);
        for _ in 0..17 {
            let Some(id) = next else { return true };
            let Some(layer) = self.layers.iter().find(|l| l.id() == id) else {
                return false;
            };
            if !layer.info.visible {
                return false;
            }
            next = layer.info.parent;
        }
        false
    }
    pub fn style_outset(&self, id: LayerId) -> [i64; 4] {
        let mut result = [0; 4];
        let mut next = Some(id);
        for _ in 0..17 {
            let Some(id) = next else { break };
            let Some(layer) = self.layers.iter().find(|l| l.id() == id) else {
                break;
            };
            let pad = layer.info.styles.outset();
            for i in 0..4 {
                result[i] += pad[i];
            }
            next = layer.info.parent;
        }
        result
    }
    pub(crate) fn insertion_parent(&self) -> Option<LayerId> {
        self.active
            .and_then(|id| self.layers.iter().find(|l| l.id() == id))
            .and_then(|l| {
                if matches!(l.info.kind, crate::LayerKind::Group) {
                    Some(l.id())
                } else {
                    l.info.parent
                }
            })
    }
    pub fn format(&self) -> PixelFormat {
        self.format
    }
    pub fn bit_depth(&self) -> u8 {
        self.format.bit_depth()
    }
    pub fn working_icc(&self) -> &[u8] {
        &self.working_icc
    }
    pub fn source_icc(&self) -> Option<&[u8]> {
        self.source_icc.as_deref()
    }
    pub fn profile_name(&self) -> &str {
        &self.profile_name
    }
    pub fn profile_fallback(&self) -> bool {
        self.profile_fallback
    }
    pub fn resolve(&self, reference: &LayerRef) -> Result<LayerId, DocumentError> {
        let matches: Vec<_> = self
            .layers
            .iter()
            .filter(|l| match reference {
                LayerRef::Id(id) => l.id() == *id,
                LayerRef::Name(name) => l.info.name == *name,
            })
            .map(Layer::id)
            .collect();
        match matches.as_slice() {
            [id] => Ok(*id),
            [] => Err(DocumentError::LayerNotFound),
            _ => Err(DocumentError::Ambiguous(matches)),
        }
    }
    pub(crate) fn validate(&self) -> Result<(), DocumentError> {
        if !(1..=2400).contains(&self.resolution_dpi)
            || self.preset.as_ref().is_some_and(|p| p.len() > 64)
        {
            return Err(DocumentError::Invalid(
                "resolution or preset metadata".into(),
            ));
        }
        self.format
            .byte_count(self.frame.width, self.frame.height)?;
        self.frame
            .x
            .checked_add(i64::from(self.frame.width))
            .ok_or_else(|| DocumentError::Invalid("frame overflow".into()))?;
        self.frame
            .y
            .checked_add(i64::from(self.frame.height))
            .ok_or_else(|| DocumentError::Invalid("frame overflow".into()))?;
        if !self.id.is_valid()
            || !self.state.is_valid()
            || self.frame.name.len() > 1024
            || self.layers.len() > MAX_LAYERS
        {
            return Err(DocumentError::Invalid("metadata".into()));
        }
        let mut ids = std::collections::HashSet::new();
        let mut bytes = 0u64;
        for layer in &self.layers {
            let info = &layer.info;
            info.styles.validate(self.bit_depth())?;
            if !info.styles.is_empty() && info.kind.is_adjustment() {
                return Err(DocumentError::Invalid(
                    "layer styles require pixel, fill or group content".into(),
                ));
            }
            if let Some([width, height]) = info.generated_size
                && (info.kind.is_adjustment()
                    || self.format.byte_count(width, height)? as u64 > MAX_FLAT_IMAGE_BYTES
                    || info.x.checked_add(i64::from(width)).is_none()
                    || info.y.checked_add(i64::from(height)).is_none())
            {
                return Err(DocumentError::Invalid("generated layer geometry".into()));
            }
            if let Some(bounds) = info.spatial_bounds {
                bounds.validate()?;
                if !matches!(
                    info.kind,
                    crate::LayerKind::Adjustment {
                        adjustment: crate::TonalAdjustment::ShadowsHighlights { .. }
                    }
                ) || self.format.byte_count(bounds.width, bounds.height)? as u64
                    > MAX_FLAT_IMAGE_BYTES
                {
                    return Err(DocumentError::Invalid(
                        "spatial adjustment sampling bounds".into(),
                    ));
                }
            }
            if info.clipped {
                let base = self
                    .layers
                    .iter()
                    .take_while(|l| l.id() != info.id)
                    .filter(|l| l.info.parent == info.parent && !l.info.clipped)
                    .last();
                if base.is_none_or(|l| l.info.kind.is_adjustment()) {
                    return Err(DocumentError::Invalid("clipping chain requires a lower pixel, fill or group base in the same parent".into()));
                }
            }
            if let Some(parent) = info.parent
                && (parent == info.id
                    || !self.layers.iter().any(|l| {
                        l.id() == parent && matches!(l.info.kind, crate::LayerKind::Group)
                    })
                    || self.is_descendant(parent, info.id)
                    || self.layer_depth(info.id) > 16)
            {
                return Err(DocumentError::Invalid(
                    "orphan, cyclic or excessively nested layer tree".into(),
                ));
            }
            if matches!(info.kind, crate::LayerKind::Group) && layer.pixels.tile_count() != 0 {
                return Err(DocumentError::Invalid(
                    "groups cannot have pixel blocks".into(),
                ));
            }
            if !matches!(info.kind, crate::LayerKind::Pixel) && layer.pixels.tile_count() != 0 {
                return Err(DocumentError::Invalid(
                    "generated or adjustment layer cannot own source pixels".into(),
                ));
            }
            let colors_valid = |color: &[u16; 4]| {
                self.format != PixelFormat::Rgba8 || color.iter().all(|c| *c <= 255)
            };
            match &info.kind {
                crate::LayerKind::Adjustment { adjustment } => adjustment.validate()?,
                crate::LayerKind::Solid { color } if !colors_valid(color) => {
                    return Err(DocumentError::Invalid(
                        "fill exceeds document bit depth".into(),
                    ));
                }
                crate::LayerKind::Gradient {
                    start,
                    end,
                    angle,
                    scale,
                    ..
                } if !colors_valid(start)
                    || !colors_valid(end)
                    || !angle.is_finite()
                    || !scale.is_finite()
                    || !(0.01..=100.0).contains(scale) =>
                {
                    return Err(DocumentError::Invalid(
                        "gradient colors/angle/scale invalid".into(),
                    ));
                }
                crate::LayerKind::HueSaturation {
                    hue,
                    saturation,
                    lightness,
                } if !hue.is_finite()
                    || !saturation.is_finite()
                    || !lightness.is_finite()
                    || !(-180.0..=180.0).contains(hue)
                    || !(-100.0..=100.0).contains(saturation)
                    || !(-100.0..=100.0).contains(lightness) =>
                {
                    return Err(DocumentError::Invalid(
                        "hue must be -180..180; saturation/lightness -100..100".into(),
                    ));
                }
                _ => {}
            }
            if !info.id.is_valid()
                || !ids.insert(info.id)
                || info.name.len() > 1024
                || !info.opacity.is_finite()
                || !(0.0..=1.0).contains(&info.opacity)
                || !info.fill_opacity.is_finite()
                || !(0.0..=1.0).contains(&info.fill_opacity)
                || layer.pixels.format() != self.format
            {
                return Err(DocumentError::Invalid("layer metadata".into()));
            }
            if let crate::LayerKind::Exposure { stops } = info.kind
                && (!stops.is_finite()
                    || !(-20.0..=20.0).contains(&stops)
                    || layer.pixels.tile_count() != 0)
            {
                return Err(DocumentError::Invalid(
                    "exposure layer parameters or pixel payload".into(),
                ));
            }
            if info.kind.is_adjustment() && layer.pixels.tile_count() != 0 {
                return Err(DocumentError::Invalid(
                    "adjustment layers cannot contain source pixel blocks".into(),
                ));
            }
            match (&info.mask, &layer.mask) {
                (None, None) => {}
                (Some(meta), Some(mask)) if meta.layout == mask.layout() => {
                    let bounds = meta.layout.bounds;
                    for (origin, anchor, position, length) in [
                        (bounds.x, meta.anchor_x, info.x, bounds.width),
                        (bounds.y, meta.anchor_y, info.y, bounds.height),
                    ] {
                        if origin
                            .checked_sub(anchor)
                            .and_then(|v| v.checked_add(position))
                            .and_then(|v| v.checked_add(i64::from(length)))
                            .is_none()
                        {
                            return Err(DocumentError::Invalid("mask position overflow".into()));
                        }
                    }
                    bytes = bytes
                        .checked_add(mask.referenced_bytes())
                        .ok_or(DocumentError::ResourceLimit)?;
                }
                _ => {
                    return Err(DocumentError::Invalid(
                        "mask metadata/payload mismatch".into(),
                    ));
                }
            }
            bytes = bytes
                .checked_add(layer.pixels.referenced_bytes())
                .ok_or(DocumentError::ResourceLimit)?;
            for coord in layer.pixels.tile_coords() {
                let [x, y] = coord.origin();
                for (origin, offset) in [(x, info.x), (y, info.y)] {
                    origin
                        .checked_add(offset)
                        .and_then(|v| v.checked_add(255))
                        .ok_or_else(|| DocumentError::Invalid("layer position overflow".into()))?;
                }
            }
        }
        if let Some(selection) = &self.selection {
            let bounds = selection.layout().bounds;
            if (bounds.x, bounds.y, bounds.width, bounds.height)
                != (
                    self.frame.x,
                    self.frame.y,
                    self.frame.width,
                    self.frame.height,
                )
            {
                return Err(DocumentError::Invalid(
                    "selection must be bounded to the artboard".into(),
                ));
            }
            bytes = bytes
                .checked_add(selection.referenced_bytes())
                .ok_or(DocumentError::ResourceLimit)?;
        }
        if let Some(selection) = &self.previous_selection {
            let bounds = selection.layout().bounds;
            if (bounds.x, bounds.y, bounds.width, bounds.height)
                != (
                    self.frame.x,
                    self.frame.y,
                    self.frame.width,
                    self.frame.height,
                )
            {
                return Err(DocumentError::Invalid("previous selection bounds".into()));
            }
            bytes = bytes
                .checked_add(selection.referenced_bytes())
                .ok_or(DocumentError::ResourceLimit)?;
        }
        if bytes > MAX_DOCUMENT_BYTES
            || self
                .format
                .byte_count(self.frame.width, self.frame.height)? as u64
                > MAX_FLAT_IMAGE_BYTES
        {
            return Err(DocumentError::ResourceLimit);
        }
        if self.active.is_some_and(|id| !ids.contains(&id)) {
            return Err(DocumentError::LayerNotFound);
        }
        Ok(())
    }
}

pub struct DocumentCandidate(Document);
impl DocumentCandidate {
    pub fn from_image(
        width: u32,
        height: u32,
        pixels: PixelBuffer,
        icc: Option<Vec<u8>>,
        name: String,
    ) -> Result<Self, DocumentError> {
        Self::from_image_with_pool(
            width,
            height,
            pixels,
            icc,
            name,
            strata_pixels::memory::MemoryPool::global(),
        )
    }
    pub fn from_image_with_pool(
        width: u32,
        height: u32,
        pixels: PixelBuffer,
        icc: Option<Vec<u8>>,
        name: String,
        pool: strata_pixels::memory::MemoryPool,
    ) -> Result<Self, DocumentError> {
        pixels.validate(width, height)?;
        if pixels.byte_len() as u64 > MAX_FLAT_IMAGE_BYTES
            || icc.as_ref().is_some_and(|v| v.len() > 4 * 1024 * 1024)
        {
            return Err(DocumentError::ResourceLimit);
        }
        let profile = strata_color::resolve_profile(icc.as_deref())?;
        let format = pixels.format();
        let layer = Layer::new(
            name,
            TiledPixels::from_dense_with_pool(
                width,
                height,
                &pixels,
                tile_limit(format),
                pool.clone(),
            )?,
        );
        let active = Some(layer.id());
        let doc = Document {
            selection: None,
            previous_selection: None,
            resolution_dpi: default_resolution_dpi(),
            preset: None,
            pool,
            id: DocumentId::fresh(),
            state: StateId::fresh(),
            frame: Frame {
                name: "Artboard 1".into(),
                x: 0,
                y: 0,
                width,
                height,
            },
            layers: vec![layer],
            active,
            format,
            working_icc: profile.icc,
            source_icc: icc,
            profile_name: profile.name,
            profile_fallback: profile.fallback,
        };
        doc.validate()?;
        Ok(Self(doc))
    }
    pub fn into_document(self) -> Document {
        self.0
    }
}
pub(crate) fn tile_limit(format: PixelFormat) -> usize {
    (MAX_DOCUMENT_BYTES / (256 * 256 * 4 * format.bytes_per_sample() as u64)) as usize
}
