//! Reproducible input fixtures, streamed into a normal project. This creates
//! test data, not a back door for editing an open application document.
use serde_json::{Value, json};
use std::{error::Error, path::Path};
use strata_core::{Editor, Source};
use strata_io::project::{ProjectSource, ProjectTile, StorageError, write_project_source};
use strata_pixels::PixelFormat;
pub const SEED: u64 = 0x5354_5241_5441_2026;
pub struct Dataset {
    pub metadata: Value,
    pub width: u32,
    pub height: u32,
    pub depth: u8,
    pub layer_ids: Vec<String>,
}
impl Dataset {
    pub fn new(width: u32, height: u32, layers: usize, depth: u8) -> Result<Self, Box<dyn Error>> {
        if layers == 0 || layers > 30 {
            return Err("layer count must be 1..30".into());
        }
        let format = match depth {
            8 => PixelFormat::Rgba8,
            16 => PixelFormat::Rgba16,
            _ => return Err("depth must be 8/16".into()),
        };
        if format.byte_count(width, height)? as u64 > strata_core::MAX_FLAT_IMAGE_BYTES {
            return Err("frame exceeds 512 MiB".into());
        }
        if u64::from(width.div_ceil(256))
            * u64::from(height.div_ceil(256))
            * layers as u64
            * (256 * 256 * 4 * (depth as u64 / 8))
            > strata_core::MAX_DOCUMENT_BYTES
        {
            return Err("project exceeds source byte limit".into());
        }
        let mut editor = Editor::new();
        let request = editor.request(Source::Test);
        editor.execute(
            request,
            "file.new",
            json!({"width":width,"height":height,"bit_depth":depth}),
            |_| Ok(()),
        )?;
        let mut metadata = editor
            .document()
            .ok_or("no fixture document")?
            .to_archive()?
            .metadata;
        let template = metadata["layers"][0].clone();
        let layer_ids: Vec<_> = (0..layers)
            .map(|_| uuid::Uuid::new_v4().to_string())
            .collect();
        metadata["layers"] = Value::Array(
            layer_ids
                .iter()
                .enumerate()
                .map(|(index, id)| {
                    let mut value = template.clone();
                    value["id"] = json!(id);
                    value["name"] = json!(format!("Independent random {index}"));
                    value
                })
                .collect(),
        );
        metadata["active"] = json!(layer_ids.last());
        Ok(Self {
            metadata,
            width,
            height,
            depth,
            layer_ids,
        })
    }
    pub fn pixel(&self, layer: usize, x: u32, y: u32) -> [u16; 4] {
        if x >= self.width || y >= self.height {
            return [0; 4];
        }
        let mut value = SEED
            ^ ((layer as u64 + 1).wrapping_mul(0x9e37_79b9_7f4a_7c15))
            ^ (u64::from(y) << 32 | u64::from(x));
        value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        value ^= value >> 31;
        if self.depth == 16 {
            [
                value as u16,
                (value >> 16) as u16,
                (value >> 32) as u16,
                (value >> 48) as u16,
            ]
        } else {
            [
                u16::from(value as u8),
                u16::from((value >> 16) as u8),
                u16::from((value >> 32) as u8),
                u16::from((value >> 48) as u8),
            ]
        }
    }
    pub fn write(&self, path: &Path) -> Result<(), Box<dyn Error>> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        write_project_source(path, self, false, false)?;
        Ok(())
    }
}
impl ProjectSource for Dataset {
    fn metadata(&self) -> Result<Value, StorageError> {
        Ok(self.metadata.clone())
    }
    fn visit_tiles(
        &self,
        visit: &mut dyn FnMut(ProjectTile) -> Result<(), StorageError>,
    ) -> Result<(), StorageError> {
        for (layer, id) in self.layer_ids.iter().enumerate() {
            for tx in 0..self.width.div_ceil(256) {
                for ty in 0..self.height.div_ceil(256) {
                    let mut bytes = Vec::with_capacity(256 * 256 * 4 * (self.depth as usize / 8));
                    for y in 0..256 {
                        for x in 0..256 {
                            let pixel = self.pixel(layer, tx * 256 + x, ty * 256 + y);
                            if self.depth == 8 {
                                bytes.extend(pixel.map(|v| v as u8));
                            } else {
                                bytes.extend(pixel.into_iter().flat_map(u16::to_le_bytes));
                            }
                        }
                    }
                    visit(ProjectTile {
                        channels: 4,
                        layer: id.clone(),
                        x: i64::from(tx),
                        y: i64::from(ty),
                        bit_depth: self.depth,
                        bytes,
                    })?;
                }
            }
            eprintln!(
                "STRATA_DATASET generated {}/{}",
                layer + 1,
                self.layer_ids.len()
            );
        }
        Ok(())
    }
}
