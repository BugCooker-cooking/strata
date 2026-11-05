//! Color sampling stays at source depth. Quick selection uses a bounded,
//! seeded grid cut; it never claims semantic object or ML inference.
use super::{selection::DenseMask, *};
use crate::{Document, DocumentError, LayerKind, composite::Compositor};
use std::collections::VecDeque;
use strata_pixels::{
    PixelBuffer, PixelFormat, TileCoord, TiledMask,
    memory::{MemoryKind, Reservation},
};

struct Samples {
    values: Vec<[u16; 4]>,
    width: usize,
    height: usize,
    _reservation: Reservation,
}
fn cancelled(cancel: &dyn Fn() -> bool) -> Result<(), CommandError> {
    if cancel() {
        Err(CommandError::Expired)
    } else {
        Ok(())
    }
}
fn normalized(mut p: [u16; 4]) -> [u16; 4] {
    // Invisible RGB cannot split a transparent region into different colors.
    if p[3] == 0 {
        p[..3].fill(0);
    }
    p
}
fn color(buffer: &PixelBuffer, index: usize) -> [u16; 4] {
    normalized(match buffer {
        PixelBuffer::Rgba8(v) => std::array::from_fn(|c| u16::from(v[index * 4 + c]) * 257),
        PixelBuffer::Rgba16(v) => std::array::from_fn(|c| v[index * 4 + c]),
    })
}
impl Samples {
    fn read(
        doc: &Document,
        source: SelectionSample,
        max_edge: u32,
        cancel: &dyn Fn() -> bool,
    ) -> Result<Self, CommandError> {
        let f = doc.frame();
        let scale = (f64::from(max_edge) / f64::from(f.width.max(f.height))).min(1.0);
        let width = (f64::from(f.width) * scale).round().max(1.0) as usize;
        let height = (f64::from(f.height) * scale).round().max(1.0) as usize;
        let reservation = doc
            .memory_pool()
            .reserve(
                MemoryKind::Transient,
                (width * height) as u64 * 8 + (width + height) as u64 * 8 + 4 * 1024 * 1024,
            )
            .map_err(DocumentError::from)?;
        let layer = match source {
            SelectionSample::Merged => None,
            SelectionSample::CurrentLayer => Some(
                doc.layers()
                    .iter()
                    .find(|l| {
                        Some(l.id()) == doc.active_layer()
                            && matches!(l.info().kind, LayerKind::Pixel)
                    })
                    .ok_or_else(|| {
                        CommandError::Invalid(
                            "current-layer sampling requires an active pixel layer".into(),
                        )
                    })?,
            ),
        };
        let origin = layer.map_or([0, 0], |l| [l.info().x, l.info().y]);
        let positions =
            |n: usize, size: u32, start: i64, offset: i64| -> Result<Vec<i64>, CommandError> {
                (0..n)
                    .map(|i| {
                        let v = ((i as u64 * 2 + 1) * u64::from(size) / (n as u64 * 2)) as i64;
                        start
                            .checked_add(v)
                            .and_then(|x| x.checked_sub(offset))
                            .ok_or_else(|| {
                                CommandError::Invalid("sampling coordinates overflow".into())
                            })
                    })
                    .collect()
            };
        let xs = positions(width, f.width, f.x, origin[0])?;
        let ys = positions(height, f.height, f.y, origin[1])?;
        let mut values = vec![[0; 4]; width * height];
        let mut compositor = Compositor::new(1);
        // Visit only sampled tiles. Each source/composite tile is decoded once,
        // including on very large documents with a sparse preview grid.
        let mut x0 = 0;
        while x0 < width {
            let tx = xs[x0].div_euclid(256);
            let x1 = xs.partition_point(|x| x.div_euclid(256) <= tx);
            let mut y0 = 0;
            while y0 < height {
                cancelled(cancel)?;
                let ty = ys[y0].div_euclid(256);
                let y1 = ys.partition_point(|y| y.div_euclid(256) <= ty);
                let coord = TileCoord::new(tx, ty).map_err(DocumentError::from)?;
                let [ox, oy] = coord.origin();
                let tile = if let Some(layer) = layer {
                    layer
                        .pixels()
                        .tile(coord)
                        .map_err(DocumentError::from)?
                        .map(|v| (*v).clone())
                } else {
                    let linear = compositor.tile(doc, coord)?;
                    Some(
                        strata_color::from_linear_p3(
                            &linear,
                            PixelFormat::Rgba16,
                            doc.working_icc(),
                        )
                        .map_err(DocumentError::from)?,
                    )
                };
                if let Some(tile) = tile {
                    for y in y0..y1 {
                        for x in x0..x1 {
                            let i = (ys[y] - oy) as usize * 256 + (xs[x] - ox) as usize;
                            values[y * width + x] = color(&tile, i);
                        }
                    }
                }
                y0 = y1;
            }
            x0 = x1;
        }
        Ok(Self {
            values,
            width,
            height,
            _reservation: reservation,
        })
    }
}

