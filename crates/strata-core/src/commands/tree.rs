use super::*;
use crate::{Document, DocumentError, Layer, LayerKind, document::tile_limit};
use serde_json::json;
use std::collections::{HashMap, HashSet};
use strata_pixels::TiledPixels;

pub(super) fn mutate(
    doc: &mut Document,
    id: &str,
    params: Value,
) -> Result<Option<Value>, CommandError> {
    if id == "layer.create_group" {
        if doc.insertion_parent().is_some_and(|parent| {
            doc.layers
                .iter()
                .any(|l| l.id() == parent && l.info.locks.all)
        }) {
            return Err(CommandError::Invalid(
                "cannot insert into a locked group".into(),
            ));
        }
        let p: CreateGroup = parse(params)?;
        if p.name.len() > 1024 || doc.layers.len() >= crate::MAX_LAYERS {
            return Err(DocumentError::ResourceLimit.into());
        }
        let mut group = Layer::new(
            p.name,
            TiledPixels::with_pool(doc.format, tile_limit(doc.format), doc.memory_pool()),
        );
        group.info.kind = LayerKind::Group;
        group.info.pass_through = true;
        group.info.parent = doc.insertion_parent();
        let id = group.id();
        doc.layers.push(group);
        doc.active = Some(id);
        return Ok(Some(json!({"layer_id":id})));
    }
    if id == "layer.group" {
        let p: GroupLayers = parse(params)?;
        if p.layers.is_empty()
            || p.layers.len() > crate::MAX_LAYERS
            || p.name.len() > 1024
            || doc.layers.len() >= crate::MAX_LAYERS
        {
            return Err(CommandError::Invalid(
                "group requires 1..128 sibling layers and a bounded name".into(),
            ));
        }
        let ids = p
            .layers
            .iter()
            .map(|r| doc.resolve(r))
            .collect::<Result<HashSet<_>, _>>()?;
        if ids.len() != p.layers.len() {
            return Err(CommandError::Invalid("duplicate group target".into()));
        }
        let selected: Vec<_> = doc
            .layers
            .iter()
            .filter(|l| ids.contains(&l.id()))
            .collect();
        let parent = selected[0].info.parent;
        if parent.is_some_and(|parent| {
            doc.layers.iter().any(|l| {
                l.info.locks.all && (l.id() == parent || doc.is_descendant(parent, l.id()))
            })
        }) {
            return Err(CommandError::Invalid("parent group is locked".into()));
        }
        if selected
            .iter()
            .any(|l| l.info.parent != parent || l.info.locks.all)
        {
            return Err(CommandError::Invalid(
                "group targets must be unlocked siblings".into(),
            ));
        }
        let siblings: Vec<_> = doc
            .layers
            .iter()
            .filter(|l| l.info.parent == parent)
            .map(Layer::id)
            .collect();
        let first = siblings.iter().position(|id| ids.contains(id)).unwrap();
        let last = siblings.iter().rposition(|id| ids.contains(id)).unwrap();
        if last - first + 1 != ids.len() {
            return Err(CommandError::Invalid(
                "group targets must form a contiguous sibling range".into(),
            ));
        }
        let insertion = doc
            .layers
            .iter()
            .rposition(|l| ids.contains(&l.id()))
            .unwrap()
            + 1;
        let mut group = Layer::new(
            p.name,
            TiledPixels::with_pool(doc.format, tile_limit(doc.format), doc.memory_pool()),
        );
        group.info.kind = LayerKind::Group;
        group.info.pass_through = true;
        group.info.parent = parent;
        let group_id = group.id();
        for l in &mut doc.layers {
            if ids.contains(&l.id()) {
                l.info.parent = Some(group_id)
            }
        }
        doc.layers.insert(insertion, group);
        doc.active = Some(group_id);
        return Ok(Some(json!({"layer_id":group_id})));
    }
    if !matches!(id, "layer.ungroup" | "layer.reparent") {
        return Ok(None);
    }
    if id == "layer.reparent" {
        let p: Reparent = parse(params)?;
        let target = doc.resolve(&p.layer)?;
        let parent = p.parent.as_ref().map(|r| doc.resolve(r)).transpose()?;
        if parent == Some(target) || parent.is_some_and(|parent| doc.is_descendant(parent, target))
        {
            return Err(CommandError::Invalid(
                "cannot parent a layer into itself or its descendants".into(),
            ));
        }
        if let Some(parent) = parent {
            let group = doc
                .layers
                .iter()
                .find(|l| l.id() == parent)
                .ok_or(DocumentError::LayerNotFound)?;
            if !matches!(group.info.kind, LayerKind::Group) || group.info.locks.all {
                return Err(CommandError::Invalid(
                    "parent must be an unlocked group".into(),
                ));
            }
        }
        let at = doc.layers.iter().position(|l| l.id() == target).unwrap();
        let mut layer = doc.layers.remove(at);
        layer.info.parent = parent;
        let siblings: Vec<_> = doc
            .layers
            .iter()
            .enumerate()
            .filter(|(_, l)| l.info.parent == parent)
            .map(|(i, _)| i)
            .collect();
        let index = p.index.unwrap_or(siblings.len());
        if index > siblings.len() {
            return Err(CommandError::Invalid("sibling index out of range".into()));
        }
        let insertion = if index == siblings.len() {
            siblings.last().map_or(doc.layers.len(), |i| i + 1)
        } else {
            siblings[index]
        };
        doc.layers.insert(insertion, layer);
        doc.active = Some(target);
        return Ok(Some(json!({"layer_id":target})));
    }
    let p: Target = parse(params)?;
    let target = doc.resolve(&p.layer)?;
    let group = doc.layers.iter().find(|l| l.id() == target).unwrap();
    if !matches!(group.info.kind, LayerKind::Group) {
        return Err(CommandError::Invalid("target must be a group".into()));
    }
    if group.info.opacity != 1.0
        || group.info.fill_opacity != 1.0
        || group.info.blend_mode != crate::BlendMode::Normal
        || group.mask.is_some()
        || !group.info.styles.is_empty()
        || !group.info.visible
    {
        return Err(CommandError::Invalid("ungroup would change group compositing; reset group opacity, visibility, blend and mask or rasterize it first".into()));
    }
    let parent = group.info.parent;
    let before = if group.info.pass_through {
        None
    } else {
        Some(doc.export_pixels()?)
    };
    let mut members = Vec::new();
    let mut rest = Vec::new();
    for mut l in doc.layers.drain(..) {
        if l.info.parent == Some(target) {
            l.info.parent = parent;
            members.push(l)
        } else {
            rest.push(l)
        }
    }
    let insertion = rest.iter().position(|l| l.id() == target).unwrap();
    rest.remove(insertion);
    let active = members.last().map(Layer::id);
    rest.splice(insertion..insertion, members);
    doc.layers = rest;
    doc.active = active;
    if let Some(before) = before {
        let after = doc.export_pixels()?;
        let equal = match (before, after) {
            (strata_pixels::PixelBuffer::Rgba8(a), strata_pixels::PixelBuffer::Rgba8(b)) => {
                a.iter().zip(b).all(|(a, b)| a.abs_diff(b) <= 1)
            }
            (strata_pixels::PixelBuffer::Rgba16(a), strata_pixels::PixelBuffer::Rgba16(b)) => {
                a.iter().zip(b).all(|(a, b)| a.abs_diff(b) <= 3)
            }
            _ => false,
        };
        if !equal {
            return Err(CommandError::Invalid("ungroup would alter isolated child blending; use pass-through first or rasterize the group".into()));
        }
    }
    Ok(Some(json!({"layer_id":target})))
}
pub(super) fn delete(doc: &mut Document, target: crate::LayerId) -> Result<Value, CommandError> {
    let ids: HashSet<_> = doc
        .layers
        .iter()
        .filter(|l| l.id() == target || doc.is_descendant(l.id(), target))
        .map(Layer::id)
        .collect();
    if doc
        .layers
        .iter()
        .any(|l| ids.contains(&l.id()) && l.info.locks.all)
    {
        return Err(CommandError::Invalid(
            "subtree contains fully locked layers".into(),
        ));
    }
    doc.layers.retain(|l| !ids.contains(&l.id()));
    if doc.active.is_some_and(|id| ids.contains(&id)) {
        doc.active = doc.layers.last().map(Layer::id)
    }
    Ok(json!({"layer_id":target}))
}
pub(super) fn duplicate(doc: &mut Document, target: crate::LayerId) -> Result<Value, CommandError> {
    let copies: Vec<_> = doc
        .layers
        .iter()
        .filter(|l| l.id() == target || doc.is_descendant(l.id(), target))
        .cloned()
        .collect();
    if doc.layers.len() + copies.len() > crate::MAX_LAYERS {
        return Err(DocumentError::ResourceLimit.into());
    }
    let map: HashMap<_, _> = copies
        .iter()
        .map(|l| (l.id(), crate::LayerId::fresh()))
        .collect();
    let new = map[&target];
    let insertion = doc.layers.iter().rposition(|l| l.id() == target).unwrap() + 1;
    let copies = copies
        .into_iter()
        .map(|mut l| {
            let root = l.id() == target;
            l.info.id = map[&l.id()];
            if let Some(parent) = l.info.parent {
                l.info.parent = Some(*map.get(&parent).unwrap_or(&parent));
            }
            if root {
                l.info.name = format!("{} copy", l.info.name)
            }
            l
        })
        .collect::<Vec<_>>();
    doc.layers.splice(insertion..insertion, copies);
    doc.active = Some(new);
    Ok(json!({"layer_id":new}))
}
pub(super) fn move_position(
    doc: &mut Document,
    target: crate::LayerId,
    x: i64,
    y: i64,
) -> Result<Value, CommandError> {
    let root = doc.layers.iter().find(|l| l.id() == target).unwrap();
    let dx = i128::from(x) - i128::from(root.info.x);
    let dy = i128::from(y) - i128::from(root.info.y);
    let ids: HashSet<_> = doc
        .layers
        .iter()
        .filter(|l| l.id() == target || doc.is_descendant(l.id(), target))
        .map(Layer::id)
        .collect();
    if doc
        .layers
        .iter()
        .any(|l| ids.contains(&l.id()) && (l.info.locks.all || l.info.locks.position))
    {
        return Err(CommandError::Invalid(
            "subtree contains position-locked layers".into(),
        ));
    }
    for layer in &mut doc.layers {
        if ids.contains(&layer.id()) {
            layer.info.x = i64::try_from(i128::from(layer.info.x) + dx)
                .map_err(|_| CommandError::Invalid("subtree position overflow".into()))?;
            layer.info.y = i64::try_from(i128::from(layer.info.y) + dy)
                .map_err(|_| CommandError::Invalid("subtree position overflow".into()))?;
        }
    }
    Ok(json!({"layer_id":target}))
}
