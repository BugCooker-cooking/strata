use super::*;
use crate::{
    Document, DocumentError, LayerId, LayerKind, PathAnchor, PathId, ShapeDefinition,
    ShapeGeometry, ShapePaint, VectorMask, WorkPath,
};
use serde_json::json;
use std::sync::Arc;

params!(CreateShape{
    #[schemars(length(min=1,max=256))] name:String,
    geometry:ShapeGeometry,color:[u16;4],
    #[serde(default)] expected_revision:Option<u64>,
    #[serde(default)] expected_document:Option<crate::DocumentId>,
    #[serde(default)] expected_active_layer:Option<LayerId>
});
params!(EditShape{
    layer:LayerRef,#[serde(default)] geometry:Option<ShapeGeometry>,
    #[serde(default)] color:Option<[u16;4]>,#[serde(default)] expected_revision:Option<u64>
});
params!(ShapeResult {
    layer_id: LayerId,
    path_id: PathId,
    definition: ShapeDefinition,
    changed: bool
});
params!(Outline {
    geometry: ShapeGeometry
});
params!(OutlineResult{anchors:Vec<PathAnchor>});

fn validate_color(doc: &Document, color: [u16; 4]) -> Result<(), CommandError> {
    if doc.bit_depth() == 8 && color.iter().any(|&v| v > 255) {
        return Err(CommandError::Invalid(
            "8-bit shape colors must use 0..255".into(),
        ));
    }
    Ok(())
}
pub(super) fn mutate(
    doc: &mut Document,
    id: &str,
    params: Value,
    revision: u64,
) -> Result<Value, CommandError> {
    let (layer_id, path_id, definition, changed) = match id {
        "shape.create" => {
            let p: CreateShape = parse(params)?;
            if p.expected_active_layer
                .is_some_and(|id| Some(id) != doc.active_layer())
                || p.expected_document.is_some_and(|v| v != doc.id())
                || p.expected_revision.is_some_and(|v| v != revision)
            {
                return Err(CommandError::Stale);
            }
            if doc.paths.len() >= crate::paths::MAX_PATHS || doc.layers.len() >= crate::MAX_LAYERS {
                return Err(DocumentError::ResourceLimit.into());
            }
            if let Some(parent) = doc.insertion_parent() {
                super::vector_masks::check_layer(doc, parent)?;
            }
            validate_color(doc, p.color)?;
            let definition = ShapeDefinition::new(p.geometry);
            let path = WorkPath {
                id: PathId::fresh(),
                name: p.name.clone(),
                shape: Some(definition.clone()),
                anchors: definition.anchors()?,
                subpaths: Vec::new(),
                open_contours: Default::default(),
            };
            path.validate()?;
            let path_id = path.id;
            super::generated::mutate(
                doc,
                "layer.fill.solid",
                json!({"name":p.name,"color":p.color}),
            )?
            .expect("registered solid fill");
            let layer_id = doc.active.ok_or(DocumentError::LayerNotFound)?;
            let layer = doc
                .layers
                .iter_mut()
                .find(|l| l.id() == layer_id)
                .ok_or(DocumentError::LayerNotFound)?;
            layer.info.vector_mask = Some(VectorMask::new(path_id, &layer.info, true));
            layer.info.shape_paint = Some(ShapePaint {
                owned_path: Some(path_id),
                ..Default::default()
            });
            doc.paths.push(Arc::new(path));
            (layer_id, path_id, definition, true)
        }
        "shape.update" => {
            let p: EditShape = parse(params)?;
            if p.expected_revision.is_some_and(|r| r != revision) {
                return Err(CommandError::Stale);
            }
            if p.color.is_none() && p.geometry.is_none() {
                return Err(CommandError::Invalid(
                    "supply shape geometry or color".into(),
                ));
            }
            let id = doc.resolve(&p.layer)?;
            super::vector_masks::check_layer(doc, id)?;
            let layer = doc
                .layers
                .iter()
                .position(|l| l.id() == id)
                .ok_or(DocumentError::LayerNotFound)?;
            let info = doc.layers[layer].info();
            let LayerKind::Solid { color } = info.kind else {
                return Err(CommandError::Invalid(
                    "select an editable shape layer".into(),
                ));
            };
            let path_id = info
                .vector_mask
                .as_ref()
                .filter(|_| info.shape_paint.is_some())
                .ok_or_else(|| CommandError::Invalid("shape has no live shape binding".into()))?
                .path;
            let index = super::paths::resolve(doc, &super::paths::PathRef::Id(path_id))?;
            let mut definition = doc.paths[index].shape.clone().ok_or_else(|| {
                CommandError::Invalid(
                    "path was edited as freeform geometry; use path.update to edit its anchors"
                        .into(),
                )
            })?;
            let geometry = p.geometry.unwrap_or(definition.geometry);
            let new_color = p.color.unwrap_or(color);
            validate_color(doc, new_color)?;
            let updated = geometry != definition.geometry;
            if updated {
                super::vector_masks::check_path_edit(doc, path_id)?;
                definition.geometry = geometry;
                let anchors = definition.anchors()?;
                let path = Arc::make_mut(&mut doc.paths[index]);
                path.anchors = anchors;
                path.shape = Some(definition.clone());
                path.validate()?;
            }
            doc.layers[layer].info.kind = LayerKind::Solid { color: new_color };
            (id, path_id, definition, updated || color != new_color)
        }
        _ => return Err(CommandError::Invalid("unknown shape command".into())),
    };
    Ok(json!(ShapeResult {
        layer_id,
        path_id,
        definition,
        changed
    }))
}