pub(super) fn wand(
    doc: &Document,
    p: &WandSelection,
    cancel: &dyn Fn() -> bool,
) -> Result<TiledMask, CommandError> {
    let f = doc.frame();
    let x = i128::from(p.x) - i128::from(f.x);
    let y = i128::from(p.y) - i128::from(f.y);
    if x < 0 || y < 0 || x >= i128::from(f.width) || y >= i128::from(f.height) {
        return Err(CommandError::Invalid(
            "wand seed must be inside the artboard".into(),
        ));
    }
    let image = Samples::read(doc, p.sample, f.width.max(f.height), cancel)?;
    let mut out = DenseMask::empty(doc, 12)?;
    let start = y as usize * image.width + x as usize;
    let target = image.values[start];
    let tolerance = u16::from(p.tolerance) * 257;
    let matches = |i: usize| {
        image.values[i]
            .iter()
            .zip(target)
            .all(|(a, b)| a.abs_diff(b) <= tolerance)
    };
    if p.contiguous {
        let mut queue = Vec::new();
        queue
            .try_reserve_exact(image.values.len())
            .map_err(|_| DocumentError::ResourceLimit)?;
        queue.push(start);
        out.values[start] = 65535;
        let mut cursor = 0;
        while cursor < queue.len() {
            if cursor.is_multiple_of(4096) {
                cancelled(cancel)?;
            }
            let i = queue[cursor];
            cursor += 1;
            let x = i % image.width;
            let y = i / image.width;
            for neighbor in [
                (x > 0).then(|| i - 1),
                (x + 1 < image.width).then_some(i + 1),
                (y > 0).then(|| i - image.width),
                (y + 1 < image.height).then_some(i + image.width),
            ]
            .into_iter()
            .flatten()
            {
                if out.values[neighbor] == 0 && matches(neighbor) {
                    out.values[neighbor] = 65535;
                    queue.push(neighbor);
                }
            }
        }
    } else {
        for (i, value) in out.values.iter_mut().enumerate() {
            if i.is_multiple_of(4096) {
                cancelled(cancel)?;
            }
            *value = if matches(i) { 65535 } else { 0 };
        }
    }
    out.tiled(doc, cancel)
}

