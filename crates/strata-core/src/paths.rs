//! A named cubic work path in document coordinates. Handle vectors
//! are relative to their anchor, so moving an anchor also moves its handles.
use crate::{
    DocumentError, PathId,
    geometry::{Homography, Point},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub const MAX_PATHS: usize = 64;
pub const MAX_ANCHORS: usize = 1024;
pub const MAX_CONTOURS: usize = 64;
pub const MAX_PATH_ANCHORS: usize = 2048;
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PathAnchor {
    pub position: Point,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub linked: bool,
    #[serde(default)]
    pub incoming: Point,
    #[serde(default)]
    pub outgoing: Point,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorkPath {
    pub id: PathId,
    #[schemars(length(min = 1, max = 256))]
    pub name: String,
    #[schemars(length(min = 2, max = 1024))]
    pub anchors: Vec<PathAnchor>,
    /// Additional closed contours share the even-odd fill rule with anchors.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[schemars(schema_with = "additional_contours_schema")]
    pub subpaths: Vec<Vec<PathAnchor>>,
    /// Indices of contours with distinct start and end points.
    #[serde(default, skip_serializing_if = "std::collections::BTreeSet::is_empty")]
    #[schemars(length(max = 64))]
    pub open_contours: std::collections::BTreeSet<usize>,
}
pub(crate) fn additional_contours_schema(
    generator: &mut schemars::SchemaGenerator,
) -> schemars::Schema {
    let anchor = generator.subschema_for::<PathAnchor>();
    schemars::json_schema!({"type":"array","maxItems":63,"items":{"type":"array","minItems":2,"maxItems":1024,"items":anchor}})
}
fn plus(a: Point, b: Point) -> Point {
    [a[0] + b[0], a[1] + b[1]]
}
fn minus(a: Point, b: Point) -> Point {
    [a[0] - b[0], a[1] - b[1]]
}
fn mid(a: Point, b: Point) -> Point {
    [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5]
}
impl WorkPath {
    pub(crate) fn contours(&self) -> impl Iterator<Item = &Vec<PathAnchor>> {
        std::iter::once(&self.anchors).chain(&self.subpaths)
    }
    fn contours_mut(&mut self) -> impl Iterator<Item = &mut Vec<PathAnchor>> {
        std::iter::once(&mut self.anchors).chain(&mut self.subpaths)
    }
    pub(crate) fn requires_v13(&self) -> bool {
        !self.open_contours.is_empty()
            || self
                .contours()
                .any(|c| c.len() == 2 || c.iter().any(|a| a.linked))
    }
    pub(crate) fn anchor_count(&self) -> usize {
        self.contours().map(Vec::len).sum()
    }
    pub(crate) fn validate(&self) -> Result<(), DocumentError> {
        if !self.id.is_valid()
            || self.name.trim().is_empty()
            || self.name.len() > 256
            || self.name.chars().any(char::is_control)
        {
            return Err(DocumentError::Invalid(
                "path requires a valid ID and a name within 256 UTF-8 bytes".into(),
            ));
        }
        if self.open_contours.iter().any(|&i| i > self.subpaths.len()) {
            return Err(DocumentError::Invalid(
                "open contour index does not exist".into(),
            ));
        }
        if self.subpaths.len() >= MAX_CONTOURS || self.anchor_count() > MAX_PATH_ANCHORS {
            return Err(DocumentError::ResourceLimit);
        }
        if self
            .contours()
            .any(|c| !(2..=MAX_ANCHORS).contains(&c.len()))
        {
            return Err(DocumentError::Invalid(
                "contour needs 2..1024 anchors".into(),
            ));
        }
        for a in self.contours().flatten() {
            if [
                a.position,
                plus(a.position, a.incoming),
                plus(a.position, a.outgoing),
            ]
            .into_iter()
            .flatten()
            .any(|v| !v.is_finite() || v.abs() > 1e12)
            {
                return Err(DocumentError::Invalid(
                    "path coordinates must be finite and within ±1e12 canvas pixels".into(),
                ));
            }
        }
        Ok(())
    }
    /// Split one cubic with de Casteljau. Both resulting cubics trace exactly
    /// the old contour, including the last segment wrapping back to anchor 0.
    pub(crate) fn split(
        &mut self,
        contour: usize,
        segment: usize,
        t: f64,
    ) -> Result<(), DocumentError> {
        if self.anchor_count() >= MAX_PATH_ANCHORS {
            return Err(DocumentError::ResourceLimit);
        }
        let open = self.open_contours.contains(&contour);
        let anchors = self
            .contours_mut()
            .nth(contour)
            .ok_or_else(|| DocumentError::Invalid("contour index does not exist".into()))?;
        if anchors.len() >= MAX_ANCHORS
            || segment >= anchors.len() - usize::from(open)
            || !(0.0 < t && t < 1.0)
        {
            return Err(DocumentError::Invalid(
                "split requires an existing segment, capacity for one anchor, and 0 < t < 1".into(),
            ));
        }
        let next = (segment + 1) % anchors.len();
        let a = &anchors[segment];
        let b = &anchors[next];
        let lerp = |a: Point, b: Point| [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
        let p = a.position;
        let q = plus(p, a.outgoing);
        let s = b.position;
        let r = plus(s, b.incoming);
        let (pq, qr, rs) = (lerp(p, q), lerp(q, r), lerp(r, s));
        let (left, right) = (lerp(pq, qr), lerp(qr, rs));
        let position = lerp(left, right);
        anchors[segment].outgoing = minus(pq, p);
        anchors[next].incoming = minus(rs, s);
        anchors.insert(
            segment + 1,
            PathAnchor {
                linked: true,
                position,
                incoming: minus(left, position),
                outgoing: minus(right, position),
            },
        );
        self.validate()
    }
    pub(crate) fn polygons(&self) -> Result<Vec<Vec<Point>>, DocumentError> {
        let mut contours = Vec::new();
        let mut total = 0;
        for (index, anchors) in self.contours().enumerate() {
            let open = self.open_contours.contains(&index);
            let mut points = vec![anchors[0].position];
            for (a, b) in anchors
                .iter()
                .zip(anchors.iter().cycle().skip(1))
                .take(anchors.len() - usize::from(open))
            {
                flatten(
                    [
                        a.position,
                        plus(a.position, a.outgoing),
                        plus(b.position, b.incoming),
                        b.position,
                    ],
                    0,
                    &mut points,
                )?;
            }
            if !open {
                points.pop();
            }
            total += points.len();
            if total > 4096 {
                return Err(DocumentError::ResourceLimit);
            }
            contours.push(points);
        }
        Ok(contours)
    }
}
/// Bounded de Casteljau subdivision. Distance to the finite chord also handles
/// collinear curves whose control points overshoot an endpoint.
fn flatten(
    [a, b, c, d]: [Point; 4],
    depth: u8,
    points: &mut Vec<Point>,
) -> Result<(), DocumentError> {
    let v = minus(d, a);
    let len = v[0] * v[0] + v[1] * v[1];
    let flat = |p: Point| {
        let q = minus(p, a);
        let t = if len == 0.0 {
            0.0
        } else {
            ((q[0] * v[0] + q[1] * v[1]) / len).clamp(0.0, 1.0)
        };
        (q[0] - t * v[0]).hypot(q[1] - t * v[1]) <= 0.125
    };
    if flat(b) && flat(c) {
        if points.len() >= 4097 {
            return Err(DocumentError::ResourceLimit);
        }
        points.push(d);
        return Ok(());
    }
    if depth >= 24 {
        return Err(DocumentError::ResourceLimit);
    }
    let (ab, bc, cd) = (mid(a, b), mid(b, c), mid(c, d));
    let (left, right) = (mid(ab, bc), mid(bc, cd));
    let center = mid(left, right);
    flatten([a, ab, left, center], depth + 1, points)?;
    flatten([center, right, cd, d], depth + 1, points)
}
pub(crate) fn transform_paths(
    paths: &mut [Arc<WorkPath>],
    matrix: &Homography,
) -> Result<(), DocumentError> {
    let map = |p| {
        matrix
            .map(p)
            .map_err(|e| DocumentError::Invalid(e.to_string()))
    };
    for path in paths {
        let path = Arc::make_mut(path);
        for a in path.contours_mut().flatten() {
            let position = map(a.position)?;
            a.incoming = minus(map(plus(a.position, a.incoming))?, position);
            a.outgoing = minus(map(plus(a.position, a.outgoing))?, position);
            a.position = position;
        }
        path.validate()?;
    }
    Ok(())
}
