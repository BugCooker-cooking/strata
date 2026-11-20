//! Trace thresholded selection pixels along their grid edges. Directed edges
//! keep the selected area on the right. At diagonal contacts a right turn
//! keeps four-connected components separate.
use super::CommandError;
use crate::{
    Document, DocumentError, PathAnchor,
    geometry::Point,
    paths::{MAX_ANCHORS, MAX_CONTOURS, MAX_PATH_ANCHORS},
};
use strata_pixels::{CoverageRect, TileCoord, TiledMask, memory::MemoryKind};

const MAX_CORNERS: usize = 65_536;
const MAX_DISTANCE_CHECKS: usize = 16_000_000;
fn check(cancel: &dyn Fn() -> bool) -> Result<(), CommandError> {
    if cancel() {
        Err(CommandError::Expired)
    } else {
        Ok(())
    }
}
pub(super) fn trace(
    doc: &Document,
    threshold: u16,
    tolerance: f64,
    cancel: &dyn Fn() -> bool,
) -> Result<Vec<Vec<PathAnchor>>, CommandError> {
    if threshold == 0 || !(0.0..=10.0).contains(&tolerance) {
        return Err(CommandError::Invalid(
            "threshold must be 1..65535 and tolerance 0..10 canvas pixels".into(),
        ));
    }
    let mask = doc.selection().ok_or_else(|| {
        CommandError::Invalid("create a selection before tracing a work path".into())
    })?;
    let layout = mask.layout();
    let baseline = layout.default_coverage ^ if layout.inverted { u16::MAX } else { 0 };
    let inside = baseline >= threshold;
    let Some(bounds) = trace_bounds(mask, inside)? else {
        return Ok(Vec::new());
    };
    if mask.tile_count() == 0 {
        let (x, y) = (bounds.x as f64, bounds.y as f64);
        let (right, bottom) = (x + f64::from(bounds.width), y + f64::from(bounds.height));
        return Ok(vec![
            [[x, y], [right, y], [right, bottom], [x, bottom]]
                .map(anchor)
                .to_vec(),
        ]);
    }
    let (width, height) = (bounds.width as usize, bounds.height as usize);
    let size = (width + 1)
        .checked_mul(height + 1)
        .ok_or(DocumentError::ResourceLimit)?;
    let area = width * height;
    let _scratch = doc
        .memory_pool()
        .reserve(
            MemoryKind::Transient,
            (area + size) as u64 + size.min(MAX_CORNERS) as u64 * 96 + 128 * 1024,
        )
        .map_err(DocumentError::from)?;
    let mut selected = vec![u8::from(inside); area];
    for (coord, tile) in mask.tiles() {
        check(cancel)?;
        let tile = tile.map_err(DocumentError::from)?;
        let Some(r) = tile_rect(bounds, *coord) else {
            continue;
        };
        let [ox, oy] = coord.origin();
        for row in 0..i64::from(r.height) {
            let src = ((r.y + row - oy) * 256 + (r.x - ox)) as usize;
            let dst =
                ((r.y + row - bounds.y) * i64::from(bounds.width) + (r.x - bounds.x)) as usize;
            let len = r.width as usize;
            for (pixel, &encoded) in selected[dst..dst + len]
                .iter_mut()
                .zip(&tile[src..src + len])
            {
                *pixel = u8::from((encoded ^ baseline) >= threshold);
            }
        }
    }
    let mut raw = boundary_rings(&selected, width, height, cancel)?;
    let mut work = 0;
    if tolerance > 0.0 {
        let proposed = raw
            .iter()
            .map(|p| simplify(p, tolerance, &mut work, cancel))
            .collect::<Result<Vec<_>, _>>()?;
        if proposed != raw && fits(&proposed) && preserves_topology(&raw, &proposed, cancel)? {
            raw = proposed;
        }
    }
    if !fits(&raw) {
        return Err(CommandError::Invalid("traced selection exceeds the editable anchor limit; try another tolerance or a smaller selection".into()));
    }
    Ok(raw
        .into_iter()
        .map(|ring| {
            ring.into_iter()
                .map(|p| anchor([p[0] + bounds.x as f64, p[1] + bounds.y as f64]))
                .collect()
        })
        .collect())
}
fn anchor(position: Point) -> PathAnchor {
    PathAnchor {
        position,
        incoming: [0.; 2],
        outgoing: [0.; 2],
    }
}
fn tile_rect(bounds: CoverageRect, coord: TileCoord) -> Option<CoverageRect> {
    let [ox, oy] = coord.origin();
    let x = ox.max(bounds.x);
    let y = oy.max(bounds.y);
    let right = ox
        .saturating_add(256)
        .min(bounds.x + i64::from(bounds.width));
    let bottom = oy
        .saturating_add(256)
        .min(bounds.y + i64::from(bounds.height));
    (right > x && bottom > y).then(|| CoverageRect {
        x,
        y,
        width: (right - x) as u32,
        height: (bottom - y) as u32,
    })
}
fn trace_bounds(
    mask: &TiledMask,
    background_inside: bool,
) -> Result<Option<CoverageRect>, DocumentError> {
    let bounds = mask.layout().bounds;
    if background_inside {
        return Ok(Some(bounds));
    }
    let mut extent = None;
    for tile in mask.tile_coords().filter_map(|&c| tile_rect(bounds, c)) {
        extent = Some(match extent {
            Some(old) => crate::geometry_pixels::union(old, tile)?,
            None => tile,
        });
    }
    Ok(extent)
}
fn boundary_rings(
    selected: &[u8],
    width: usize,
    height: usize,
    cancel: &dyn Fn() -> bool,
) -> Result<Vec<Vec<Point>>, CommandError> {
    let stride = width + 1;
    let size = stride * (height + 1);
    let mut edges = vec![0u8; size];
    for (y, row) in selected.chunks_exact(width).enumerate() {
        check(cancel)?;
        for (x, &v) in row.iter().enumerate() {
            if v == 0 {
                continue;
            }
            let i = y * width + x;
            let p = y * stride + x;
            if y == 0 || selected[i - width] == 0 {
                edges[p] |= 1;
            }
            if x + 1 == width || row[x + 1] == 0 {
                edges[p + 1] |= 2;
            }
            if y + 1 == height || selected[i + width] == 0 {
                edges[p + stride + 1] |= 4;
            }
            if x == 0 || row[x - 1] == 0 {
                edges[p + stride] |= 8;
            }
        }
    }
    let mut raw = Vec::new();
    let mut corners = 0;
    for y in 0..=height {
        check(cancel)?;
        for x in 0..=width {
            let start = y * stride + x;
            while edges[start] != 0 {
                if raw.len() >= MAX_CONTOURS {
                    return Err(CommandError::Invalid(
                        "selection exceeds 64 closed contours".into(),
                    ));
                }
                let points = ring(&mut edges, stride, start, cancel)?;
                corners += points.len();
                if corners > MAX_CORNERS {
                    return Err(DocumentError::ResourceLimit.into());
                }
                raw.push(points);
            }
        }
    }
    Ok(raw)
}
fn fits(rings: &[Vec<Point>]) -> bool {
    rings.iter().all(|p| p.len() <= MAX_ANCHORS)
        && rings.iter().map(Vec::len).sum::<usize>() <= MAX_PATH_ANCHORS
}

