use crate::{
    Document, DocumentError, DocumentId, Frame, Layer, LayerId, LayerInfo, StateId,
    document::tile_limit,
};
use serde::{Deserialize, Serialize};
use strata_io::project::{ProjectArchive, ProjectTile};
use strata_pixels::{PixelBuffer, PixelFormat, TileCoord, TiledPixels};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
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
            resolution_dpi: self.resolution_dpi,
            preset: self.preset.clone(),
            version: 0,
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
        if self
            .layers
            .iter()
            .map(|l| l.pixels.referenced_bytes())
            .sum::<u64>()
            > crate::MAX_FLAT_IMAGE_BYTES
        {
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
        if meta.version != 0
            || meta.layers.len() > crate::MAX_LAYERS
            || meta.working_icc.len() > 4 * 1024 * 1024
            || meta
                .source_icc
                .as_ref()
                .is_some_and(|v| v.len() > 4 * 1024 * 1024)
        {
            return Err(DocumentError::Invalid(
                "project version or metadata size".into(),
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
            .map(|info| Layer {
                info,
                pixels: TiledPixels::with_pool(format, tile_limit(format), pool.clone()),
            })
            .collect();
        let doc = Self {
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
            profile_name: profile.name,
            profile_fallback: meta.profile_fallback,
        };
        doc.validate()?;
        Ok(doc)
    }
    fn insert_project_tile(&mut self, tile: ProjectTile) -> Result<(), DocumentError> {
        if tile.bit_depth != self.bit_depth() {
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
                    bytes,
                })?;
            }
        }
        Ok(())
    }
}
