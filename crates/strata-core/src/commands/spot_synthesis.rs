use super::{CommandError, SpotHealMode};

type Pixel = [f64; 4];
const DIRECTIONS: [[isize; 2]; 4] = [[1, 0], [0, 1], [1, 1], [-1, 1]];
const CARDINAL: [[isize; 2]; 4] = [[1, 0], [-1, 0], [0, 1], [0, -1]];

struct Domain<'a> {
    pixels: &'a [Pixel],
    ids: &'a [i32],
    width: usize,
}

impl Domain<'_> {
    fn neighbor(&self, at: usize, [dx, dy]: [isize; 2]) -> Option<usize> {
        let x = (at % self.width).checked_add_signed(dx)?;
        let y = (at / self.width).checked_add_signed(dy)?;
        let index = y * self.width + x;
        (x < self.width && index < self.pixels.len()).then_some(index)
    }

    fn known(&self, i: usize) -> bool {
        self.ids[i] < 0 && self.pixels[i][3] > 1e-6
    }

    fn border(&self, i: usize) -> bool {
        self.known(i)
            && CARDINAL.iter().any(|&delta| {
                self.neighbor(i, delta)
                    .is_some_and(|neighbor| self.ids[neighbor] >= 0)
            })
    }
}

fn check(index: usize, cancel: &dyn Fn() -> bool) -> Result<(), CommandError> {
    if index.is_multiple_of(1024) && cancel() {
        return Err(CommandError::Expired);
    }
    Ok(())
}

fn mix(mut bits: u64) -> u64 {
    bits = (bits ^ (bits >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    bits = (bits ^ (bits >> 27)).wrapping_mul(0x94d049bb133111eb);
    bits ^ (bits >> 31)
}

/// Reuse local high-frequency color differences as deterministic synthetic
/// texture. The solver supplies the low-frequency color from the target rim.
fn texture(
    domain: Domain<'_>,
    out: &mut [Pixel],
    cancel: &dyn Fn() -> bool,
) -> Result<(), CommandError> {
    let mut details = Vec::<[f64; 3]>::with_capacity(512);
    let mut seen = 0u64;
    let mut has_border = false;
    for i in 0..out.len() {
        check(i, cancel)?;
        if !domain.border(i) {
            continue;
        }
        has_border = true;
        for delta in CARDINAL {
            if let Some(j) = domain.neighbor(i, delta).filter(|&j| domain.known(j)) {
                let detail = std::array::from_fn(|c| {
                    ((domain.pixels[i][c] - domain.pixels[j][c]) * 0.35).clamp(-0.08, 0.08)
                });
                seen += 1;
                if details.len() < 512 {
                    details.push(detail);
                } else {
                    let slot = (mix(seen) % seen) as usize;
                    if slot < details.len() {
                        details[slot] = detail;
                    }
                }
            }
        }
    }
    if !has_border {
        return Err(CommandError::Invalid(
            "spot healing needs nontransparent content around the stroke".into(),
        ));
    }
    for (i, pixel) in out.iter_mut().enumerate() {
        check(i, cancel)?;
        let rgb = if domain.ids[i] < 0 || details.is_empty() {
            [0.0; 3]
        } else {
            let noise = mix(i as u64 + 1);
            let sign = if noise & 1 == 0 { -1.0 } else { 1.0 };
            details[((noise >> 1) % details.len() as u64) as usize].map(|v| v * sign)
        };
        pixel[..3].copy_from_slice(&rgb);
    }
    Ok(())
}

/// Follow short, compatible color runs across a hole. Each pair of sweeps
/// caches the first valid boundary hit along a direction. The most compatible
/// pair guides the solve, allowing a line to continue through the defect.
fn directional(
    domain: Domain<'_>,
    radius: u32,
    out: &mut [Pixel],
    cancel: &dyn Fn() -> bool,
) -> Result<(), CommandError> {
    const NONE: usize = usize::MAX;
    let n = out.len();
    let mut score = vec![f64::INFINITY; n];
    let mut near = vec![NONE; n];
    let mut far = vec![NONE; n];
    let mut rim_sum = [0.0; 3];
    let mut rim_count = 0;
    for i in 0..n {
        check(i, cancel)?;
        if domain.border(i) {
            rim_count += 1;
            for (c, sum) in rim_sum.iter_mut().enumerate() {
                *sum += domain.pixels[i][c];
            }
        }
    }
    if rim_count == 0 {
        return Err(CommandError::Invalid(
            "spot healing needs nontransparent content around the stroke".into(),
        ));
    }
    let fallback = rim_sum.map(|v| v / f64::from(rim_count));
    for (i, px) in out.iter_mut().enumerate() {
        if domain.ids[i] >= 0 {
            px[..3].copy_from_slice(&fallback);
        }
    }
    for [dx, dy] in DIRECTIONS {
        let stride = (dy * domain.width as isize + dx) as usize;
        for i in 0..n {
            check(i, cancel)?;
            near[i] = if domain.known(i) {
                i
            } else if domain.ids[i] >= 0 {
                domain.neighbor(i, [-dx, -dy]).map_or(NONE, |j| near[j])
            } else {
                NONE
            };
        }
        for i in (0..n).rev() {
            check(i, cancel)?;
            if domain.ids[i] < 0 {
                far[i] = if domain.known(i) { i } else { NONE };
                continue;
            }
            far[i] = domain.neighbor(i, [dx, dy]).map_or(NONE, |j| far[j]);
            let (a, b) = (near[i], far[i]);
            if a == NONE || b == NONE {
                continue;
            }
            let steps = (b - a) / stride;
            if (i - a).max(b - i) / stride > radius as usize {
                continue;
            }
            let before = domain.pixels[a];
            let after = domain.pixels[b];
            let error =
                (0..3).map(|c| (before[c] - after[c]).powi(2)).sum::<f64>() + steps as f64 * 1e-6;
            if error < score[i] {
                score[i] = error;
                let t = (i - a) as f64 / (b - a) as f64;
                for c in 0..3 {
                    out[i][c] = before[c] * (1.0 - t) + after[c] * t;
                }
            }
        }
    }
    Ok(())
}

pub(super) fn guide(
    mode: SpotHealMode,
    pixels: &[Pixel],
    ids: &[i32],
    width: usize,
    radius: u32,
    out: &mut [Pixel],
    cancel: &dyn Fn() -> bool,
) -> Result<(), CommandError> {
    let domain = Domain { pixels, ids, width };
    match mode {
        SpotHealMode::CreateTexture => texture(domain, out, cancel),
        SpotHealMode::ContentAware => directional(domain, radius, out, cancel),
        SpotHealMode::ProximityMatch => {
            unreachable!("proximity mode uses an explicit sampled patch")
        }
    }
}
