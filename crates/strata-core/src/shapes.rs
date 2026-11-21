//! Parametric shape geometry shared by the canvas draft and document commands.
use crate::{
    DocumentError, PathAnchor,
    geometry::{Homography, Point},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ShapeKind {
    Rectangle {
        #[serde(default)]
        #[schemars(range(min = 0.0, max = 1e12))]
        radius: f64,
    },
    Ellipse {},
    Triangle {},
    Polygon {
        #[schemars(range(min = 3, max = 128))]
        sides: u16,
    },
    Line {
        #[schemars(schema_with = "positive_width")]
        weight: f64,
    },
}
fn positive_width(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    schemars::json_schema!({"type":"number","exclusiveMinimum":0,"maximum":1e12})
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ShapeGeometry {
    pub start: Point,
    pub end: Point,
    pub primitive: ShapeKind,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ShapeDefinition {
    pub geometry: ShapeGeometry,
    /// Geometry is entered in its original coordinate space. Whole-image
    /// edits accumulate this affine mapping and keep the parameters editable.
    pub placement: [f64; 9],
}
const IDENTITY: [f64; 9] = [1., 0., 0., 0., 1., 0., 0., 0., 1.];
const K: f64 = 0.552_284_749_830_793_6;
fn invalid(s: &str) -> DocumentError {
    DocumentError::Invalid(s.into())
}
fn corner(position: Point) -> PathAnchor {
    PathAnchor {
        position,
        linked: false,
        incoming: [0.; 2],
        outgoing: [0.; 2],
    }
}
fn curved(position: Point, incoming: Point, outgoing: Point) -> PathAnchor {
    PathAnchor {
        position,
        linked: true,
        incoming,
        outgoing,
    }
}
impl ShapeGeometry {
    pub fn outline(self) -> Result<Vec<PathAnchor>, DocumentError> {
        if self
            .start
            .into_iter()
            .chain(self.end)
            .any(|n| !n.is_finite() || n.abs() > 1e12)
        {
            return Err(invalid(
                "shape endpoints must be finite canvas coordinates within ±1e12",
            ));
        }
        let [x0, y0] = self.start;
        let [x1, y1] = self.end;
        let (x, y, r, b) = (x0.min(x1), y0.min(y1), x0.max(x1), y0.max(y1));
        let (w, h) = (r - x, b - y);
        if let ShapeKind::Line { weight } = self.primitive {
            let length = (x1 - x0).hypot(y1 - y0);
            if length <= 0. || !weight.is_finite() || !(0.0 < weight && weight <= 1e12) {
                return Err(invalid(
                    "line needs distinct endpoints and a positive finite width",
                ));
            }
            let (dx, dy) = (
                (y0 - y1) / length * weight / 2.,
                (x1 - x0) / length * weight / 2.,
            );
            let points = [
                [x0 + dx, y0 + dy],
                [x1 + dx, y1 + dy],
                [x1 - dx, y1 - dy],
                [x0 - dx, y0 - dy],
            ];
            if points.iter().flatten().any(|n| n.abs() > 1e12) {
                return Err(invalid(
                    "line width extends beyond the supported canvas coordinates",
                ));
            }
            return Ok(points.map(corner).to_vec());
        }
        if w == 0. || h == 0. {
            return Err(invalid("shape bounds must have positive width and height"));
        }
        let (rx, ry) = (w / 2., h / 2.);
        let (cx, cy) = (x + rx, y + ry);
        Ok(match self.primitive {
            ShapeKind::Rectangle { radius } => {
                if !radius.is_finite() || !(0.0..=1e12).contains(&radius) {
                    return Err(invalid("corner radius must be finite and nonnegative"));
                }
                let d = radius.min(rx).min(ry);
                if d == 0. {
                    [[x, y], [r, y], [r, b], [x, b]].map(corner).to_vec()
                } else {
                    let k = K * d;
                    let mut out: Vec<PathAnchor> = Vec::new();
                    for a in [
                        curved([x + d, y], [-k, 0.], [0., 0.]),
                        curved([r - d, y], [0., 0.], [k, 0.]),
                        curved([r, y + d], [0., -k], [0., 0.]),
                        curved([r, b - d], [0., 0.], [0., k]),
                        curved([r - d, b], [k, 0.], [0., 0.]),
                        curved([x + d, b], [0., 0.], [-k, 0.]),
                        curved([x, b - d], [0., k], [0., 0.]),
                        curved([x, y + d], [0., 0.], [0., -k]),
                    ] {
                        if let Some(last) = out.last_mut().filter(|p| p.position == a.position) {
                            last.outgoing = a.outgoing;
                        } else {
                            out.push(a);
                        }
                    }
                    out
                }
            }
            ShapeKind::Ellipse {} => vec![
                curved([cx, y], [-K * rx, 0.], [K * rx, 0.]),
                curved([r, cy], [0., -K * ry], [0., K * ry]),
                curved([cx, b], [K * rx, 0.], [-K * rx, 0.]),
                curved([x, cy], [0., K * ry], [0., -K * ry]),
            ],
            ShapeKind::Triangle {} => [[cx, y], [r, b], [x, b]].map(corner).to_vec(),
            ShapeKind::Polygon { sides } => {
                if !(3..=128).contains(&sides) {
                    return Err(invalid("polygon requires 3..128 sides"));
                }
                let mut vertices: Vec<_> = (0..sides)
                    .map(|i| {
                        let a = f64::from(i) * std::f64::consts::TAU / f64::from(sides);
                        [a.sin(), -a.cos()]
                    })
                    .collect();
                for axis in 0..2 {
                    let lo = vertices
                        .iter()
                        .map(|p| p[axis])
                        .fold(f64::INFINITY, f64::min);
                    let hi = vertices
                        .iter()
                        .map(|p| p[axis])
                        .fold(f64::NEG_INFINITY, f64::max);
                    for v in &mut vertices {
                        v[axis] = [x, y][axis] + (v[axis] - lo) / (hi - lo) * [w, h][axis];
                    }
                }
                vertices.into_iter().map(corner).collect()
            }
            ShapeKind::Line { .. } => unreachable!(),
        })
    }
}
impl ShapeDefinition {
    pub(crate) fn new(geometry: ShapeGeometry) -> Self {
        Self {
            geometry,
            placement: IDENTITY,
        }
    }
    pub(crate) fn anchors(&self) -> Result<Vec<PathAnchor>, DocumentError> {
        let mut anchors = self.geometry.outline()?;
        if self.placement != IDENTITY {
            let m = Homography::new(self.placement).map_err(|e| invalid(&e.to_string()))?;
            if m.values()[6] != 0. || m.values()[7] != 0. {
                return Err(invalid("shape parameter placement must be affine"));
            }
            for a in &mut anchors {
                crate::paths::transform_anchor(a, &m)?;
            }
        }
        Ok(anchors)
    }
    pub(crate) fn matches(&self, stored: &[PathAnchor]) -> Result<bool, DocumentError> {
        // Trigonometry for regular polygons may differ by a few ULPs across
        // libm implementations. Keep the stored curve as the display source.
        let expected = self.anchors()?;
        let close = |a: Point, b: Point| {
            a.into_iter()
                .zip(b)
                .all(|(x, y)| (x - y).abs() <= 1e-9 + 8.0 * f64::EPSILON * x.abs().max(y.abs()))
        };
        Ok(expected.len() == stored.len()
            && expected.iter().zip(stored).all(|(a, b)| {
                a.linked == b.linked
                    && close(a.position, b.position)
                    && close(a.incoming, b.incoming)
                    && close(a.outgoing, b.outgoing)
            }))
    }
    pub(crate) fn transform(&mut self, m: &Homography) -> Result<(), DocumentError> {
        self.placement = Homography::new(self.placement)
            .and_then(|p| p.then(m))
            .map_err(|e| invalid(&e.to_string()))?
            .values();
        Ok(())
    }
}

/// A shape uses constant ink through an editable vector mask. The optional
/// layer-local support records destructive crops independently of the path.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema, Default, Hash)]
#[serde(deny_unknown_fields)]
pub struct ShapePaint {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owned_path: Option<crate::PathId>,
    pub clip: Option<crate::GeneratedGeometry>,
}
impl ShapePaint {
    pub(crate) fn bounds(
        &self,
        doc: &crate::Document,
        info: &crate::LayerInfo,
    ) -> Result<Option<strata_pixels::CoverageRect>, DocumentError> {
        use strata_pixels::CoverageRect;
        let clip = self
            .clip
            .as_ref()
            .map(|c| c.bounds([info.x, info.y]))
            .transpose()?;
        let Some(m) = info
            .vector_mask
            .as_ref()
            .filter(|m| m.enabled && !m.inverted)
        else {
            let f = doc.frame();
            return Ok(clip.unwrap_or(Some(CoverageRect {
                x: f.x,
                y: f.y,
                width: f.width,
                height: f.height,
            })));
        };
        let contours = m.contours(doc, info)?;
        let mut lo = [f64::INFINITY; 2];
        let mut hi = [f64::NEG_INFINITY; 2];
        for p in contours.iter().flatten() {
            for k in 0..2 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        let [mut x, mut y] = lo.map(|n| n.floor() as i64);
        let [mut right, mut bottom] = hi.map(|n| n.ceil() as i64);
        if let Some(clip) = clip {
            let Some(c) = clip else { return Ok(None) };
            x = x.max(c.x);
            y = y.max(c.y);
            right = right.min(c.x + i64::from(c.width));
            bottom = bottom.min(c.y + i64::from(c.height));
        }
        if right <= x || bottom <= y {
            return Ok(None);
        }
        Ok(Some(CoverageRect {
            x,
            y,
            width: (right - x)
                .try_into()
                .map_err(|_| DocumentError::ResourceLimit)?,
            height: (bottom - y)
                .try_into()
                .map_err(|_| DocumentError::ResourceLimit)?,
        }))
    }
}