#[derive(Clone, Copy)]
struct Edge {
    to: usize,
    reverse: usize,
    capacity: f64,
}
struct Cut {
    edges: Vec<Vec<Edge>>,
}
impl Cut {
    fn new(n: usize) -> Self {
        Self {
            edges: vec![Vec::new(); n],
        }
    }
    fn link(&mut self, from: usize, to: usize, forward: f64, backward: f64) {
        let a = self.edges[from].len();
        let b = self.edges[to].len();
        self.edges[from].push(Edge {
            to,
            reverse: b,
            capacity: forward,
        });
        self.edges[to].push(Edge {
            to: from,
            reverse: a,
            capacity: backward,
        });
    }
    fn solve(
        &mut self,
        source: usize,
        sink: usize,
        cancel: &dyn Fn() -> bool,
    ) -> Result<Vec<bool>, CommandError> {
        let n = self.edges.len();
        let mut levels = vec![usize::MAX; n];
        let mut current = vec![0; n];
        let mut path: Vec<(usize, usize)> = Vec::with_capacity(n);
        let mut queue = VecDeque::with_capacity(n);
        loop {
            cancelled(cancel)?;
            levels.fill(usize::MAX);
            levels[source] = 0;
            queue.clear();
            queue.push_back(source);
            while let Some(v) = queue.pop_front() {
                if v.is_multiple_of(1024) {
                    cancelled(cancel)?;
                }
                for e in &self.edges[v] {
                    if e.capacity > 1e-8 && levels[e.to] == usize::MAX {
                        levels[e.to] = levels[v] + 1;
                        queue.push_back(e.to);
                    }
                }
            }
            if levels[sink] == usize::MAX {
                break;
            }
            current.fill(0);
            // Iterative Dinic traversal avoids recursion on long thin images.
            loop {
                cancelled(cancel)?;
                path.clear();
                let mut v = source;
                while v != sink {
                    while current[v] < self.edges[v].len() {
                        let e = self.edges[v][current[v]];
                        if e.capacity > 1e-8 && levels[e.to] == levels[v] + 1 {
                            break;
                        }
                        current[v] += 1;
                    }
                    if current[v] == self.edges[v].len() {
                        levels[v] = usize::MAX;
                        if let Some((parent, _)) = path.pop() {
                            v = parent;
                            current[v] += 1;
                        } else {
                            break;
                        }
                    } else {
                        let edge = current[v];
                        path.push((v, edge));
                        v = self.edges[v][edge].to;
                    }
                    if path.len().is_multiple_of(1024) {
                        cancelled(cancel)?;
                    }
                }
                if v != sink {
                    break;
                }
                let amount = path
                    .iter()
                    .map(|(v, i)| self.edges[*v][*i].capacity)
                    .fold(f64::INFINITY, f64::min);
                for &(v, i) in &path {
                    let e = self.edges[v][i];
                    self.edges[v][i].capacity -= amount;
                    self.edges[e.to][e.reverse].capacity += amount;
                }
            }
        }
        Ok(levels.into_iter().map(|v| v != usize::MAX).collect())
    }
}
fn distance(a: [u16; 4], b: [u16; 4]) -> f64 {
    a.into_iter()
        .zip(b)
        .map(|(a, b)| ((f64::from(a) - f64::from(b)) / 65535.0).powi(2))
        .sum()
}
fn seed_grid(
    doc: &Document,
    image: &Samples,
    p: &QuickSelection,
    cancel: &dyn Fn() -> bool,
) -> Result<Vec<u8>, CommandError> {
    if p.foreground.is_empty()
        || p.foreground.len() + p.background.len() > 4096
        || !p.radius.is_finite()
        || !(0.5..=256.0).contains(&p.radius)
        || !p.smoothness.is_finite()
        || !(0.0..=100.0).contains(&p.smoothness)
    {
        return Err(CommandError::Invalid("quick selection needs foreground seeds, at most 4096 points, radius 0.5..256 and smoothness 0..100".into()));
    }
    let f = doc.frame();
    let mut seeds = vec![0; image.values.len()];
    for (points, label) in [(&p.foreground, 1), (&p.background, 2)] {
        for point in points {
            cancelled(cancel)?;
            if !point.iter().all(|v| v.is_finite())
                || point[0] < f.x as f64
                || point[1] < f.y as f64
                || point[0] >= f.x as f64 + f64::from(f.width)
                || point[1] >= f.y as f64 + f64::from(f.height)
            {
                return Err(CommandError::Invalid(
                    "quick selection seed must be finite and inside the artboard".into(),
                ));
            }
            let cx = (point[0] - f.x as f64) * image.width as f64 / f64::from(f.width);
            let cy = (point[1] - f.y as f64) * image.height as f64 / f64::from(f.height);
            let rx = p.radius * image.width as f64 / f64::from(f.width);
            let ry = p.radius * image.height as f64 / f64::from(f.height);
            let left = (cx - rx).floor().max(0.0) as usize;
            let right = ((cx + rx).ceil() as usize).min(image.width - 1);
            let top = (cy - ry).floor().max(0.0) as usize;
            let bottom = ((cy + ry).ceil() as usize).min(image.height - 1);
            for y in top..=bottom {
                for x in left..=right {
                    if ((x as f64 + 0.5 - cx) / rx).powi(2) + ((y as f64 + 0.5 - cy) / ry).powi(2)
                        > 1.0
                        && (x != cx.floor() as usize || y != cy.floor() as usize)
                    {
                        continue;
                    }
                    let seed = &mut seeds[y * image.width + x];
                    if *seed != 0 && *seed != label {
                        return Err(CommandError::Invalid("foreground/background brushes overlap at 256-pixel preview resolution; separate the seeds".into()));
                    }
                    *seed = label;
                }
            }
        }
    }
    if p.background.is_empty() {
        for y in 0..image.height {
            for x in 0..image.width {
                if (x == 0 || y == 0 || x + 1 == image.width || y + 1 == image.height)
                    && seeds[y * image.width + x] == 0
                {
                    seeds[y * image.width + x] = 2;
                }
            }
        }
    }
    if !seeds.contains(&2) {
        return Err(CommandError::Invalid(
            "no background seed remains; mark a separate background brush".into(),
        ));
    }
    Ok(seeds)
}
fn model(image: &Samples, seeds: &[u8], label: u8) -> ([f64; 4], [f64; 4]) {
    let mut mean = [0.0; 4];
    let mut second = [0.0; 4];
    let mut count = 0.0;
    for (p, &seed) in image.values.iter().zip(seeds) {
        if seed != label {
            continue;
        }
        count += 1.0;
        for c in 0..4 {
            let value = f64::from(p[c]) / 65535.0;
            mean[c] += value;
            second[c] += value * value;
        }
    }
    for c in 0..4 {
        mean[c] /= count;
        second[c] = (second[c] / count - mean[c] * mean[c]).max(0.0025);
    }
    (mean, second)
}
fn cost(p: [u16; 4], model: &([f64; 4], [f64; 4])) -> f64 {
    (0..4)
        .map(|c| {
            0.5 * ((f64::from(p[c]) / 65535.0 - model.0[c]).powi(2) / model.1[c] + model.1[c].ln())
        })
        .sum()
}
pub(super) fn quick(
    doc: &Document,
    p: &QuickSelection,
    cancel: &dyn Fn() -> bool,
) -> Result<TiledMask, CommandError> {
    let image = Samples::read(doc, p.sample, 256, cancel)?;
    let count = image.values.len();
    let _scratch = doc
        .memory_pool()
        .reserve(MemoryKind::Transient, count as u64 * 640 + 4096)
        .map_err(DocumentError::from)?;
    let seeds = seed_grid(doc, &image, p, cancel)?;
    let fg = model(&image, &seeds, 1);
    let bg = model(&image, &seeds, 2);
    let mut contrast = 0.0;
    let mut pairs = 0;
    for y in 0..image.height {
        for x in 0..image.width {
            let i = y * image.width + x;
            if x + 1 < image.width {
                contrast += distance(image.values[i], image.values[i + 1]);
                pairs += 1;
            }
            if y + 1 < image.height {
                contrast += distance(image.values[i], image.values[i + image.width]);
                pairs += 1;
            }
        }
    }
    let beta = if contrast > 1e-12 {
        pairs as f64 / (2.0 * contrast)
    } else {
        0.0
    };
    let mut graph = Cut::new(count + 2);
    let source = count;
    let sink = count + 1;
    for y in 0..image.height {
        cancelled(cancel)?;
        for x in 0..image.width {
            let i = y * image.width + x;
            let (f, b) = match seeds[i] {
                1 => (0.0, 1e9),
                2 => (1e9, 0.0),
                _ => {
                    let f = cost(image.values[i], &fg);
                    let b = cost(image.values[i], &bg);
                    let base = f.min(b);
                    (f - base, b - base)
                }
            };
            graph.link(source, i, b, 0.0);
            graph.link(i, sink, f, 0.0);
            for j in [
                (x + 1 < image.width).then_some(i + 1),
                (y + 1 < image.height).then_some(i + image.width),
            ]
            .into_iter()
            .flatten()
            {
                let weight = p.smoothness
                    * (-beta * distance(image.values[i], image.values[j])).exp()
                    + 0.001;
                graph.link(i, j, weight, weight);
            }
        }
    }
    let selected = graph.solve(source, sink, cancel)?;
    let mut out = DenseMask::empty(doc, 2)?;
    for y in 0..out.height {
        cancelled(cancel)?;
        let sy = ((f64::from(y) + 0.5) * image.height as f64 / f64::from(out.height) - 0.5)
            .clamp(0.0, (image.height - 1) as f64);
        let y0 = sy.floor() as usize;
        let y1 = (y0 + 1).min(image.height - 1);
        let fy = sy - y0 as f64;
        for x in 0..out.width {
            let sx = ((f64::from(x) + 0.5) * image.width as f64 / f64::from(out.width) - 0.5)
                .clamp(0.0, (image.width - 1) as f64);
            let x0 = sx.floor() as usize;
            let x1 = (x0 + 1).min(image.width - 1);
            let fx = sx - x0 as f64;
            let v = |x, y| {
                if selected[y * image.width + x] {
                    1.0
                } else {
                    0.0
                }
            };
            let value = (v(x0, y0) * (1.0 - fx) + v(x1, y0) * fx) * (1.0 - fy)
                + (v(x0, y1) * (1.0 - fx) + v(x1, y1) * fx) * fy;
            out.values[(y * out.width + x) as usize] = (value * 65535.0).round() as u16;
        }
    }
    out.tiled(doc, cancel)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn document(width: u32, height: u32) -> Document {
        let mut e = Editor::new();
        e.execute(e.request(Source::Test), "file.new", serde_json::json!({"width":width,"height":height,"bit_depth":16,"background":[32768,32769,50001,65535]}), |_|Ok(())).unwrap();
        e.document().unwrap().clone()
    }
    #[test]
    fn negative_canvas_and_raw_16bit_tolerance_preserve_adjacent_codes() {
        let mut doc = document(16, 16);
        doc.frame.x = -260;
        doc.frame.y = -3;
        doc.layers[0].info.x = -260;
        doc.layers[0].info.y = -3;
        doc.layers[0]
            .pixels
            .set_pixel(
                2,
                0,
                strata_pixels::PixelValue::Rgba16([32769, 32769, 50001, 65535]),
            )
            .unwrap();
        let mut p = WandSelection {
            x: -260,
            y: -3,
            tolerance: 0,
            contiguous: false,
            sample: SelectionSample::CurrentLayer,
            mode: SelectionMode::Replace,
        };
        let exact = wand(&doc, &p, &|| false).unwrap();
        assert_eq!(exact.sample(-260, -3).unwrap(), 65535);
        assert_eq!(exact.sample(-258, -3).unwrap(), 0);
        p.tolerance = 1;
        assert_eq!(
            wand(&doc, &p, &|| false).unwrap().sample(-258, -3).unwrap(),
            65535
        );
        p.sample = SelectionSample::Merged;
        assert_eq!(
            wand(&doc, &p, &|| false).unwrap().sample(-257, -2).unwrap(),
            65535
        );
    }
    #[test]
    fn quick_preview_upsamples_soft_edges_and_rejects_conflicting_coarse_seeds() {
        let mut doc = document(520, 64);
        let mut rgba = Vec::new();
        for y in 0..64 {
            for x in 0..520 {
                rgba.extend(if (200..320).contains(&x) && (10..54).contains(&y) {
                    [65535, 0, 0, 65535]
                } else {
                    [0, 0, 65535, 65535]
                });
            }
        }
        doc.layers[0].pixels = strata_pixels::TiledPixels::from_dense_with_pool(
            520,
            64,
            &PixelBuffer::Rgba16(rgba),
            64,
            doc.memory_pool(),
        )
        .unwrap();
        let mut p = QuickSelection {
            foreground: vec![[260.5, 32.5]],
            background: vec![[1.5, 1.5]],
            radius: 0.5,
            smoothness: 8.0,
            sample: SelectionSample::CurrentLayer,
            mode: SelectionMode::Replace,
        };
        let mask = quick(&doc, &p, &|| false).unwrap();
        assert_eq!(mask.sample(260, 32).unwrap(), 65535);
        assert_eq!(mask.sample(100, 32).unwrap(), 0);
        assert!((197..203).any(|x| {
            let v = mask.sample(x, 32).unwrap();
            v > 0 && v < 65535
        }));
        p.background = vec![[260.6, 32.6]];
        assert!(quick(&doc, &p, &|| false).is_err());
    }
    #[test]
    fn residual_cut_matches_exhaustive_energy_and_can_stop() {
        let unary = [(0.0, 20.0), (4.0, 1.0), (2.0, 3.0), (20.0, 0.0), (1.0, 5.0)];
        let pairs = [
            (0, 1, 7.0),
            (1, 2, 4.0),
            (2, 3, 6.0),
            (1, 4, 2.0),
            (4, 3, 1.0),
        ];
        let mut cut = Cut::new(7);
        for (i, (fg, bg)) in unary.iter().copied().enumerate() {
            cut.link(5, i, bg, 0.0);
            cut.link(i, 6, fg, 0.0);
        }
        for (a, b, w) in pairs {
            cut.link(a, b, w, w);
        }
        let energy = |selected: &[bool]| {
            unary
                .iter()
                .enumerate()
                .map(|(i, (f, b))| if selected[i] { *f } else { *b })
                .sum::<f64>()
                + pairs
                    .iter()
                    .map(|(a, b, w)| {
                        if selected[*a] != selected[*b] {
                            *w
                        } else {
                            0.0
                        }
                    })
                    .sum::<f64>()
        };
        let result = cut.solve(5, 6, &|| false).unwrap();
        let best = (0..32)
            .map(|bits| energy(&(0..5).map(|i| bits & (1 << i) != 0).collect::<Vec<_>>()))
            .fold(f64::INFINITY, f64::min);
        assert_eq!(energy(&result), best);
        assert!(matches!(
            cut.solve(5, 6, &|| true),
            Err(CommandError::Expired)
        ));
    }
}