fn segments(points: &[Point]) -> impl Iterator<Item = (Point, Point)> + '_ {
    points
        .iter()
        .copied()
        .zip(points.iter().copied().cycle().skip(1))
}
fn intersects(a: Point, b: Point, c: Point, d: Point) -> bool {
    if (0..2).any(|i| a[i].max(b[i]) < c[i].min(d[i]) || c[i].max(d[i]) < a[i].min(b[i])) {
        return false;
    }
    let orient = |u: Point, v: Point, w: Point| {
        (v[0] - u[0]) * (w[1] - u[1]) - (v[1] - u[1]) * (w[0] - u[0])
    };
    orient(a, b, c) * orient(a, b, d) <= 0. && orient(c, d, a) * orient(c, d, b) <= 0.
}
fn inside(points: &[Point], p: Point) -> bool {
    segments(points)
        .filter(|(a, b)| {
            (a[1] > p[1]) != (b[1] > p[1])
                && p[0] < a[0] + (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1])
        })
        .count()
        % 2
        == 1
}
// The first vertex of every simplified ring is retained. Disjoint simple
// boundaries can only change their nesting by moving that vertex across
// another boundary. Fall back to the exact rings on any ambiguous contact.
fn preserves_topology(
    raw: &[Vec<Point>],
    new: &[Vec<Point>],
    cancel: &dyn Fn() -> bool,
) -> Result<bool, CommandError> {
    for (i, ring) in new.iter().enumerate() {
        if ring == &raw[i] {
            continue;
        }
        let mut seen = std::collections::HashSet::new();
        if raw[i]
            .iter()
            .any(|p| !seen.insert((p[0] as i64, p[1] as i64)))
        {
            return Ok(false);
        }
        for (e, (a, b)) in segments(ring).enumerate() {
            check(cancel)?;
            for (f, (c, d)) in segments(ring).enumerate().skip(e + 2) {
                if e == 0 && f + 1 == ring.len() {
                    continue;
                }
                if intersects(a, b, c, d) {
                    return Ok(false);
                }
            }
        }
        for (j, other) in new.iter().enumerate().filter(|(j, _)| *j != i) {
            if inside(&raw[i], raw[j][0]) != inside(ring, other[0]) {
                return Ok(false);
            }
            for (a, b) in segments(ring) {
                check(cancel)?;
                if segments(other).any(|(c, d)| intersects(a, b, c, d)) {
                    return Ok(false);
                }
            }
        }
    }
    Ok(true)
}

