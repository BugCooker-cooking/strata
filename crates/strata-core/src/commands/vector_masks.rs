use super::*;
use crate::{Document, DocumentError, LayerId, PathId, VectorMask};
use serde_json::json;

params!(SetMask{
    layer:LayerRef,
    #[serde(default)] path:Option<super::paths::PathRef>,
    #[serde(default)] enabled:Option<bool>, #[serde(default)] inverted:Option<bool>,
    #[serde(default)] linked:Option<bool>, #[serde(default)] expected_revision:Option<u64>
});
params!(RemoveMask{layer:LayerRef,#[serde(default)] expected_revision:Option<u64>});
params!(MaskResult{layer_id:LayerId,vector_mask:Option<VectorMask>,changed:bool});

pub(super) fn check_layer(doc: &Document, target: LayerId) -> Result<(), CommandError> {
    if doc.layers().iter().any(|l| {
        (l.id() == target || doc.is_descendant(target, l.id()))
            && (l.info().locks.all || l.info().locks.pixels || l.info().locks.transparency)
    }) {
        return Err(CommandError::Invalid(
            "unlock the vector mask layer and its ancestors before editing".into(),
        ));
    }
    Ok(())
}
pub(super) fn check_path_edit(doc: &Document, path: PathId) -> Result<(), CommandError> {
    for layer in doc.layers().iter().filter(|l| {
        l.info()
            .vector_mask
            .as_ref()
            .is_some_and(|m| m.path == path)
    }) {
        check_layer(doc, layer.id())?;
    }
    Ok(())
}
pub(super) fn mutate(
    doc: &mut Document,
    id: &str,
    params: Value,
    revision: u64,
) -> Result<Value, CommandError> {
    let (layer, expected, update) = match id {
        "layer.vector_mask.set" => {
            let p: SetMask = parse(params)?;
            if p.path.is_none() && p.enabled.is_none() && p.inverted.is_none() && p.linked.is_none()
            {
                return Err(CommandError::Invalid(
                    "supply a source path or vector mask settings".into(),
                ));
            }
            (doc.resolve(&p.layer)?, p.expected_revision, Some(p))
        }
        "layer.vector_mask.remove" => {
            let p: RemoveMask = parse(params)?;
            (doc.resolve(&p.layer)?, p.expected_revision, None)
        }
        _ => return Err(CommandError::Invalid("unknown vector mask edit".into())),
    };
    if expected.is_some_and(|r| r != revision) {
        return Err(CommandError::Stale);
    }
    check_layer(doc, layer)?;
    let i = doc
        .layers
        .iter()
        .position(|l| l.id() == layer)
        .ok_or(DocumentError::LayerNotFound)?;
    let info = doc.layers[i].info();
    let value = if let Some(p) = update {
        let mut mask = if let Some(reference) = p.path {
            let path = doc.paths[super::paths::resolve(doc, &reference)?].id;
            if let Some(old) = info.vector_mask.as_ref().filter(|m| m.path == path) {
                old.clone()
            } else {
                VectorMask::new(path, info, p.linked.unwrap_or(true))
            }
        } else {
            info.vector_mask.clone().ok_or_else(|| {
                CommandError::Invalid("bind a path before editing vector mask settings".into())
            })?
        };
        if let Some(enabled) = p.enabled {
            mask.enabled = enabled;
        }
        if let Some(inverted) = p.inverted {
            mask.inverted = inverted;
        }
        if let Some(linked) = p.linked {
            mask.relink(info, linked)?;
        }
        Some(mask)
    } else {
        None
    };
    let changed = value != info.vector_mask;
    let info = &mut doc.layers[i].info;
    if let Some(shape) = &mut info.shape_paint
        && shape.owned_path != value.as_ref().map(|m| m.path)
    {
        shape.owned_path = None;
    }
    info.vector_mask = value.clone();
    Ok(json!(MaskResult {
        layer_id: layer,
        vector_mask: value,
        changed
    }))
}
