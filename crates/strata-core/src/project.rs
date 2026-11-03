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
    pub fn to_archive(&self) -> Result<ProjectArchive, DocumentError> {
        self.validate()?;
        let metadata = serde_json::to_value(Manifest {
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
        let mut tiles = Vec::new();
        for layer in &self.layers {
            for (coord, data) in layer.pixels.tiles() {
                let bytes = match data {
                    PixelBuffer::Rgba8(v) => v.clone(),
                    PixelBuffer::Rgba16(v) => v.iter().flat_map(|v| v.to_le_bytes()).collect(),
                };
                tiles.push(ProjectTile {
                    layer: layer.id().to_string(),
                    x: coord.x(),
                    y: coord.y(),
                    bit_depth: self.bit_depth(),
                    bytes,
                });
            }
        }
        Ok(ProjectArchive { metadata, tiles })
    }
    pub fn from_archive(archive: ProjectArchive) -> Result<Self, DocumentError> {
        let meta: Manifest = serde_json::from_value(archive.metadata)
            .map_err(|e| DocumentError::Invalid(e.to_string()))?;
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
        let mut layers: Vec<_> = meta
            .layers
            .into_iter()
            .map(|info| Layer {
                info,
                pixels: TiledPixels::new(format, tile_limit(format)),
            })
            .collect();
        let mut seen = std::collections::HashSet::new();
        for tile in archive.tiles {
            if tile.bit_depth != meta.bit_depth
                || !seen.insert((tile.layer.clone(), tile.x, tile.y))
            {
                return Err(DocumentError::Invalid("duplicate tile or depth".into()));
            }
            let layer = layers
                .iter_mut()
                .find(|l| l.id().to_string() == tile.layer)
                .ok_or_else(|| DocumentError::Invalid("orphan tile".into()))?;
            let coord = TileCoord::new(tile.x, tile.y)?;
            let block = match format {
                PixelFormat::Rgba8 => PixelBuffer::Rgba8(tile.bytes),
                PixelFormat::Rgba16 => {
                    if tile.bytes.len() % 2 != 0 {
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
        }
        let doc = Self {
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
}
