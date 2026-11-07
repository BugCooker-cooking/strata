use crate::{
    Document, DocumentError, DocumentId, Frame, Layer, LayerId, LayerInfo, StateId,
    document::tile_limit,
};
use serde::{Deserialize, Serialize};
use strata_io::project::{ProjectArchive, ProjectTile};
use strata_pixels::{MaskLayout, PixelBuffer, PixelFormat, TileCoord, TiledMask, TiledPixels};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    #[serde(default)]
    selection: Option<MaskLayout>,
    #[serde(default)]
    previous_selection: Option<MaskLayout>,
    #[serde(default = "crate::document::default_resolution_dpi")]
    resolution_dpi: u32,
    #[serde(default)]
    preset: Option<String>,
    version: u32,
    id: DocumentId,
    state: StateId,
    frame: Frame,
    layers: Vec<LayerInfo>,
    active: Option<LayerId>,
    bit_depth: u8,
    working_icc: Vec<u8>,
    source_icc: Option<Vec<u8>>,
    profile_name: String,
    profile_fallback: bool,
}
impl Document {
    pub(crate) fn manifest(&self) -> Result<serde_json::Value, DocumentError> {
        self.validate()?;
        let metadata = serde_json::to_value(Manifest {
            selection: self.selection.as_ref().map(TiledMask::layout),
            previous_selection: self.previous_selection.as_ref().map(TiledMask::layout),
            resolution_dpi: self.resolution_dpi,
            preset: self.preset.clone(),
            version: if self.layers.iter().any(|l|matches!(&l.info.kind, crate::LayerKind::Text{text} if !text.operations.is_empty())) {
                8
            } else if self
                .layers
                .iter()
                .any(|l| matches!(l.info.kind, crate::LayerKind::Text { .. }))
            {
                7
            } else {
                6
            },
            id: self.id,
            state: self.state,
            frame: self.frame.clone(),
            layers: self.layers.iter().map(|l| l.info.clone()).collect(),
            active: self.active,
            bit_depth: self.bit_depth(),
            working_icc: self.working_icc.clone(),
            source_icc: self.source_icc.clone(),
            profile_name: self.profile_name.clone(),
            profile_fallback: self.profile_fallback,
        })
        .map_err(|e| DocumentError::Invalid(e.to_string()))?;
        Ok(metadata)
    }
    pub fn to_archive(&self) -> Result<ProjectArchive, DocumentError> {
        if self.source_bytes() > crate::MAX_FLAT_IMAGE_BYTES {
            return Err(DocumentError::ResourceLimit);
        }
        use strata_io::project::ProjectSource;
        let mut tiles = Vec::new();
        self.visit_tiles(&mut |tile| {
            tiles.push(tile);
            Ok(())
        })
        .map_err(|e| DocumentError::Invalid(e.to_string()))?;
        Ok(ProjectArchive {
            metadata: self.manifest()?,
            tiles,
        })
    }
    pub fn from_archive(archive: ProjectArchive) -> Result<Self, DocumentError> {
        let mut doc = Self::from_manifest(archive.metadata)?;
        let mut seen = std::collections::HashSet::new();
        for tile in archive.tiles {
            if !seen.insert((tile.layer.clone(), tile.x, tile.y)) {
                return Err(DocumentError::Invalid("duplicate tile".into()));
            }
            doc.insert_project_tile(tile)?;
        }
        doc.validate()?;
        Ok(doc)
    }
    fn from_manifest(metadata: serde_json::Value) -> Result<Self, DocumentError> {
        Self::from_manifest_with_pool(metadata, strata_pixels::memory::MemoryPool::global())
    }
    fn from_manifest_with_pool(
        metadata: serde_json::Value,
        pool: strata_pixels::memory::MemoryPool,
    ) -> Result<Self, DocumentError> {
        let meta: Manifest =
            serde_json::from_value(metadata).map_err(|e| DocumentError::Invalid(e.to_string()))?;
        if !matches!(meta.version, 0..=8)
            || meta.layers.len() > crate::MAX_LAYERS
            || meta.working_icc.len() > 4 * 1024 * 1024
            || meta
                .source_icc
                .as_ref()
                .is_some_and(|v| v.len() > 4 * 1024 * 1024)
            || meta.profile_name.len() > 1024
        {
            return Err(DocumentError::Invalid(
                "project version or metadata size".into(),
            ));
        }
        if meta.version == 0
            && (meta.selection.is_some()
                || meta
                    .layers
                    .iter()
                    .any(|l| l.kind != crate::LayerKind::Pixel || l.mask.is_some()))
        {
            return Err(DocumentError::Invalid(
                "v0 manifest contains newer feature metadata".into(),
            ));
        }
        if meta.version < 3 && meta.previous_selection.is_some() {
            return Err(DocumentError::Invalid(
                "previous selection requires manifest version 3".into(),
            ));
        }
        if meta.version < 4
            && meta
                .layers
                .iter()
                .any(|l| matches!(l.kind, crate::LayerKind::Adjustment { .. }))
        {
            return Err(DocumentError::Invalid(
                "tonal adjustments require manifest version 4".into(),
            ));
        }
        if meta.version < 5
            && meta.layers.iter().any(|l| {
                l.spatial_bounds.is_some()
                    || l.generated_size.is_some()
                        && !matches!(
                            l.kind,
                            crate::LayerKind::Solid { .. } | crate::LayerKind::Gradient { .. }
                        )
            })
        {
            return Err(DocumentError::Invalid(
                "frozen canvas/style/spatial references require manifest version 5".into(),
            ));
        }
        if meta.version < 6 && meta.layers.iter().any(|l| l.generated_crop.is_some()) {
            return Err(DocumentError::Invalid(
                "finite generated crop requires manifest version 6".into(),
            ));
        }
        if meta.version < 7
            && meta
                .layers
                .iter()
                .any(|l| matches!(l.kind, crate::LayerKind::Text { .. }))
        {
            return Err(DocumentError::Invalid(
                "editable text requires manifest version7".into(),
            ));
        }
        if meta.version < 8
            && meta.layers.iter().any(
                |l| matches!(&l.kind,crate::LayerKind::Text{text} if !text.operations.is_empty()),
            )
        {
            return Err(DocumentError::Invalid(
                "retained text geometry requires manifest version8".into(),
            ));
        }
        if meta.version < 2
            && meta.layers.iter().any(|l| {
                l.parent.is_some()
                    || !l.styles.is_empty()
                    || l.clipped
                    || l.pass_through
                    || !matches!(
                        l.kind,
                        crate::LayerKind::Pixel | crate::LayerKind::Exposure { .. }
                    )
                    || l.fill_opacity != 1.0
                    || l.locks != crate::LayerLocks::default()
                    || l.color_label.is_some()
            })
        {
            return Err(DocumentError::Invalid(
                "legacy manifest contains layer-tree or locking metadata requiring version 2"
                    .into(),
            ));
        }
        let format = match meta.bit_depth {
            8 => PixelFormat::Rgba8,
            16 => PixelFormat::Rgba16,
            _ => return Err(DocumentError::Invalid("bit depth".into())),
        };
        let profile = strata_color::resolve_profile(Some(&meta.working_icc))?;
        if profile.fallback {
            return Err(DocumentError::Invalid("invalid working ICC".into()));
        }
        let layers: Vec<_> = meta
            .layers
            .into_iter()
            .map(|info| {
                Ok(Layer {
                    mask: info
                        .mask
                        .as_ref()
                        .map(|m| TiledMask::new(m.layout, mask_tile_limit(), pool.clone()))
                        .transpose()?,
                    info,
                    pixels: TiledPixels::with_pool(format, tile_limit(format), pool.clone()),
                })
            })
            .collect::<Result<Vec<_>, DocumentError>>()?;
        let doc = Self {
            previous_selection: meta
                .previous_selection
                .map(|layout| TiledMask::new(layout, mask_tile_limit(), pool.clone()))
                .transpose()?,
            selection: meta
                .selection
                .map(|layout| TiledMask::new(layout, mask_tile_limit(), pool.clone()))
                .transpose()?,
            resolution_dpi: meta.resolution_dpi,
            preset: meta.preset,
            pool,
            id: meta.id,
            state: meta.state,
            frame: meta.frame,
            layers,
            active: meta.active,
            format,
            working_icc: meta.working_icc,
            source_icc: meta.source_icc,
            profile_name: meta.profile_name,
            profile_fallback: meta.profile_fallback,
        };
        doc.validate()?;
        Ok(doc)
    }
    fn insert_project_tile(&mut self, tile: ProjectTile) -> Result<(), DocumentError> {
        if tile.channels == 1 {
            if tile.bit_depth != 16 || tile.bytes.len() != 256 * 256 * 2 {
                return Err(DocumentError::Invalid("coverage encoding".into()));
            }
            let mask = if tile.layer == "previous_selection" {
                self.previous_selection.as_mut()
            } else if tile.layer == "selection" {
                self.selection.as_mut()
            } else if let Some(id) = tile.layer.strip_prefix("mask:") {
                self.layers
                    .iter_mut()
                    .find(|l| l.id().to_string() == id)
                    .and_then(|l| l.mask.as_mut())
            } else {
                None
            }
            .ok_or_else(|| DocumentError::Invalid("orphan coverage plane".into()))?;
            let samples = tile
                .bytes
                .chunks_exact(2)
                .map(|v| u16::from_le_bytes([v[0], v[1]]))
                .collect();
            mask.insert_encoded_tile(TileCoord::new(tile.x, tile.y)?, samples)?;
            return Ok(());
        }
        if tile.bit_depth != self.bit_depth() || tile.channels != 4 {
            return Err(DocumentError::Invalid("duplicate tile or depth".into()));
        }
        let layer = self
            .layers
            .iter_mut()
            .find(|l| l.id().to_string() == tile.layer)
            .ok_or_else(|| DocumentError::Invalid("orphan tile".into()))?;
        let coord = TileCoord::new(tile.x, tile.y)?;
        let block = match self.format {
            PixelFormat::Rgba8 => PixelBuffer::Rgba8(tile.bytes),
            PixelFormat::Rgba16 => {
                if !tile.bytes.len().is_multiple_of(2) {
                    return Err(DocumentError::Invalid("odd 16-bit payload".into()));
                }
                PixelBuffer::Rgba16(
                    tile.bytes
                        .chunks_exact(2)
                        .map(|v| u16::from_le_bytes([v[0], v[1]]))
                        .collect(),
                )
            }
        };
        layer.pixels.insert_tile(coord, block)?;
        Ok(())
    }
    pub(crate) fn read_file(
        path: &std::path::Path,
        recovery: bool,
        pool: strata_pixels::memory::MemoryPool,
    ) -> Result<Self, crate::commands::CommandError> {
        let _io = pool
            .reserve(
                strata_pixels::memory::MemoryKind::Transient,
                10 * 1024 * 1024,
            )
            .map_err(DocumentError::from)?;
        let mut doc: Option<Self> = None;
        strata_io::project::read_project_stream(path, |metadata, tile| {
            let invalid =
                |e: DocumentError| strata_io::project::StorageError::Invalid(e.to_string());
            if let Some(tile) = tile {
                doc.as_mut()
                    .ok_or_else(|| {
                        strata_io::project::StorageError::Invalid("missing manifest".into())
                    })?
                    .insert_project_tile(tile)
                    .map_err(invalid)?;
            } else {
                let metadata = if recovery {
                    metadata.get("document").ok_or_else(|| {
                        strata_io::project::StorageError::Invalid(
                            "not a recovery checkpoint".into(),
                        )
                    })?
                } else {
                    metadata
                };
                doc = Some(
                    Self::from_manifest_with_pool(metadata.clone(), pool.clone())
                        .map_err(invalid)?,
                );
            }
            Ok(())
        })?;
        let doc = doc.ok_or_else(|| DocumentError::Invalid("missing manifest".into()))?;
        doc.validate()?;
        Ok(doc)
    }
}
impl strata_io::project::ProjectSource for Document {
    fn metadata(&self) -> Result<serde_json::Value, strata_io::project::StorageError> {
        self.manifest()
            .map_err(|e| strata_io::project::StorageError::Invalid(e.to_string()))
    }
    fn visit_tiles(
        &self,
        visit: &mut dyn FnMut(ProjectTile) -> Result<(), strata_io::project::StorageError>,
    ) -> Result<(), strata_io::project::StorageError> {
        for layer in &self.layers {
            for (coord, data) in layer.pixels.tiles() {
                let data =
                    data.map_err(|e| strata_io::project::StorageError::Invalid(e.to_string()))?;
                let _scratch = self
                    .pool
                    .reserve(
                        strata_pixels::memory::MemoryKind::Transient,
                        data.byte_len() as u64,
                    )
                    .map_err(|e| strata_io::project::StorageError::Invalid(e.to_string()))?;
                let bytes = match &*data {
                    PixelBuffer::Rgba8(v) => v.clone(),
                    PixelBuffer::Rgba16(v) => v.iter().flat_map(|v| v.to_le_bytes()).collect(),
                };
                visit(ProjectTile {
                    layer: layer.id().to_string(),
                    x: coord.x(),
                    y: coord.y(),
                    bit_depth: self.bit_depth(),
                    channels: 4,
                    bytes,
                })?;
            }
            if let Some(mask) = &layer.mask {
                write_mask(format!("mask:{}", layer.id()), mask, &self.pool, visit)?;
            }
        }
        if let Some(selection) = &self.selection {
            write_mask("selection".into(), selection, &self.pool, visit)?;
        }
        if let Some(selection) = &self.previous_selection {
            write_mask("previous_selection".into(), selection, &self.pool, visit)?;
        }
        Ok(())
    }
}

pub(crate) fn mask_tile_limit() -> usize {
    (crate::MAX_DOCUMENT_BYTES / (256 * 256 * 2)) as usize
}
fn write_mask(
    name: String,
    mask: &TiledMask,
    pool: &strata_pixels::memory::MemoryPool,
    visit: &mut dyn FnMut(ProjectTile) -> Result<(), strata_io::project::StorageError>,
) -> Result<(), strata_io::project::StorageError> {
    for (coord, tile) in mask.tiles() {
        let tile = tile.map_err(|e| strata_io::project::StorageError::Invalid(e.to_string()))?;
        let _scratch = pool
            .reserve(strata_pixels::memory::MemoryKind::Transient, 256 * 256 * 2)
            .map_err(|e| strata_io::project::StorageError::Invalid(e.to_string()))?;
        visit(ProjectTile {
            layer: name.clone(),
            x: coord.x(),
            y: coord.y(),
            bit_depth: 16,
            channels: 1,
            bytes: tile.iter().flat_map(|v| v.to_le_bytes()).collect(),
        })?;
    }
    Ok(())
}
