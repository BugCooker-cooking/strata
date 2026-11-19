use super::*;
use crate::{Document, DocumentError};
use serde_json::json;
use strata_pixels::{
    CoverageRect, TileCoord, TiledMask,
    memory::{MemoryKind, Reservation},
};

pub(super) struct DenseMask {
    pub(super) values: Vec<u16>,
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) bounds: CoverageRect,
    _reservation: Reservation,
}
impl DenseMask {
    pub(super) fn empty(doc: &Document, multiplier: u64) -> Result<Self, CommandError> {
        let f = doc.frame();
        let area = u64::from(f.width) * u64::from(f.height);
        let reservation = doc
            .memory_pool()
            .reserve(
                MemoryKind::Transient,
                area.checked_mul(multiplier)
                    .ok_or(DocumentError::ResourceLimit)?
                    + 256 * 256 * 2
                    + (u64::from(f.width) + u64::from(f.height)) * 48,
            )
            .map_err(DocumentError::from)?;
        let mut values = Vec::new();
        values
            .try_reserve_exact(area as usize)
            .map_err(|_| DocumentError::ResourceLimit)?;
        values.resize(area as usize, 0);
        Ok(Self {
            values,
            width: f.width,
            height: f.height,
            bounds: CoverageRect {
                x: f.x,
                y: f.y,
                width: f.width,
                height: f.height,
            },
            _reservation: reservation,
        })
    }
    fn read(
        doc: &Document,
        multiplier: u64,
        cancel: &dyn Fn() -> bool,
    ) -> Result<Self, CommandError> {
        let source = doc.selection().ok_or_else(|| {
            CommandError::Invalid("no selection; create a selection first".into())
        })?;
        let mut dense = Self::empty(doc, multiplier)?;
        for ty in 0..dense.height.div_ceil(256) {
            if cancel() {
                return Err(CommandError::Expired);
            }
            for tx in 0..dense.width.div_ceil(256) {
                let tile = source
                    .region_tile(
                        dense.bounds.x + i64::from(tx * 256),
                        dense.bounds.y + i64::from(ty * 256),
                    )
                    .map_err(DocumentError::from)?;
                for y in 0..(dense.height - ty * 256).min(256) {
                    for x in 0..(dense.width - tx * 256).min(256) {
                        dense.values[((ty * 256 + y) * dense.width + tx * 256 + x) as usize] =
                            tile[(y * 256 + x) as usize];
                    }
                }
            }
        }
        Ok(dense)
    }
    pub(super) fn tiled(
        &self,
        doc: &Document,
        cancel: &dyn Fn() -> bool,
    ) -> Result<TiledMask, CommandError> {
        let mut out = super::local::uniform(doc, 0)?;
        let (first, _) = TileCoord::locate(self.bounds.x, self.bounds.y);
        let (last, _) = TileCoord::locate(
            self.bounds.x + i64::from(self.width) - 1,
            self.bounds.y + i64::from(self.height) - 1,
        );
        for tx in first.x()..=last.x() {
            if cancel() {
                return Err(CommandError::Expired);
            }
            for ty in first.y()..=last.y() {
                let coord = TileCoord::new(tx, ty).map_err(DocumentError::from)?;
                let [ox, oy] = coord.origin();
                let mut block = vec![0; 256 * 256];
                for y in 0..256usize {
                    let sy = i128::from(oy) + y as i128 - i128::from(self.bounds.y);
                    if sy < 0 || sy >= i128::from(self.height) {
                        continue;
                    }
                    for x in 0..256usize {
                        let sx = i128::from(ox) + x as i128 - i128::from(self.bounds.x);
                        if sx >= 0 && sx < i128::from(self.width) {
                            block[y * 256 + x] =
                                self.values[sy as usize * self.width as usize + sx as usize];
                        }
                    }
                }
                out.insert_tile(coord, block).map_err(DocumentError::from)?;
            }
        }
        Ok(out)
    }
}
fn ellipse(
    doc: &Document,
    p: &Marquee,
    cancel: &dyn Fn() -> bool,
) -> Result<TiledMask, CommandError> {
    CoverageRect {
        x: p.x,
        y: p.y,
        width: p.width,
        height: p.height,
    }
    .validate()
    .map_err(DocumentError::from)?;
    let mut out = super::local::uniform(doc, 0)?;
    let f = doc.frame();
    let left = p.x.max(f.x);
    let top = p.y.max(f.y);
    let right = (p.x + i64::from(p.width)).min(f.x + i64::from(f.width));
    let bottom = (p.y + i64::from(p.height)).min(f.y + i64::from(f.height));
    if left >= right || top >= bottom {
        return Ok(out);
    }
    let cx = p.x as f64 + f64::from(p.width) / 2.0;
    let cy = p.y as f64 + f64::from(p.height) / 2.0;
    let rx = f64::from(p.width) / 2.0;
    let ry = f64::from(p.height) / 2.0;
    let (first, _) = TileCoord::locate(left, top);
    let (last, _) = TileCoord::locate(right - 1, bottom - 1);
    for tx in first.x()..=last.x() {
        if cancel() {
            return Err(CommandError::Expired);
        }
        for ty in first.y()..=last.y() {
            let _scratch = doc
                .memory_pool()
                .reserve(MemoryKind::Transient, 256 * 256 * 2)
                .map_err(DocumentError::from)?;
            let coord = TileCoord::new(tx, ty).map_err(DocumentError::from)?;
            let [ox, oy] = coord.origin();
            let mut values = vec![0; 256 * 256];
            for y in top.max(oy)..bottom.min(oy.saturating_add(256)) {
                for x in left.max(ox)..right.min(ox.saturating_add(256)) {
                    let dx = (x as f64 + 0.5 - cx) / rx;
                    let dy = (y as f64 + 0.5 - cy) / ry;
                    let far_x = dx.abs() + 0.5 / rx;
                    let far_y = dy.abs() + 0.5 / ry;
                    let near_x = (dx.abs() - 0.5 / rx).max(0.0);
                    let near_y = (dy.abs() - 0.5 / ry).max(0.0);
                    let coverage = if far_x * far_x + far_y * far_y <= 1.0 {
                        65535
                    } else if near_x * near_x + near_y * near_y >= 1.0 {
                        0
                    } else {
                        let mut count = 0u32;
                        for sy in 0..4 {
                            for sx in 0..4 {
                                let dx = (x as f64 + (f64::from(sx) + 0.5) / 4.0 - cx) / rx;
                                let dy = (y as f64 + (f64::from(sy) + 0.5) / 4.0 - cy) / ry;
                                if dx * dx + dy * dy <= 1.0 {
                                    count += 1;
                                }
                            }
                        }
                        (count * 65535 / 16) as u16
                    };
                    values[(y - oy) as usize * 256 + (x - ox) as usize] = coverage;
                }
            }
            out.insert_tile(coord, values)
                .map_err(DocumentError::from)?;
        }
    }
    Ok(out)
}
pub(super) fn polygon(
    doc: &Document,
    p: &PolygonSelection,
    cancel: &dyn Fn() -> bool,
) -> Result<TiledMask, CommandError> {
    if !(3..=4096).contains(&p.points.len())
        || p.points
            .iter()
            .flatten()
            .any(|v| !v.is_finite() || v.abs() > 1e12)
    {
        return Err(CommandError::Invalid(
            "polygon needs 3..4096 finite points within ±1e12 canvas pixels".into(),
        ));
    }
    let mut out = DenseMask::empty(doc, 2)?;
    let min_y = p
        .points
        .iter()
        .map(|p| p[1])
        .fold(f64::INFINITY, f64::min)
        .floor()
        .max(out.bounds.y as f64) as i64;
    let max_y = p
        .points
        .iter()
        .map(|p| p[1])
        .fold(f64::NEG_INFINITY, f64::max)
        .ceil()
        .min(out.bounds.y as f64 + f64::from(out.height)) as i64;
    let edges = p
        .points
        .iter()
        .copied()
        .zip(p.points.iter().copied().cycle().skip(1))
        .take(p.points.len())
        .collect::<Vec<_>>();
    let mut crossings = Vec::with_capacity(p.points.len());
    for y in min_y..max_y {
        if cancel() {
            return Err(CommandError::Expired);
        }
        for sy in 0..4 {
            let scan = y as f64 + (f64::from(sy) + 0.5) / 4.0;
            crossings.clear();
            for (a, b) in &edges {
                if (a[1] <= scan && b[1] > scan) || (b[1] <= scan && a[1] > scan) {
                    crossings.push(a[0] + (scan - a[1]) * (b[0] - a[0]) / (b[1] - a[1]));
                }
            }
            crossings.sort_by(f64::total_cmp);
            for span in crossings.chunks_exact(2) {
                let left = span[0].max(out.bounds.x as f64);
                let right = span[1].min(out.bounds.x as f64 + f64::from(out.width));
                if right <= left {
                    continue;
                }
                for x in left.floor() as i64..right.ceil() as i64 {
                    let mut count = 0u16;
                    for sx in 0..4 {
                        let sample = x as f64 + (f64::from(sx) + 0.5) / 4.0;
                        if sample >= left && sample < right {
                            count += 1;
                        }
                    }
                    if x >= out.bounds.x && x < out.bounds.x + i64::from(out.width) {
                        let index = (y - out.bounds.y) as usize * out.width as usize
                            + (x - out.bounds.x) as usize;
                        out.values[index] += count;
                    }
                }
            }
        }
    }
    for value in &mut out.values {
        *value = (u32::from(*value) * 65535 / 16) as u16;
    }
    out.tiled(doc, cancel)
}
fn distance_line(f: &[f64]) -> Vec<f64> {
    let n = f.len();
    let mut sites = vec![0usize; n];
    let mut cuts = vec![0.0; n + 1];
    let mut k = 0usize;
    cuts[0] = f64::NEG_INFINITY;
    cuts[1] = f64::INFINITY;
    for q in 1..n {
        let mut split = ((f[q] + (q * q) as f64) - (f[sites[k]] + (sites[k] * sites[k]) as f64))
            / (2.0 * (q - sites[k]) as f64);
        while split <= cuts[k] && k > 0 {
            k -= 1;
            split = ((f[q] + (q * q) as f64) - (f[sites[k]] + (sites[k] * sites[k]) as f64))
                / (2.0 * (q - sites[k]) as f64);
        }
        k += 1;
        sites[k] = q;
        cuts[k] = split;
        cuts[k + 1] = f64::INFINITY;
    }
    let mut out = vec![0.0; n];
    k = 0;
    for (q, value) in out.iter_mut().enumerate() {
        while cuts[k + 1] < (q as f64) {
            k += 1
        }
        *value = (q as f64 - sites[k] as f64).powi(2) + f[sites[k]];
    }
    out
}
fn distance_field(
    values: &[u16],
    width: u32,
    height: u32,
    selected: bool,
    cancel: &dyn Fn() -> bool,
) -> Result<Vec<f64>, CommandError> {
    let w = width as usize;
    let h = height as usize;
    let mut first = vec![0.0; w * h];
    let mut result = vec![0.0; w * h];
    for y in 0..h {
        if cancel() {
            return Err(CommandError::Expired);
        }
        let row: Vec<_> = (0..w)
            .map(|x| {
                if (values[y * w + x] >= 32768) == selected {
                    0.0
                } else {
                    1e15
                }
            })
            .collect();
        let row = distance_line(&row);
        first[y * w..(y + 1) * w].copy_from_slice(&row);
    }
    for x in 0..w {
        if cancel() {
            return Err(CommandError::Expired);
        }
        let column: Vec<_> = (0..h).map(|y| first[y * w + x]).collect();
        let column = distance_line(&column);
        for y in 0..h {
            let edge = (x + 1).min(y + 1).min(w - x).min(h - y) as f64;
            result[y * w + x] = if selected {
                column[y]
            } else {
                column[y].min(edge * edge)
            };
        }
    }
    Ok(result)
}

