//! Editable fills sample their original gradient through a local projective
//! mapping. The convex support records destructive crops in the order applied.
use crate::{DocumentError, Frame, LayerInfo, LayerKind, geometry::Homography};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::hash::{Hash, Hasher};
use strata_pixels::CoverageRect;

type Point = [f64; 2];
const MAX_VERTICES: usize = 32;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GeneratedGeometry {
    matrix: [f64; 9],
    // Boundary in current layer-local coordinates, counterclockwise.
    // An empty polygon deliberately has no support.
    support: Vec<Point>,
}

impl Hash for GeneratedGeometry {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.support.len().hash(state);
        for value in self.matrix.iter().chain(self.support.iter().flatten()) {
            (if *value == 0.0 { 0 } else { value.to_bits() }).hash(state);
        }
    }
}

fn invalid(error: impl std::fmt::Display) -> DocumentError {
    DocumentError::Invalid(format!("generated geometry: {error}"))
}

fn rect_points(rect: CoverageRect) -> Vec<Point> {
    let (x, y) = (rect.x as f64, rect.y as f64);
    let (r, b) = (x + f64::from(rect.width), y + f64::from(rect.height));
    vec![[x, y], [r, y], [r, b], [x, b]]
}

fn edges(points: &[Point]) -> impl Iterator<Item = (Point, Point)> + '_ {
    points
        .iter()
        .copied()
        .zip(points.iter().copied().cycle().skip(1))
}

fn cross(a: Point, b: Point, p: Point) -> f64 {
    (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0])
}

fn normalize_support(points: &mut Vec<Point>) {
    points.dedup();
    if points.first() == points.last() {
        points.pop();
    }
    if points.len() < 3 {
        points.clear();
        return;
    }
    let area: f64 = edges(points).map(|(a, b)| cross(a, b, points[0])).sum();
    if area == 0.0 {
        points.clear();
    } else if area < 0.0 {
        points.reverse();
    }
}

fn translation([x, y]: [f64; 2]) -> Result<Homography, DocumentError> {
    Homography::affine([1.0, 0.0, 0.0, 1.0, x, y]).map_err(invalid)
}

impl GeneratedGeometry {
    pub(crate) fn update_reference(
        target: &mut LayerInfo,
        original: &LayerInfo,
        frame: &Frame,
        transform: &Homography,
    ) -> Result<(), DocumentError> {
        if !matches!(
            original.kind,
            LayerKind::Solid { .. } | LayerKind::Gradient { .. }
        ) && original.generated_size.is_none()
            && original.styles.gradient_overlay.is_none()
        {
            return Ok(());
        }
        let frame_size = [frame.width, frame.height];
        target.generated_geometry = Some(Self::warp(
            original,
            frame_size,
            transform,
            [target.x, target.y],
        )?);
        target.generated_size = Some(original.generated_size.unwrap_or(frame_size));
        target.generated_crop = None;
        Ok(())
    }

    pub(crate) fn warp(
        info: &LayerInfo,
        frame_size: [u32; 2],
        transform: &Homography,
        origin: [i64; 2],
    ) -> Result<Self, DocumentError> {
        let local = translation([-(origin[0] as f64), -(origin[1] as f64)])?;
        let relative = translation([info.x as f64, info.y as f64])?
            .then(transform)
            .and_then(|value| value.then(&local))
            .map_err(invalid)?;
        let (mapping, support) = if let Some(current) = &info.generated_geometry {
            (current.mapping()?, current.support.clone())
        } else {
            let [width, height] = info.generated_size.unwrap_or(frame_size);
            let domain = if matches!(
                info.kind,
                LayerKind::Solid { .. } | LayerKind::Gradient { .. }
            ) {
                info.generated_crop.as_ref().map(|crop| crop.bounds)
            } else {
                None
            }
            .unwrap_or(Some(CoverageRect {
                x: 0,
                y: 0,
                width,
                height,
            }));
            (
                translation([0.0, 0.0])?,
                domain.map_or_else(Vec::new, rect_points),
            )
        };
        let mut support = relative.map_polygon(&support).map_err(invalid)?;
        normalize_support(&mut support);
        let matrix = mapping.then(&relative).map_err(invalid)?.values();
        let result = Self { matrix, support };
        result.validate(origin)?;
        Ok(result)
    }

    pub(crate) fn crop(&mut self, bounds: CoverageRect) {
        // Sutherland-Hodgman clipping in the current local plane. Inserting
        // an edge here prevents later transforms resurrecting discarded fill.
        let limits = [
            (0, bounds.x as f64, true),
            (1, bounds.y as f64, true),
            (0, bounds.x as f64 + f64::from(bounds.width), false),
            (1, bounds.y as f64 + f64::from(bounds.height), false),
        ];
        for (axis, limit, is_min) in limits {
            let inside = |p: Point| {
                if is_min {
                    p[axis] >= limit
                } else {
                    p[axis] <= limit
                }
            };
            let mut output = Vec::new();
            for (a, b) in edges(&self.support) {
                if inside(a) != inside(b) {
                    let t = (limit - a[axis]) / (b[axis] - a[axis]);
                    let other = 1 - axis;
                    let mut p = a;
                    p[axis] = limit;
                    p[other] += t * (b[other] - a[other]);
                    output.push(p);
                }
                if inside(b) {
                    output.push(b);
                }
            }
            self.support = output;
        }
        normalize_support(&mut self.support);
    }

    pub(crate) fn bounds(&self, [x, y]: [i64; 2]) -> Result<Option<CoverageRect>, DocumentError> {
        if self.support.is_empty() {
            return Ok(None);
        }
        let [l, t] = self
            .support
            .iter()
            .fold([f64::INFINITY; 2], |a, p| [a[0].min(p[0]), a[1].min(p[1])]);
        let [r, b] = self.support.iter().fold([f64::NEG_INFINITY; 2], |a, p| {
            [a[0].max(p[0]), a[1].max(p[1])]
        });
        let rect = CoverageRect {
            x: x.checked_add(l.floor() as i64)
                .ok_or(DocumentError::ResourceLimit)?,
            y: y.checked_add(t.floor() as i64)
                .ok_or(DocumentError::ResourceLimit)?,
            width: u32::try_from((r.ceil() - l.floor()) as i64)
                .map_err(|_| DocumentError::ResourceLimit)?,
            height: u32::try_from((b.ceil() - t.floor()) as i64)
                .map_err(|_| DocumentError::ResourceLimit)?,
        };
        rect.validate()?;
        Ok(Some(rect))
    }

    pub(crate) fn validate(&self, origin: [i64; 2]) -> Result<(), DocumentError> {
        self.mapping()?;
        if self.support.is_empty() {
            return Ok(());
        }
        if !(3..=MAX_VERTICES).contains(&self.support.len())
            || self
                .support
                .iter()
                .flatten()
                .any(|v| !v.is_finite() || v.abs() > 1e12)
            || edges(&self.support).any(|(a, b)| {
                let tolerance = 1e-9 * (b[0] - a[0]).hypot(b[1] - a[1]);
                a == b || self.support.iter().any(|&p| cross(a, b, p) < -tolerance)
            })
        {
            return Err(invalid("invalid convex fill support"));
        }
        self.bounds(origin)?;
        Ok(())
    }

    pub(crate) fn mapping(&self) -> Result<Homography, DocumentError> {
        Homography::new(self.matrix).map_err(invalid)
    }

    pub(crate) fn contains(&self, p: Point) -> bool {
        !self.support.is_empty() && edges(&self.support).all(|(a, b)| cross(a, b, p) >= 0.0)
    }
}
