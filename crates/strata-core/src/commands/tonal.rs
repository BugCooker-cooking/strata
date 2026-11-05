use super::*;
use crate::{Document, DocumentError, Layer, LayerKind, document::tile_limit};
use serde_json::json;
use strata_pixels::TiledPixels;

pub(super) fn mutate(
    doc: &mut Document,
    id: &str,
    params: Value,
) -> Result<Option<Value>, CommandError> {
    if id == "layer.add_tonal_adjustment" {
        let p: AddTonal = parse(params)?;
        p.adjustment.validate()?;
        if p.name.len() > 1024 || doc.layers.len() >= crate::MAX_LAYERS {
            return Err(DocumentError::ResourceLimit.into());
        }
        if doc.insertion_parent().is_some_and(|parent| {
            doc.layers.iter().any(|l| {
                l.info.locks.all && (l.id() == parent || doc.is_descendant(parent, l.id()))
            })
        }) {
            return Err(CommandError::Invalid(
                "cannot insert into locked group".into(),
            ));
        }
        let mask = super::local::mask(
            doc,
            if doc.selection().is_some() {
                MaskPreset::FromSelection
            } else {
                MaskPreset::White
            },
        )?;
        let mut layer = Layer::new(
            p.name,
            TiledPixels::with_pool(doc.format, tile_limit(doc.format), doc.memory_pool()),
        );
        layer.info.kind = LayerKind::Adjustment {
            adjustment: p.adjustment,
        };
        layer.info.parent = doc.insertion_parent();
        super::local::attach(&mut layer, mask);
        let id = layer.id();
        let index = doc
            .active
            .and_then(|id| doc.layers.iter().position(|l| l.id() == id))
            .map_or(doc.layers.len(), |i| i + 1);
        doc.layers.insert(index, layer);
        doc.active = Some(id);
        return Ok(Some(json!({"layer_id":id})));
    }
    if id == "adjust.tonal.set" {
        let p: SetTonal = parse(params)?;
        p.adjustment.validate()?;
        let id = doc.resolve(&p.layer)?;
        let layer = doc.layers.iter_mut().find(|l| l.id() == id).unwrap();
        let LayerKind::Adjustment {
            adjustment: current,
        } = &layer.info.kind
        else {
            return Err(CommandError::Invalid(
                "target must be a tonal adjustment layer".into(),
            ));
        };
        if std::mem::discriminant(current) != std::mem::discriminant(&p.adjustment) {
            return Err(CommandError::Invalid(
                "settings must keep the adjustment type; create a separate layer for another type"
                    .into(),
            ));
        }
        layer.info.kind = LayerKind::Adjustment {
            adjustment: p.adjustment,
        };
        return Ok(Some(json!({"layer_id":id})));
    }
    Ok(None)
}
