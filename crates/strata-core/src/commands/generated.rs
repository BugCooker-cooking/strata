use super::*;
use crate::{Document, DocumentError, Layer, LayerKind, document::tile_limit};
use serde_json::json;
use strata_pixels::TiledPixels;
pub(super) fn mutate(
    doc: &mut Document,
    id: &str,
    params: Value,
) -> Result<Option<Value>, CommandError> {
    let layer = if id == "layer.fill.solid" {
        let p: SolidFill = parse(params.clone())?;
        Some((p.name, LayerKind::Solid { color: p.color }))
    } else if id == "layer.fill.gradient" {
        let p: GradientFill = parse(params.clone())?;
        Some((
            p.name,
            LayerKind::Gradient {
                start: p.start,
                end: p.end,
                angle: p.angle,
                scale: p.scale,
                radial: p.radial,
            },
        ))
    } else if id == "layer.add_hue_saturation" {
        let p: CreateGroup = parse(params.clone())?;
        Some((
            p.name,
            LayerKind::HueSaturation {
                hue: 0.0,
                saturation: 0.0,
                lightness: 0.0,
            },
        ))
    } else {
        None
    };
    if let Some((name, kind)) = layer {
        if name.len() > 1024 || doc.layers.len() >= crate::MAX_LAYERS {
            return Err(DocumentError::ResourceLimit.into());
        }
        let mut layer = Layer::new(
            name,
            TiledPixels::with_pool(doc.format, tile_limit(doc.format), doc.memory_pool()),
        );
        layer.info.kind = kind;
        if matches!(
            layer.info.kind,
            LayerKind::Solid { .. } | LayerKind::Gradient { .. }
        ) {
            layer.info.generated_size = Some([doc.frame.width, doc.frame.height]);
        }
        layer.info.parent = doc.insertion_parent();
        layer.info.x = doc.frame.x;
        layer.info.y = doc.frame.y;
        if matches!(layer.info.kind, LayerKind::HueSaturation { .. }) {
            let mask = super::local::mask(
                doc,
                if doc.selection.is_some() {
                    MaskPreset::FromSelection
                } else {
                    MaskPreset::White
                },
            )?;
            super::local::attach(&mut layer, mask);
        }
        let id = layer.id();
        let index = doc
            .active
            .and_then(|id| doc.layers.iter().position(|l| l.id() == id))
            .map_or(doc.layers.len(), |i| i + 1);
        doc.layers.insert(index, layer);
        doc.active = Some(id);
        return Ok(Some(json!({"layer_id":id})));
    }
    if !matches!(
        id,
        "layer.fill.solid.set" | "layer.fill.gradient.set" | "adjust.hue_saturation.set"
    ) {
        return Ok(None);
    }
    let reference: LayerRef = parse(
        params
            .get("layer")
            .cloned()
            .ok_or_else(|| CommandError::Invalid("missing layer".into()))?,
    )?;
    let id_layer = doc.resolve(&reference)?;
    let layer = doc.layers.iter_mut().find(|l| l.id() == id_layer).unwrap();
    match id {
        "layer.fill.solid.set" => {
            let p: SetSolidFill = parse(params)?;
            if !matches!(layer.info.kind, LayerKind::Solid { .. }) {
                return Err(CommandError::Invalid(
                    "target must be a solid fill layer".into(),
                ));
            }
            layer.info.kind = LayerKind::Solid { color: p.color };
        }
        "layer.fill.gradient.set" => {
            let p: SetGradientFill = parse(params)?;
            if !matches!(layer.info.kind, LayerKind::Gradient { .. }) {
                return Err(CommandError::Invalid(
                    "target must be a gradient fill layer".into(),
                ));
            }
            layer.info.kind = LayerKind::Gradient {
                start: p.start,
                end: p.end,
                angle: p.angle,
                scale: p.scale,
                radial: p.radial,
            };
        }
        _ => {
            let p: HueSaturationParams = parse(params)?;
            if !matches!(layer.info.kind, LayerKind::HueSaturation { .. }) {
                return Err(CommandError::Invalid(
                    "target must be a hue/saturation adjustment layer".into(),
                ));
            }
            layer.info.kind = LayerKind::HueSaturation {
                hue: p.hue,
                saturation: p.saturation,
                lightness: p.lightness,
            };
        }
    }
    Ok(Some(json!({"layer_id":id_layer})))
}
