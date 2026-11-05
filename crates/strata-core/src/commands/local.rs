use super::*;
use crate::{
    Document, DocumentError, Layer, LayerKind, LayerMaskInfo, document::tile_limit,
    project::mask_tile_limit,
};
use serde_json::json;
use std::collections::BTreeSet;
use strata_pixels::{
    CoverageRect, MaskLayout, TileCoord, TiledMask, TiledPixels, memory::MemoryKind,
};

fn bounds(doc: &Document) -> CoverageRect {
    CoverageRect {
        x: doc.frame.x,
        y: doc.frame.y,
        width: doc.frame.width,
        height: doc.frame.height,
    }
}
fn uniform(doc: &Document, value: u16) -> Result<TiledMask, CommandError> {
    TiledMask::new(
        MaskLayout {
            bounds: bounds(doc),
            default_coverage: value,
            inverted: false,
        },
        mask_tile_limit(),
        doc.memory_pool(),
    )
    .map_err(|e| DocumentError::from(e).into())
}
pub(super) fn mask(doc: &Document, preset: MaskPreset) -> Result<TiledMask, CommandError> {
    match preset {
        MaskPreset::White => uniform(doc, u16::MAX),
        MaskPreset::Black => uniform(doc, 0),
        MaskPreset::FromSelection => doc.selection.clone().ok_or_else(|| {
            CommandError::Invalid(
                "No selection. Use select.marquee/all/subject first, or choose a white/black mask."
                    .into(),
            )
        }),
    }
}
pub(super) fn attach(layer: &mut Layer, mask: TiledMask) {
    layer.info.mask = Some(LayerMaskInfo {
        enabled: true,
        layout: mask.layout(),
        anchor_x: layer.info.x,
        anchor_y: layer.info.y,
    });
    layer.mask = Some(mask);
}
fn operation(a: u16, b: u16, mode: SelectionMode) -> u16 {
    match mode {
        SelectionMode::Replace => b,
        SelectionMode::Add => a.max(b),
        SelectionMode::Subtract => a.saturating_sub(b),
        SelectionMode::Intersect => a.min(b),
    }
}
pub(super) fn combine(
    doc: &Document,
    a: Option<&TiledMask>,
    b: TiledMask,
    mode: SelectionMode,
) -> Result<TiledMask, CommandError> {
    if matches!(mode, SelectionMode::Replace) {
        return Ok(b);
    }
    let empty = uniform(doc, 0)?;
    let a = a.unwrap_or(&empty);
    let base = |m: &TiledMask| {
        let layout = m.layout();
        layout.default_coverage ^ if layout.inverted { 65535 } else { 0 }
    };
    let mut out = uniform(doc, operation(base(a), base(&b), mode))?;
    let coords: BTreeSet<_> = a.tile_coords().chain(b.tile_coords()).copied().collect();
    for coord in coords {
        let _scratch = doc
            .memory_pool()
            .reserve(MemoryKind::Transient, 256 * 256 * 2 * 3)
            .map_err(DocumentError::from)?;
        let [x, y] = coord.origin();
        let left = a.region_tile(x, y).map_err(DocumentError::from)?;
        let right = b.region_tile(x, y).map_err(DocumentError::from)?;
        let values = left
            .iter()
            .zip(right)
            .map(|(a, b)| operation(*a, b, mode))
            .collect();
        out.insert_tile(coord, values)
            .map_err(DocumentError::from)?;
    }
    Ok(out)
}
fn rectangle(doc: &Document, p: Marquee) -> Result<TiledMask, CommandError> {
    let input = CoverageRect {
        x: p.x,
        y: p.y,
        width: p.width,
        height: p.height,
    };
    input.validate().map_err(DocumentError::from)?;
    let frame = bounds(doc);
    let x = input.x.max(frame.x);
    let y = input.y.max(frame.y);
    let right = (input.x + i64::from(input.width)).min(frame.x + i64::from(frame.width));
    let bottom = (input.y + i64::from(input.height)).min(frame.y + i64::from(frame.height));
    let mut result = uniform(doc, 0)?;
    if x >= right || y >= bottom {
        return Ok(result);
    }
    if (x, y, right - x, bottom - y)
        == (
            frame.x,
            frame.y,
            i64::from(frame.width),
            i64::from(frame.height),
        )
    {
        return uniform(doc, 65535);
    }
    let (first, _) = TileCoord::locate(x, y);
    let (last, _) = TileCoord::locate(right - 1, bottom - 1);
    for tx in first.x()..=last.x() {
        for ty in first.y()..=last.y() {
            let _scratch = doc
                .memory_pool()
                .reserve(MemoryKind::Transient, 256 * 256 * 2)
                .map_err(DocumentError::from)?;
            let coord = TileCoord::new(tx, ty).map_err(DocumentError::from)?;
            let [ox, oy] = coord.origin();
            let mut values = vec![0; 256 * 256];
            for py in y.max(oy)..bottom.min(oy.saturating_add(256)) {
                for px in x.max(ox)..right.min(ox.saturating_add(256)) {
                    values[(py - oy) as usize * 256 + (px - ox) as usize] = 65535;
                }
            }
            result
                .insert_tile(coord, values)
                .map_err(DocumentError::from)?;
        }
    }
    Ok(result)
}