fn gaussian(
    values: &[u16],
    width: u32,
    height: u32,
    sigma: f64,
    cancel: &dyn Fn() -> bool,
) -> Result<Vec<u16>, CommandError> {
    if sigma == 0.0 {
        return Ok(values.to_vec());
    }
    if sigma > 8.0 {
        return gaussian_boxes(values, width, height, sigma, cancel);
    }
    let radius = (sigma * 3.0).ceil() as i64;
    let mut kernel: Vec<_> = (-radius..=radius)
        .map(|x| (-(x as f64 * x as f64) / (2.0 * sigma * sigma)).exp())
        .collect();
    let total: f64 = kernel.iter().sum();
    for weight in &mut kernel {
        *weight /= total;
    }
    let w = width as usize;
    let h = height as usize;
    let mut temporary = vec![0.0f64; values.len()];
    let mut out = vec![0; values.len()];
    for y in 0..h {
        if cancel() {
            return Err(CommandError::Expired);
        }
        for x in 0..w {
            let mut value = 0.0;
            for (dx, weight) in (-radius..=radius).zip(&kernel) {
                let sx = x as i64 + dx;
                if sx >= 0 && sx < width as i64 {
                    value += f64::from(values[y * w + sx as usize]) * weight;
                }
            }
            temporary[y * w + x] = value;
        }
    }
    for y in 0..h {
        if cancel() {
            return Err(CommandError::Expired);
        }
        for x in 0..w {
            let mut value = 0.0;
            for (dy, weight) in (-radius..=radius).zip(&kernel) {
                let sy = y as i64 + dy;
                if sy >= 0 && sy < height as i64 {
                    value += temporary[sy as usize * w + x] * weight;
                }
            }
            out[y * w + x] = value.round().clamp(0.0, 65535.0) as u16;
        }
    }
    Ok(out)
}
fn gaussian_boxes(
    values: &[u16],
    width: u32,
    height: u32,
    sigma: f64,
    cancel: &dyn Fn() -> bool,
) -> Result<Vec<u16>, CommandError> {
    let ideal = (4.0 * sigma * sigma + 1.0).sqrt();
    let mut lower = ideal.floor() as usize;
    if lower.is_multiple_of(2) {
        lower = lower.saturating_sub(1)
    }
    lower = lower.max(1);
    let upper = lower + 2;
    let m = ((12.0 * sigma * sigma - 3.0 * (lower * lower) as f64 - 12.0 * lower as f64 - 9.0)
        / (-4.0 * lower as f64 - 4.0))
        .round()
        .clamp(0.0, 3.0) as usize;
    let w = width as usize;
    let h = height as usize;
    let mut image = values.iter().map(|v| f64::from(*v)).collect::<Vec<_>>();
    let mut temporary = vec![0.0; image.len()];
    let mut prefix = vec![0.0; w.max(h) + 1];
    for pass in 0..3 {
        let kernel = if pass < m { lower } else { upper };
        let radius = kernel / 2;
        let scale = 1.0 / kernel as f64;
        for y in 0..h {
            if cancel() {
                return Err(CommandError::Expired);
            }
            prefix[0] = 0.0;
            for x in 0..w {
                prefix[x + 1] = prefix[x] + image[y * w + x];
            }
            for x in 0..w {
                temporary[y * w + x] =
                    (prefix[(x + radius + 1).min(w)] - prefix[x.saturating_sub(radius)]) * scale;
            }
        }
        for x in 0..w {
            if cancel() {
                return Err(CommandError::Expired);
            }
            prefix[0] = 0.0;
            for y in 0..h {
                prefix[y + 1] = prefix[y] + temporary[y * w + x];
            }
            for y in 0..h {
                image[y * w + x] =
                    (prefix[(y + radius + 1).min(h)] - prefix[y.saturating_sub(radius)]) * scale;
            }
        }
    }
    Ok(image
        .into_iter()
        .map(|v| v.round().clamp(0.0, 65535.0) as u16)
        .collect())
}
fn modify(
    doc: &Document,
    id: &str,
    radius: f64,
    cancel: &dyn Fn() -> bool,
) -> Result<TiledMask, CommandError> {
    if !radius.is_finite() || !(0.0..=256.0).contains(&radius) {
        return Err(CommandError::Invalid(
            "selection radius must be finite in 0..256 pixels".into(),
        ));
    }
    let mut dense = DenseMask::read(doc, 40, cancel)?;
    if radius == 0.0 && id != "select.border" {
        return dense.tiled(doc, cancel);
    }
    match id {
        "select.feather" => {
            dense.values = gaussian(&dense.values, dense.width, dense.height, radius, cancel)?
        }
        "select.smooth" => {
            let filtered = gaussian(
                &dense.values,
                dense.width,
                dense.height,
                radius.max(0.01) / 2.0,
                cancel,
            )?;
            dense.values = filtered
                .into_iter()
                .map(|v| ((f64::from(v) / 65535.0 - 0.25) * 2.0).clamp(0.0, 1.0) * 65535.0)
                .map(|v| v.round() as u16)
                .collect();
        }
        _ => {
            let outside = matches!(id, "select.expand" | "select.border");
            let distances =
                distance_field(&dense.values, dense.width, dense.height, outside, cancel)?;
            let inner = if id == "select.border" {
                Some(distance_field(
                    &dense.values,
                    dense.width,
                    dense.height,
                    false,
                    cancel,
                )?)
            } else {
                None
            };
            for (index, value) in dense.values.iter_mut().enumerate() {
                let distance = distances[index].sqrt();
                *value = match id {
                    "select.expand" => (*value)
                        .max(((radius + 0.5 - distance).clamp(0.0, 1.0) * 65535.0).round() as u16),
                    "select.contract" => (*value)
                        .min(((distance - radius + 0.5).clamp(0.0, 1.0) * 65535.0).round() as u16),
                    "select.border" => {
                        let signed = if *value >= 32768 {
                            inner.as_ref().unwrap()[index].sqrt() - 0.5
                        } else {
                            -(distance - 0.5)
                        };
                        ((radius / 2.0 + 0.5 - signed.abs()).clamp(0.0, 1.0) * 65535.0).round()
                            as u16
                    }
                    _ => unreachable!(),
                };
            }
        }
    }
    dense.tiled(doc, cancel)
}
fn transform(
    doc: &Document,
    matrix: [f64; 6],
    cancel: &dyn Fn() -> bool,
) -> Result<TiledMask, CommandError> {
    if matrix.iter().any(|v| !v.is_finite() || v.abs() > 1e12) {
        return Err(CommandError::Invalid(
            "selection transform requires six finite bounded affine values".into(),
        ));
    }
    let [a, b, c, d, tx, ty] = matrix;
    let determinant = a * d - b * c;
    if determinant.abs() < 1e-12 {
        return Err(CommandError::Invalid(
            "selection transform must be invertible".into(),
        ));
    }
    let dense = DenseMask::read(doc, 4, cancel)?;
    let mut result = vec![0; dense.values.len()];
    for y in 0..dense.height {
        if cancel() {
            return Err(CommandError::Expired);
        }
        for x in 0..dense.width {
            let gx = dense.bounds.x as f64 + f64::from(x) + 0.5 - tx;
            let gy = dense.bounds.y as f64 + f64::from(y) + 0.5 - ty;
            let sx = (d * gx - c * gy) / determinant - dense.bounds.x as f64 - 0.5;
            let sy = (-b * gx + a * gy) / determinant - dense.bounds.y as f64 - 0.5;
            if sx <= -1.0
                || sy <= -1.0
                || sx >= f64::from(dense.width)
                || sy >= f64::from(dense.height)
            {
                continue;
            }
            let x0 = sx.floor() as i64;
            let y0 = sy.floor() as i64;
            let fx = sx - x0 as f64;
            let fy = sy - y0 as f64;
            let mut value = 0.0;
            for (px, wx) in [(x0, 1.0 - fx), (x0 + 1, fx)] {
                for (py, wy) in [(y0, 1.0 - fy), (y0 + 1, fy)] {
                    if px >= 0
                        && py >= 0
                        && px < i64::from(dense.width)
                        && py < i64::from(dense.height)
                    {
                        value += f64::from(
                            dense.values[py as usize * dense.width as usize + px as usize],
                        ) * wx
                            * wy;
                    }
                }
            }
            result[(y * dense.width + x) as usize] = value.round().clamp(0.0, 65535.0) as u16;
        }
    }
    let out = DenseMask {
        values: result,
        ..dense
    };
    out.tiled(doc, cancel)
}
pub(super) fn mutate(
    doc: &mut Document,
    id: &str,
    params: Value,
) -> Result<Option<Value>, CommandError> {
    mutate_cancellable(doc, id, params, &|| false)
}
pub(super) fn mutate_cancellable(
    doc: &mut Document,
    id: &str,
    params: Value,
    cancel: &dyn Fn() -> bool,
) -> Result<Option<Value>, CommandError> {
    if cancel() {
        return Err(CommandError::Expired);
    }
    if id == "select.reselect" {
        let _: Empty = parse(params)?;
        let previous = doc
            .previous_selection
            .clone()
            .ok_or_else(|| CommandError::Invalid("no prior selection to restore".into()))?;
        let current = doc.selection.replace(previous);
        if current.is_some() {
            doc.previous_selection = current;
        }
        return Ok(Some(
            json!({"selection":doc.selection().map(TiledMask::layout)}),
        ));
    }
    let (mask, mode) = match id {
        "select.wand" => {
            let p: WandSelection = parse(params)?;
            (super::sample_selection::wand(doc, &p, cancel)?, p.mode)
        }
        "select.quick" => {
            let p: QuickSelection = parse(params)?;
            (super::sample_selection::quick(doc, &p, cancel)?, p.mode)
        }
        "select.feather" | "select.expand" | "select.contract" | "select.smooth"
        | "select.border" => {
            let p: SelectionRadius = parse(params)?;
            (modify(doc, id, p.radius, cancel)?, SelectionMode::Replace)
        }
        "select.transform" => {
            let p: TransformSelection = parse(params)?;
            (transform(doc, p.matrix, cancel)?, SelectionMode::Replace)
        }
        "select.ellipse" => {
            let p: Marquee = parse(params)?;
            (ellipse(doc, &p, cancel)?, p.mode)
        }
        "select.from_path" => {
            let p: super::paths::PathSelection = parse(params)?;
            (super::paths::coverage(doc, &p.path, cancel)?, p.mode)
        }
        "select.polygon" | "select.lasso" => {
            let p: PolygonSelection = parse(params)?;
            (polygon(doc, &p, cancel)?, p.mode)
        }
        _ => return Ok(None),
    };
    let combined =
        super::local::combine_cancellable(doc, doc.selection.as_ref(), mask, mode, cancel)?;
    if let Some(current) = &doc.selection {
        doc.previous_selection = Some(current.clone());
    }
    doc.selection = Some(combined);
    Ok(Some(
        json!({"selection":doc.selection().map(TiledMask::layout)}),
    ))
}
