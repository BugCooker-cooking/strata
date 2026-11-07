use super::*;
use crate::{
    Document, DocumentError, Layer, LayerKind,
    typography::{TextDefinition, raster},
};
use serde_json::json;
pub(super) fn fonts() -> Value {
    let fonts = strata_text::font_catalog()
        .into_iter()
        .zip([
            strata_text::FontFamily::Sans,
            strata_text::FontFamily::Serif,
            strata_text::FontFamily::Cjk,
        ])
        .map(|((family, sha), id)| FontDescriptor {
            id,
            family: family.into(),
            bundled: true,
            license: "SIL OFL1.1".into(),
            version: strata_text::VERSION,
            sha256: sha.into(),
        })
        .collect();
    serde_json::to_value(FontsResult {
        fonts,
        system_fonts_exposed: false,
    })
    .unwrap()
}
pub(super) fn inspect(doc: &Document, layer: &LayerRef) -> Result<Value, CommandError> {
    let id = doc.resolve(layer)?;
    if !doc.effective_visibility(id) {
        return Err(CommandError::Invalid(
            "inspect text requires a visible layer and visible ancestors".into(),
        ));
    }
    let l = doc.layers().iter().find(|l| l.id() == id).unwrap();
    let LayerKind::Text { text } = &l.info().kind else {
        return Err(CommandError::Invalid(
            "target must be an editable text layer".into(),
        ));
    };
    Ok(serde_json::to_value(TextResult {
        layer_id: id,
        changed: false,
        definition: text.clone(),
        metrics: None,
    })
    .unwrap())
}
fn color(doc: &Document, value: [f64; 4]) -> Result<[u16; 4], CommandError> {
    if value
        .iter()
        .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
    {
        return Err(CommandError::Invalid(
            "text color needs normalized straight working RGBA0..1".into(),
        ));
    }
    let max = if doc.bit_depth() == 8 { 255. } else { 65535. };
    Ok(value.map(|v| (v * max).round() as u16))
}
pub(super) fn mutate(
    doc: &mut Document,
    id: &str,
    params: Value,
    cancel: &dyn Fn() -> bool,
) -> Result<Option<Value>, CommandError> {
    if !matches!(id, "type.create" | "type.update") {
        return Ok(None);
    }
    if cancel() {
        return Err(CommandError::Expired);
    }
    if id == "type.create" {
        let p: CreateText = parse(params)?;
        if p.name.len() > 1024
            || doc.layers.len() >= crate::MAX_LAYERS
            || p.x.unsigned_abs() > 1_000_000_000_000
            || p.y.unsigned_abs() > 1_000_000_000_000
        {
            return Err(DocumentError::ResourceLimit.into());
        }
        let parent = doc.insertion_parent();
        if let Some(parent) = parent
            && doc.layers.iter().any(|l| {
                (l.id() == parent || doc.is_descendant(parent, l.id()))
                    && (l.info().locks.all || l.info().locks.pixels)
            })
        {
            return Err(CommandError::Invalid(
                "parent is locked against creating text".into(),
            ));
        }
        let definition = TextDefinition {
            version: strata_text::VERSION,
            spec: p.spec.into(),
            color: color(doc, p.color)?,
        };
        let (pixels, metrics) = raster(&definition, doc.format, None, doc.memory_pool(), cancel)?;
        let mut layer = Layer::new(p.name, pixels);
        layer.info.kind = LayerKind::Text {
            text: definition.clone(),
        };
        layer.info.x = p.x;
        layer.info.y = p.y;
        layer.info.parent = parent;
        let target = layer.id();
        let at = doc
            .active
            .and_then(|active| doc.layers.iter().position(|l| l.id() == active))
            .map_or(doc.layers.len(), |i| i + 1);
        doc.layers.insert(at, layer);
        doc.active = Some(target);
        return Ok(Some(
            serde_json::to_value(TextResult {
                layer_id: target,
                changed: true,
                definition,
                metrics: Some(metrics),
            })
            .unwrap(),
        ));
    }
    let p: UpdateText = parse(params)?;
    let target = doc.resolve(&p.layer)?;
    if doc.layers.iter().any(|l| {
        (l.id() == target || doc.is_descendant(target, l.id()))
            && (l.info().locks.all || l.info().locks.pixels || l.info().locks.transparency)
    }) {
        return Err(CommandError::Invalid(
            "unlock text pixel/transparency/all and ancestor locks before changing type".into(),
        ));
    }
    let index = doc.layers.iter().position(|l| l.id() == target).unwrap();
    let LayerKind::Text { text: old } = &doc.layers[index].info.kind else {
        return Err(CommandError::Invalid(
            "target must be an editable text layer".into(),
        ));
    };
    let definition = TextDefinition {
        version: strata_text::VERSION,
        spec: p.spec.into(),
        color: color(doc, p.color)?,
    };
    definition.validate(doc.format)?;
    if *old == definition {
        return Ok(Some(
            json!({"layer_id":target,"changed":false,"definition":definition,"metrics":null}),
        ));
    }
    let (pixels, metrics) = raster(
        &definition,
        doc.format,
        doc.layers[index].info.generated_crop.as_ref(),
        doc.memory_pool(),
        cancel,
    )?;
    if cancel() {
        return Err(CommandError::Expired);
    }
    doc.layers[index].info.kind = LayerKind::Text {
        text: definition.clone(),
    };
    doc.layers[index].pixels = pixels;
    Ok(Some(
        serde_json::to_value(TextResult {
            layer_id: target,
            changed: true,
            definition,
            metrics: Some(metrics),
        })
        .unwrap(),
    ))
}