pub(super) fn mutate(
    doc: &mut Document,
    id: &str,
    params: Value,
) -> Result<Option<Value>, CommandError> {
    if id.starts_with("select.") {
        match id {
            "select.marquee" => {
                let p: Marquee = parse(params)?;
                let mode = p.mode;
                let next = rectangle(doc, p)?;
                doc.selection = Some(combine(doc, doc.selection.as_ref(), next, mode)?);
            }
            "select.all" => {
                let _: Empty = parse(params)?;
                doc.selection = Some(uniform(doc, 65535)?);
            }
            "select.none" => {
                let _: Empty = parse(params)?;
                doc.selection = None;
            }
            "select.inverse" => {
                let _: Empty = parse(params)?;
                if let Some(selection) = doc.selection.as_mut() {
                    selection.invert()
                } else {
                    doc.selection = Some(uniform(doc, 65535)?);
                }
            }
            _ => return Ok(None),
        }
        return Ok(Some(
            json!({"selection":doc.selection.as_ref().map(TiledMask::layout)}),
        ));
    }
    if id == "layer.add_adjustment" {
        let p: AddAdjustment = parse(params)?;
        if p.name.len() > 1024 || doc.layers.len() >= crate::MAX_LAYERS {
            return Err(DocumentError::ResourceLimit.into());
        }
        if !p.stops.is_finite() || !(-20.0..=20.0).contains(&p.stops) {
            return Err(CommandError::Invalid(
                "exposure stops must be finite in -20..20".into(),
            ));
        }
        let coverage = mask(doc, p.mask)?;
        let mut layer = Layer::new(
            p.name,
            TiledPixels::with_pool(doc.format, tile_limit(doc.format), doc.memory_pool()),
        );
        layer.info.kind = match p.adjustment_type {
            AdjustmentType::Exposure => LayerKind::Exposure { stops: p.stops },
        };
        layer.info.parent = doc.insertion_parent();
        attach(&mut layer, coverage);
        let id = layer.id();
        let index = doc
            .active
            .and_then(|id| doc.layers.iter().position(|l| l.id() == id))
            .map_or(doc.layers.len(), |index| index + 1);
        doc.layers.insert(index, layer);
        doc.active = Some(id);
        return Ok(Some(json!({"layer_id":id})));
    }
    if !matches!(
        id,
        "layer.mask.add"
            | "layer.mask.set_enabled"
            | "layer.mask.invert"
            | "layer.mask.remove"
            | "adjust.exposure.set"
    ) {
        return Ok(None);
    }
    let target: LayerRef = parse(
        params
            .get("layer")
            .cloned()
            .ok_or_else(|| CommandError::Invalid("missing layer".into()))?,
    )?;
    let layer = doc.resolve(&target)?;
    let index = doc
        .layers
        .iter()
        .position(|l| l.id() == layer)
        .ok_or(DocumentError::LayerNotFound)?;
    match id {
        "layer.mask.add" => {
            let p: AddMask = parse(params)?;
            if doc.layers[index].mask.is_some() {
                return Err(CommandError::Invalid(
                    "layer already has a mask; remove it explicitly before replacing".into(),
                ));
            }
            let coverage = mask(doc, p.source)?;
            attach(&mut doc.layers[index], coverage);
        }
        "layer.mask.set_enabled" => {
            let p: EnableMask = parse(params)?;
            doc.layers[index]
                .info
                .mask
                .as_mut()
                .ok_or_else(|| CommandError::Invalid("layer has no mask".into()))?
                .enabled = p.enabled;
        }
        "layer.mask.invert" => {
            let _: Target = parse(params)?;
            let layer = &mut doc.layers[index];
            let mask = layer
                .mask
                .as_mut()
                .ok_or_else(|| CommandError::Invalid("layer has no mask".into()))?;
            mask.invert();
            layer
                .info
                .mask
                .as_mut()
                .expect("validated mask metadata")
                .layout = mask.layout();
        }
        "layer.mask.remove" => {
            let _: Target = parse(params)?;
            doc.layers[index].mask = None;
            doc.layers[index].info.mask = None;
        }
        "adjust.exposure.set" => {
            let p: SetExposure = parse(params)?;
            if !matches!(doc.layers[index].info.kind, LayerKind::Exposure { .. }) {
                return Err(CommandError::Invalid(
                    "target must be an exposure adjustment layer".into(),
                ));
            }
            doc.layers[index].info.kind = LayerKind::Exposure { stops: p.stops };
        }
        _ => unreachable!(),
    }
    Ok(Some(json!({"layer_id":layer})))
}
