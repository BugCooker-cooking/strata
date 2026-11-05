//! W3C separable and nonseparable equations on linear Display P3 RGB.
//! Extra photographic modes use bounded integer-equivalent burn/dodge math.
use crate::BlendMode;
pub(crate) fn color(back: [f64; 3], front: [f64; 3], mode: BlendMode) -> [f64; 3] {
    use BlendMode::*;
    match mode {
        Hue => set_lum(set_sat(front, sat(back)), lum(back)),
        Saturation => set_lum(set_sat(back, sat(front)), lum(back)),
        Color => set_lum(front, lum(back)),
        Luminosity => set_lum(back, lum(front)),
        DarkerColor => {
            if front.iter().sum::<f64>() < back.iter().sum::<f64>() {
                front
            } else {
                back
            }
        }
        LighterColor => {
            if front.iter().sum::<f64>() > back.iter().sum::<f64>() {
                front
            } else {
                back
            }
        }
        _ => std::array::from_fn(|i| channel(back[i], front[i], mode)),
    }
}
fn burn(b: f64, s: f64) -> f64 {
    if b >= 1.0 {
        1.0
    } else if s <= 0.0 {
        0.0
    } else {
        1.0 - ((1.0 - b) / s).min(1.0)
    }
}
fn dodge(b: f64, s: f64) -> f64 {
    if b <= 0.0 {
        0.0
    } else if s >= 1.0 {
        1.0
    } else {
        (b / (1.0 - s)).min(1.0)
    }
}
fn vivid(b: f64, s: f64) -> f64 {
    if s <= 0.5 {
        burn(b, 2.0 * s)
    } else {
        dodge(b, 2.0 * s - 1.0)
    }
}
fn channel(b: f64, s: f64, mode: BlendMode) -> f64 {
    use BlendMode::*;
    match mode {
        Normal | Dissolve => s,
        Darken => b.min(s),
        Multiply => b * s,
        ColorBurn => burn(b, s),
        LinearBurn => (b + s - 1.0).max(0.0),
        Lighten => b.max(s),
        Screen => b + s - b * s,
        ColorDodge => dodge(b, s),
        LinearDodge => (b + s).min(1.0),
        Overlay => {
            if b <= 0.5 {
                2.0 * b * s
            } else {
                1.0 - 2.0 * (1.0 - b) * (1.0 - s)
            }
        }
        HardLight => {
            if s <= 0.5 {
                2.0 * b * s
            } else {
                1.0 - 2.0 * (1.0 - b) * (1.0 - s)
            }
        }
        SoftLight => {
            if s <= 0.5 {
                b - (1.0 - 2.0 * s) * b * (1.0 - b)
            } else {
                let d = if b <= 0.25 {
                    ((16.0 * b - 12.0) * b + 4.0) * b
                } else {
                    b.sqrt()
                };
                b + (2.0 * s - 1.0) * (d - b)
            }
        }
        VividLight => vivid(b, s),
        LinearLight => (b + 2.0 * s - 1.0).clamp(0.0, 1.0),
        PinLight => {
            if s <= 0.5 {
                b.min(2.0 * s)
            } else {
                b.max(2.0 * s - 1.0)
            }
        }
        HardMix => {
            if vivid(b, s) < 0.5 {
                0.0
            } else {
                1.0
            }
        }
        Difference => (b - s).abs(),
        Exclusion => b + s - 2.0 * b * s,
        Subtract => (b - s).max(0.0),
        Divide => {
            if b <= 0.0 {
                0.0
            } else if s <= 0.0 {
                1.0
            } else {
                (b / s).min(1.0)
            }
        }
        Hue | Saturation | Color | Luminosity | DarkerColor | LighterColor => {
            unreachable!("nonseparable RGB modes handled together")
        }
    }
}
fn lum(c: [f64; 3]) -> f64 {
    c[0] * 0.3 + c[1] * 0.59 + c[2] * 0.11
}
fn sat(c: [f64; 3]) -> f64 {
    c.into_iter().fold(f64::NEG_INFINITY, f64::max) - c.into_iter().fold(f64::INFINITY, f64::min)
}
fn clip(mut c: [f64; 3]) -> [f64; 3] {
    let l = lum(c);
    let n = c.into_iter().fold(f64::INFINITY, f64::min);
    let x = c.into_iter().fold(f64::NEG_INFINITY, f64::max);
    if n < 0.0 {
        for v in &mut c {
            *v = if l == n {
                0.0
            } else {
                l + (*v - l) * l / (l - n)
            };
        }
    }
    if x > 1.0 {
        for v in &mut c {
            *v = if x == l {
                1.0
            } else {
                l + (*v - l) * (1.0 - l) / (x - l)
            };
        }
    }
    c.map(|v| v.clamp(0.0, 1.0))
}
fn set_lum(c: [f64; 3], l: f64) -> [f64; 3] {
    let delta = l - lum(c);
    clip(c.map(|v| v + delta))
}
fn set_sat(mut c: [f64; 3], s: f64) -> [f64; 3] {
    let mut indices = [0, 1, 2];
    indices.sort_by(|a, b| c[*a].total_cmp(&c[*b]));
    let [min, mid, max] = indices;
    if c[max] > c[min] {
        c[mid] = (c[mid] - c[min]) * s / (c[max] - c[min]);
        c[max] = s;
    } else {
        c[mid] = 0.0;
        c[max] = 0.0;
    }
    c[min] = 0.0;
    c
}
