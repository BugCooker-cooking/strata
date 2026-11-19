//! A named, closed cubic contour in document coordinates. Handle vectors
//! are relative to their anchor, so moving an anchor also moves its handles.
use crate::{
    DocumentError, PathId,
    geometry::{Homography, Point},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub const MAX_PATHS: usize = 64;
pub const MAX_ANCHORS: usize = 128;
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PathAnchor {
    pub position: Point,
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
    #[schemars(length(min = 3, max = 128))]
    pub anchors: Vec<PathAnchor>,
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
        if !(3..=MAX_ANCHORS).contains(&self.anchors.len()) {
            return Err(DocumentError::Invalid(
                "closed path needs 3..128 anchors".into(),
            ));
        }
        for a in &self.anchors {
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
    pub(crate) fn split(&mut self, segment: usize, t: f64) -> Result<(), DocumentError> {
        if self.anchors.len() >= MAX_ANCHORS
            || segment >= self.anchors.len()
            || !(0.0 < t && t < 1.0)
        {
            return Err(DocumentError::Invalid(
                "split requires an existing segment, capacity for one anchor, and 0 < t < 1".into(),
            ));
        }
        let next = (segment + 1) % self.anchors.len();
        let a = &self.anchors[segment];
        let b = &self.anchors[next];
        let lerp = |a: Point, b: Point| [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
        let p = a.position;
        let q = plus(p, a.outgoing);
        let s = b.position;
        let r = plus(s, b.incoming);
        let (pq, qr, rs) = (lerp(p, q), lerp(q, r), lerp(r, s));
        let (left, right) = (lerp(pq, qr), lerp(qr, rs));
        let position = lerp(left, right);
        self.anchors[segment].outgoing = minus(pq, p);
        self.anchors[next].incoming = minus(rs, s);
        self.anchors.insert(
            segment + 1,
            PathAnchor {
                position,
                incoming: minus(left, position),
                outgoing: minus(right, position),
            },
        );
        self.validate()
    }
    pub(crate) fn polygon(&self) -> Result<Vec<Point>, DocumentError> {
        let mut points = vec![self.anchors[0].position];
        for (a, b) in self.anchors.iter().zip(self.anchors.iter().cycle().skip(1)) {
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
        points.pop(); // The polygon rasterizer closes the final edge itself.
        Ok(points)
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
        for a in &mut path.anchors {
            let position = map(a.position)?;
            a.incoming = minus(map(plus(a.position, a.incoming))?, position);
            a.outgoing = minus(map(plus(a.position, a.outgoing))?, position);
            a.position = position;
        }
        path.validate()?;
    }
    Ok(())
}
