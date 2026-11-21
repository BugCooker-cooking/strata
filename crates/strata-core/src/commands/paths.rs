use super::*;
use crate::{Document, DocumentError, PathAnchor, PathId, WorkPath};
use serde_json::json;
use std::{collections::BTreeSet, sync::Arc};

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum PathRef {
    Id(PathId),
    Name(String),
}
params!(CreatePath{
    #[schemars(length(min=1,max=256))] name:String,
    #[schemars(length(min=2,max=1024))] anchors:Vec<PathAnchor>,
    #[serde(default)] #[schemars(schema_with="crate::paths::additional_contours_schema")] subpaths:Vec<Vec<PathAnchor>>,
    #[serde(default)] #[schemars(schema_with="open_indices_schema")] open_contours:Vec<usize>,
    #[serde(default)] expected_revision:Option<u64>
});
params!(UpdatePath{
    path:PathRef,
    #[serde(default)] #[schemars(length(min=1,max=256))] name:Option<String>,
    #[serde(default)] #[schemars(length(min=2,max=1024))] anchors:Option<Vec<PathAnchor>>,
    #[serde(default)] #[schemars(schema_with="optional_subpaths_schema")] subpaths:Option<Vec<Vec<PathAnchor>>>,
    #[serde(default)] #[schemars(schema_with="optional_indices_schema")] open_contours:Option<Vec<usize>>,
    #[serde(default)] expected_revision:Option<u64>
});
fn open_indices_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    schemars::json_schema!({"type":"array","maxItems":64,"uniqueItems":true,"items":{"type":"integer","minimum":0,"maximum":63}})
}
fn optional_indices_schema(g: &mut schemars::SchemaGenerator) -> schemars::Schema {
    schemars::json_schema!({"anyOf":[{"type":"null"},open_indices_schema(g)]})
}
fn open_indices(indices: Vec<usize>) -> Result<BTreeSet<usize>, CommandError> {
    let n = indices.len();
    if n > crate::paths::MAX_CONTOURS {
        return Err(DocumentError::ResourceLimit.into());
    }
    let set: BTreeSet<_> = indices.into_iter().collect();
    if set.len() != n {
        return Err(CommandError::Invalid("duplicate open contour index".into()));
    }
    Ok(set)
}
fn optional_subpaths_schema(g: &mut schemars::SchemaGenerator) -> schemars::Schema {
    schemars::json_schema!({"anyOf":[{"type":"null"},crate::paths::additional_contours_schema(g)]})
}
fn default_threshold() -> u16 {
    32768
}
params!(TracePath{
    #[schemars(length(min=1,max=256))] name:String,
    #[serde(default="default_threshold")] #[schemars(range(min=1,max=65535))] threshold:u16,
    #[serde(default)] #[schemars(range(min=0.0,max=10.0))] tolerance:f64,
    #[serde(default)] expected_revision:Option<u64>
});
fn split_parameter(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    schemars::json_schema!({"type":"number","exclusiveMinimum":0,"exclusiveMaximum":1})
}
params!(SplitPath{path:PathRef,#[serde(default)] #[schemars(range(max=63))] contour:usize,#[schemars(range(max=1023))] segment:usize,#[schemars(schema_with="split_parameter")] t:f64,#[serde(default)] expected_revision:Option<u64>});
params!(PathTarget { path: PathRef });
params!(DeletePath{path:PathRef,#[serde(default)] expected_revision:Option<u64>});
params!(PathSelection {
    path: PathRef,
    #[serde(default)]
    mode: SelectionMode
});
params!(PathSummary {
    id: PathId,
    name: String,
    contour_count: usize,
    anchor_count: usize
});
params!(PathList{paths:Vec<PathSummary>});
params!(PathChange {
    path_id: PathId,
    changed: bool
});

pub(super) fn resolve(doc: &Document, target: &PathRef) -> Result<usize, CommandError> {
    let mut matches = doc.paths.iter().enumerate().filter(|(_, p)| match target {
        PathRef::Id(id) => p.id == *id,
        PathRef::Name(name) => p.name == *name,
    });
    let (index, _) = matches
        .next()
        .ok_or_else(|| CommandError::Invalid("path not found".into()))?;
    if matches.next().is_some() {
        return Err(CommandError::Invalid(
            "ambiguous path name; list paths and use its ID".into(),
        ));
    }
    Ok(index)
}
pub(super) fn list(doc: &Document) -> Value {
    json!(PathList {
        paths: doc
            .paths
            .iter()
            .map(|p| PathSummary {
                id: p.id,
                name: p.name.clone(),
                anchor_count: p.anchor_count(),
                contour_count: 1 + p.subpaths.len()
            })
            .collect()
    })
}
pub(super) fn mutate(
    doc: &mut Document,
    id: &str,
    params: Value,
    revision: u64,
    cancel: &dyn Fn() -> bool,
) -> Result<Value, CommandError> {
    let matches_revision = |expected: Option<u64>| {
        if expected.is_some_and(|v| v != revision) {
            Err(CommandError::Stale)
        } else {
            Ok(())
        }
    };
    let (path_id, changed) = match id {
        "path.create" | "path.from_selection" => {
            if doc.paths.len() >= crate::paths::MAX_PATHS {
                return Err(DocumentError::ResourceLimit.into());
            }
            let (name, anchors, subpaths, open_contours) = if id == "path.create" {
                let p: CreatePath = parse(params)?;
                matches_revision(p.expected_revision)?;
                (
                    p.name,
                    p.anchors,
                    p.subpaths,
                    open_indices(p.open_contours)?,
                )
            } else {
                let p: TracePath = parse(params)?;
                matches_revision(p.expected_revision)?;
                let contours = super::path_trace::trace(doc, p.threshold, p.tolerance, cancel)?;
                let mut contours = contours.into_iter();
                (
                    p.name,
                    contours.next().ok_or_else(|| {
                        CommandError::Invalid("no selection pixels meet the threshold".into())
                    })?,
                    contours.collect(),
                    Default::default(),
                )
            };
            let path = WorkPath {
                id: PathId::fresh(),
                shape: None,
                name,
                anchors,
                subpaths,
                open_contours,
            };
            path.validate()?;
            let id = path.id;
            doc.paths.push(Arc::new(path));
            (id, true)
        }
        "path.update" => {
            let p: UpdatePath = parse(params)?;
            matches_revision(p.expected_revision)?;
            if p.name.is_none()
                && p.anchors.is_none()
                && p.subpaths.is_none()
                && p.open_contours.is_none()
            {
                return Err(CommandError::Invalid(
                    "supply a path name or anchors".into(),
                ));
            }
            let index = resolve(doc, &p.path)?;
            let old = &doc.paths[index];
            let mut path = WorkPath {
                id: old.id,
                shape: old.shape.clone(),
                name: p.name.unwrap_or_else(|| old.name.clone()),
                anchors: p.anchors.unwrap_or_else(|| old.anchors.clone()),
                subpaths: p.subpaths.unwrap_or_else(|| old.subpaths.clone()),
                open_contours: p
                    .open_contours
                    .map(open_indices)
                    .transpose()?
                    .unwrap_or_else(|| old.open_contours.clone()),
            };
            if path.anchors != old.anchors
                || path.subpaths != old.subpaths
                || path.open_contours != old.open_contours
            {
                super::vector_masks::check_path_edit(doc, old.id)?;
                path.shape = None;
            }
            path.validate()?;
            let changed = path != **old;
            let id = path.id;
            if changed {
                doc.paths[index] = Arc::new(path);
            }
            (id, changed)
        }
        "path.split" => {
            let p: SplitPath = parse(params)?;
            matches_revision(p.expected_revision)?;
            let index = resolve(doc, &p.path)?;
            super::vector_masks::check_path_edit(doc, doc.paths[index].id)?;
            let path = Arc::make_mut(&mut doc.paths[index]);
            path.split(p.contour, p.segment, p.t)?;
            (path.id, true)
        }
        "path.delete" => {
            let p: DeletePath = parse(params)?;
            matches_revision(p.expected_revision)?;
            let index = resolve(doc, &p.path)?;
            if doc.layers().iter().any(|l| {
                l.info()
                    .vector_mask
                    .as_ref()
                    .is_some_and(|m| m.path == doc.paths[index].id)
            }) {
                return Err(CommandError::Invalid(
                    "remove vector mask bindings before deleting their source path".into(),
                ));
            }
            (doc.paths.remove(index).id, true)
        }
        _ => return Err(CommandError::Invalid("unknown path edit".into())),
    };
    Ok(json!(PathChange { path_id, changed }))
}

params!(FillPath {
    path: PathRef,
    #[schemars(length(min = 1, max = 256))]
    name: String,
    color: [u16; 4]
});
pub(super) fn coverage(
    doc: &Document,
    target: &PathRef,
    cancel: &dyn Fn() -> bool,
) -> Result<strata_pixels::TiledMask, CommandError> {
    let contours = doc.paths[resolve(doc, target)?].polygons()?;
    super::selection::compound_polygon(doc, &contours, cancel)
}
pub(super) fn fill(
    doc: &mut Document,
    params: Value,
    cancel: &dyn Fn() -> bool,
) -> Result<Value, CommandError> {
    let p: FillPath = parse(params)?;
    if p.name.trim().is_empty()
        || p.name.len() > 256
        || p.name.chars().any(char::is_control)
        || (doc.bit_depth() == 8 && p.color.iter().any(|&c| c > 255))
    {
        return Err(CommandError::Invalid(
            "invalid path fill name or working RGBA samples".into(),
        ));
    }
    if doc.insertion_parent().is_some_and(|parent| {
        doc.layers()
            .iter()
            .any(|l| l.info().locks.all && (l.id() == parent || doc.is_descendant(parent, l.id())))
    }) {
        return Err(CommandError::Invalid(
            "cannot insert into a locked parent group".into(),
        ));
    }
    let mask = coverage(doc, &p.path, cancel)?;
    let result = super::generated::mutate(
        doc,
        "layer.fill.solid",
        json!({"name":p.name,"color":p.color}),
    )?
    .expect("registered solid fill factory");
    let id = doc.active.ok_or(CommandError::NoDocument)?;
    let layer = doc
        .layers
        .iter_mut()
        .find(|l| l.id() == id)
        .ok_or(DocumentError::LayerNotFound)?;
    super::local::attach(layer, mask);
    Ok(result)
}