fn ring(
    edges: &mut [u8],
    stride: usize,
    start: usize,
    cancel: &dyn Fn() -> bool,
) -> Result<Vec<Point>, CommandError> {
    let mut pos = start;
    let first = edges[pos].trailing_zeros() as usize;
    let mut direction = first;
    let mut points = vec![[(pos % stride) as f64, (pos / stride) as f64]];
    let mut steps = 0;
    loop {
        edges[pos] &= !(1 << direction);
        pos = match direction {
            0 => pos + 1,
            1 => pos + stride,
            2 => pos - 1,
            _ => pos - stride,
        };
        steps += 1;
        if steps & 2047 == 0 {
            check(cancel)?;
        }
        if pos == start {
            if direction == first {
                points.remove(0);
            }
            return Ok(points);
        }
        let next = [(direction + 1) % 4, direction, (direction + 3) % 4]
            .into_iter()
            .find(|d| edges[pos] & (1 << d) != 0)
            .ok_or_else(|| CommandError::Invalid("selection boundary is open".into()))?;
        if next != direction {
            if points.len() >= MAX_CORNERS {
                return Err(DocumentError::ResourceLimit.into());
            }
            points.push([(pos % stride) as f64, (pos / stride) as f64]);
        }
        direction = next;
    }
}
fn squared_distance(p: Point, a: Point, b: Point) -> f64 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let length = dx * dx + dy * dy;
    let t = if length > 0.0 {
        (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / length).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (p[0] - a[0] - t * dx).powi(2) + (p[1] - a[1] - t * dy).powi(2)
}
// Simplify two open arcs of a closed ring. Keeping the original in the
// degenerate case retains small islands and holes even at large tolerances.
fn simplify(
    ring: &[Point],
    tolerance: f64,
    checks: &mut usize,
    cancel: &dyn Fn() -> bool,
) -> Result<Vec<Point>, CommandError> {
    let n = ring.len();
    let pivot = (1..n)
        .max_by(|&i, &j| {
            squared_distance(ring[i], ring[0], ring[0])
                .total_cmp(&squared_distance(ring[j], ring[0], ring[0]))
        })
        .unwrap();
    let mut keep = vec![false; n];
    keep[0] = true;
    keep[pivot] = true;
    let mut stack = vec![(0, pivot), (pivot, n)];
    while let Some((lo, hi)) = stack.pop() {
        check(cancel)?;
        *checks += hi - lo;
        if *checks > MAX_DISTANCE_CHECKS {
            return Err(DocumentError::ResourceLimit.into());
        }
        let mut farthest = None;
        let mut maximum = tolerance * tolerance;
        for i in lo + 1..hi {
            let dist = squared_distance(ring[i], ring[lo], ring[hi % n]);
            if dist > maximum {
                maximum = dist;
                farthest = Some(i);
            }
        }
        if let Some(i) = farthest {
            keep[i] = true;
            stack.extend([(lo, i), (i, hi)]);
        }
    }
    let result: Vec<_> = ring
        .iter()
        .zip(keep)
        .filter_map(|(&p, keep)| keep.then_some(p))
        .collect();
    let area = |p: &[Point]| {
        p.iter()
            .zip(p.iter().cycle().skip(1))
            .map(|(a, b)| a[0] * b[1] - a[1] * b[0])
            .sum::<f64>()
    };
    if result.len() < 3 || area(&result) * area(ring) <= 0.0 {
        return Ok(ring.to_vec());
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_three_by_three_binary_masks_reconstruct_including_diagonal_junctions() {
        for bits in 0u16..512 {
            let pixels: Vec<_> = (0..9).map(|i| ((bits >> i) & 1) as u8).collect();
            let contours = boundary_rings(&pixels, 3, 3, &|| false).unwrap();
            for y in 0..3 {
                for x in 0..3 {
                    let p = [x as f64 + 0.5, y as f64 + 0.5];
                    let covered = contours.iter().filter(|c| inside(c, p)).count() % 2;
                    assert_eq!(
                        covered,
                        usize::from(pixels[y * 3 + x]),
                        "mask {bits}, ({x},{y})"
                    );
                }
            }
        }
    }
}
