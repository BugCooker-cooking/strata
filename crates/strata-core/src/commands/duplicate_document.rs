use super::{CommandError, DuplicateDocument, Editor, Source};
use crate::{Document, DocumentError, DocumentId, Layer, StateId};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use strata_pixels::{
    TILE_EDGE, TiledPixels,
    memory::{MemoryKind, Reservation},
};

/// A committed document handed to a trusted workspace host. No pixel data or
/// capability is serialized in the command response.
pub struct DocumentCopy {
    pub name: String,
    pub editor: Editor,
    pub source: Source,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DuplicateMode {
    #[default]
    Layered,
    Merged,
}

#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DuplicateResult {
    pub copy_document_id: DocumentId,
    pub name: String,
    pub mode: DuplicateMode,
}

pub(super) fn prepare(
    original: &Document,
    params: &DuplicateDocument,
    cancel: &dyn Fn() -> bool,
) -> Result<(Document, Arc<Reservation>), CommandError> {
    if params.name.trim().is_empty()
        || params.name.len() > 256
        || params.name.chars().any(char::is_control)
    {
        return Err(CommandError::Invalid(
            "copy name must contain 1..256 UTF-8 bytes without control characters".into(),
        ));
    }
    if cancel() {
        return Err(CommandError::Expired);
    }

    let frame = original.frame();
    let pool = original.memory_pool();
    let (copy_tiles, flat_bytes) = match params.mode {
        DuplicateMode::Layered => {
            let tiles = original
                .layers()
                .iter()
                .map(|layer| {
                    (layer.pixels().tile_count() + layer.mask().map_or(0, |mask| mask.tile_count()))
                        as u64
                })
                .sum::<u64>();
            (tiles, 0)
        }
        DuplicateMode::Merged => {
            let bytes = original
                .format()
                .byte_count(frame.width, frame.height)
                .map_err(DocumentError::from)? as u64;
            if bytes > crate::MAX_FLAT_IMAGE_BYTES {
                return Err(DocumentError::ResourceLimit.into());
            }
            let tiles = u64::from(frame.width.div_ceil(TILE_EDGE))
                * u64::from(frame.height.div_ceil(TILE_EDGE));
            (tiles, bytes)
        }
    };
    let selection_tiles = [original.selection(), original.previous_selection()]
        .into_iter()
        .flatten()
        .map(|mask| mask.tile_count() as u64)
        .sum::<u64>();
    let profile_bytes = original.working_icc().len() + original.source_icc().map_or(0, <[u8]>::len);
    // Actual shared tiles already have source reservations. Budget an estimate
    // for the new index entries and cloned document metadata separately.
    let allocation = Arc::new(
        pool.reserve(
            MemoryKind::Source,
            (copy_tiles + selection_tiles) * 128 + 1024 * 1024 + profile_bytes as u64,
        )
        .map_err(DocumentError::from)?,
    );

    let mut copy = if params.mode == DuplicateMode::Layered {
        // Layer IDs are local to the document and seed Dissolve's noise.
        // Preserve them so the copied canvas has the exact same appearance.
        original.clone()
    } else {
        let _scratch = pool
            .reserve(MemoryKind::Transient, flat_bytes + 512 * 1024)
            .map_err(DocumentError::from)?;
        let pixels = original.export_pixels_cancellable(cancel)?;
        let tiled = TiledPixels::from_dense_with_pool_cancellable(
            frame.width,
            frame.height,
            &pixels,
            crate::document::tile_limit(original.format()),
            pool.clone(),
            cancel,
        )
        .map_err(|error| match error {
            strata_pixels::TileError::Cancelled => CommandError::Expired,
            other => DocumentError::from(other).into(),
        })?;
        let mut layer = Layer::new("Merged".into(), tiled);
        layer.info.x = frame.x;
        layer.info.y = frame.y;
        Document {
            paths: original.paths.clone(),
            guides: original.guides.clone(),
            selection: original.selection.clone(),
            previous_selection: original.previous_selection.clone(),
            resolution_dpi: original.resolution_dpi,
            preset: original.preset.clone(),
            pool,
            id: original.id,
            state: original.state,
            frame: frame.clone(),
            active: Some(layer.id()),
            layers: vec![layer],
            format: original.format,
            working_icc: original.working_icc.clone(),
            source_icc: original.source_icc.clone(),
            profile_name: original.profile_name.clone(),
            profile_fallback: original.profile_fallback,
        }
    };
    copy.id = DocumentId::fresh();
    copy.state = StateId::fresh();
    if cancel() {
        return Err(CommandError::Expired);
    }
    copy.validate()?;
    Ok((copy, allocation))
}
