use crate::{BlendMode, Document, DocumentError, Layer, LayerKind};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use strata_pixels::{PixelBuffer, TileCoord, memory::MemoryKind};

fn opacity() -> f64 {
    1.0
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Shadow {
    pub color: [u16; 4],
    pub offset_x: i32,
    pub offset_y: i32,
    pub blur: f64,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum StrokePosition {
    #[default]
    Outside,
    Inside,
    Center,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Stroke {
    pub color: [u16; 4],
    pub size: f64,
    #[serde(default)]
    pub position: StrokePosition,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Glow {
    pub color: [u16; 4],
    pub size: f64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Overlay {
    pub color: [u16; 4],
    #[serde(default = "opacity")]
    pub opacity: f64,
    #[serde(default)]
    pub blend_mode: BlendMode,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GradientOverlay {
    pub start: [u16; 4],
    pub end: [u16; 4],
    pub angle: f64,
    #[serde(default = "opacity")]
    pub scale: f64,
    #[serde(default = "opacity")]
    pub opacity: f64,
    #[serde(default)]
    pub radial: bool,
    #[serde(default)]
    pub blend_mode: BlendMode,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LayerStyles {
    #[serde(default)]
    pub drop_shadow: Option<Shadow>,
    #[serde(default)]
    pub stroke: Option<Stroke>,
    #[serde(default)]
    pub outer_glow: Option<Glow>,
    #[serde(default)]
    pub color_overlay: Option<Overlay>,
    #[serde(default)]
    pub gradient_overlay: Option<GradientOverlay>,
}
impl LayerStyles {
    pub fn is_empty(&self) -> bool {
        self == &Self::default()
    }
    pub fn outset(&self) -> [i64; 4] {
        let mut pad = [0i64; 4];
        let size = self
            .stroke
            .as_ref()
            .map_or(0.0, |s| {
                if s.position == StrokePosition::Inside {
                    0.0
                } else {
                    s.size
                }
            })
            .max(self.outer_glow.as_ref().map_or(0.0, |s| s.size));
        pad.fill(size.ceil() as i64 + 2);
        if let Some(shadow) = &self.drop_shadow {
            let blur = (shadow.blur * 3.0).ceil() as i64 + 2;
            pad[0] = pad[0].max((blur - i64::from(shadow.offset_x)).max(0));
            pad[1] = pad[1].max((blur - i64::from(shadow.offset_y)).max(0));
            pad[2] = pad[2].max((blur + i64::from(shadow.offset_x)).max(0));
            pad[3] = pad[3].max((blur + i64::from(shadow.offset_y)).max(0));
        }
        if self.is_empty() { [0; 4] } else { pad }
    }
    pub(crate) fn validate(&self, depth: u8) -> Result<(), DocumentError> {
        let color = |c: &[u16; 4]| depth == 16 || c.iter().all(|v| *v <= 255);
        let range = |v: f64, a: f64, b: f64| v.is_finite() && (a..=b).contains(&v);
        if self.drop_shadow.as_ref().is_some_and(|s| {
            !color(&s.color)
                || !range(s.blur, 0.0, 32.0)
                || s.offset_x.unsigned_abs() > 256
                || s.offset_y.unsigned_abs() > 256
        }) || self
            .stroke
            .as_ref()
            .is_some_and(|s| !color(&s.color) || !range(s.size, 0.0, 64.0))
            || self
                .outer_glow
                .as_ref()
                .is_some_and(|s| !color(&s.color) || !range(s.size, 0.0, 64.0))
            || self
                .color_overlay
                .as_ref()
                .is_some_and(|s| !color(&s.color) || !range(s.opacity, 0.0, 1.0))
            || self.gradient_overlay.as_ref().is_some_and(|s| {
                !color(&s.start)
                    || !color(&s.end)
                    || !s.angle.is_finite()
                    || !range(s.scale, 0.01, 100.0)
                    || !range(s.opacity, 0.0, 1.0)
            })
        {
            return Err(DocumentError::Invalid(
                "style color, radius, offset or opacity out of range".into(),
            ));
        }
        Ok(())
    }
}
pub(crate) fn render(
    doc: &Document,
    layer: &Layer,
    coord: TileCoord,
    layers: usize,
) -> Result<Vec<[f64; 4]>, DocumentError> {
    let _budget = doc
        .memory_pool()
        .reserve(MemoryKind::Transient, 64 * 1024 * 1024)?;
    let styles = &layer.info.styles;
    let pad = styles.outset();
    let halo = *pad.iter().max().unwrap_or(&0) as usize;
    let side = 256 + halo * 2;
    let [ox, oy] = coord.origin();
    let left = ox
        .checked_sub(halo as i64)
        .ok_or_else(|| DocumentError::Invalid("style coordinate overflow".into()))?;
    let top = oy
        .checked_sub(halo as i64)
        .ok_or_else(|| DocumentError::Invalid("style coordinate overflow".into()))?;
    let right = ox
        .checked_add(256 + halo as i64)
        .ok_or_else(|| DocumentError::Invalid("style coordinate overflow".into()))?;
    let bottom = oy
        .checked_add(256 + halo as i64)
        .ok_or_else(|| DocumentError::Invalid("style coordinate overflow".into()))?;
    let (first, _) = TileCoord::locate(left, top);
    let (last, _) = TileCoord::locate(right - 1, bottom - 1);
    let mut isolated = doc.clone();
    let retained: std::collections::HashSet<_> = doc
        .layers
        .iter()
        .take(layers)
        .filter(|l| l.id() == layer.id() || doc.is_descendant(l.id(), layer.id()))
        .map(Layer::id)
        .collect();
    isolated.layers.retain(|l| retained.contains(&l.id()));
    let root = isolated
        .layers
        .iter_mut()
        .find(|l| l.id() == layer.id())
        .unwrap();
    root.info.parent = None;
    root.info.styles = LayerStyles::default();
    root.info.opacity = 1.0;
    root.info.fill_opacity = 1.0;
    root.info.blend_mode = BlendMode::Normal;
    root.info.clipped = false;
    root.info.visible = true;
    let mut alpha = vec![0.0f64; side * side];
    let mut center = vec![[0.0; 4]; 256 * 256];
    for tx in first.x()..=last.x() {
        for ty in first.y()..=last.y() {
            let c = TileCoord::new(tx, ty)?;
            let values =
                crate::composite::compose_stack(&isolated, c, isolated.layers.len(), None)?;
            let [x0, y0] = c.origin();
            if c == coord {
                center.copy_from_slice(&values);
            }
            for y in 0..256usize {
                let gy = y0 + y as i64;
                if gy < top || gy >= bottom {
                    continue;
                }
                for x in 0..256usize {
                    let gx = x0 + x as i64;
                    if gx < left || gx >= right {
                        continue;
                    }
                    alpha[(gy - top) as usize * side + (gx - left) as usize] =
                        values[y * 256 + x][3];
                }
            }
        }
    }
    let shadow = styles
        .drop_shadow
        .as_ref()
        .map(|s| blur(&alpha, side, s.blur));
    let glow = styles
        .outer_glow
        .as_ref()
        .map(|s| blur(&alpha, side, s.size / 3.0));
    let distance = styles.stroke.as_ref().map(|_| {
        (
            distance_field(&alpha, side, true),
            distance_field(&alpha, side, false),
        )
    });
    let shadow_color = styles
        .drop_shadow
        .as_ref()
        .map(|s| linear_color(doc, s.color))
        .transpose()?;
    let glow_color = styles
        .outer_glow
        .as_ref()
        .map(|s| linear_color(doc, s.color))
        .transpose()?;
    let stroke_color = styles
        .stroke
        .as_ref()
        .map(|s| linear_color(doc, s.color))
        .transpose()?;
    let overlay = styles
        .color_overlay
        .as_ref()
        .map(|s| linear_color(doc, s.color))
        .transpose()?;
    let gradient = if let Some(g) = &styles.gradient_overlay {
        let mut gradient = layer.clone();
        gradient.info.kind = LayerKind::Gradient {
            start: g.start,
            end: g.end,
            angle: g.angle,
            scale: g.scale,
            radial: g.radial,
        };
        gradient.info.styles = LayerStyles::default();
        Some(crate::composite::generated_fill(doc, &gradient, coord)?)
    } else {
        None
    };
    let mut out = vec![[0.0; 4]; 256 * 256];
    for y in 0..256usize {
        for x in 0..256usize {
            let index = y * 256 + x;
            let aindex = (y + halo) * side + x + halo;
            let original_alpha = center[index][3];
            if let (Some(shadow), Some(color), Some(settings)) =
                (&shadow, shadow_color, &styles.drop_shadow)
            {
                let sx = (x + halo) as i64 - i64::from(settings.offset_x);
                let sy = (y + halo) as i64 - i64::from(settings.offset_y);
                let amount = if sx >= 0 && sy >= 0 && sx < side as i64 && sy < side as i64 {
                    shadow[sy as usize * side + sx as usize]
                } else {
                    0.0
                };
                out[index] = over(out[index], color, amount);
            }
            if let (Some(glow), Some(color)) = (&glow, glow_color) {
                out[index] = over(out[index], color, glow[aindex] * (1.0 - original_alpha));
            }
            let mut content = center[index];
            content[3] *= layer.info.fill_opacity;
            if let (Some(color), Some(settings)) = (overlay, &styles.color_overlay) {
                let mixed = crate::blending::color(
                    [content[0], content[1], content[2]],
                    [color[0], color[1], color[2]],
                    settings.blend_mode,
                );
                let amount = color[3] * settings.opacity;
                let amount = if settings.blend_mode == BlendMode::Dissolve {
                    if crate::composite::dissolve_noise(layer.id(), ox + x as i64, oy + y as i64)
                        < amount
                    {
                        1.0
                    } else {
                        0.0
                    }
                } else {
                    amount
                };
                for c in 0..3 {
                    content[c] += (mixed[c] - content[c]) * amount;
                }
            }
            if let (Some(gradient), Some(settings)) = (&gradient, &styles.gradient_overlay) {
                let color = gradient[index];
                let mixed = crate::blending::color(
                    [content[0], content[1], content[2]],
                    [color[0], color[1], color[2]],
                    settings.blend_mode,
                );
                let amount = color[3] * settings.opacity;
                let amount = if settings.blend_mode == BlendMode::Dissolve {
                    if crate::composite::dissolve_noise(layer.id(), ox + x as i64, oy + y as i64)
                        < amount
                    {
                        1.0
                    } else {
                        0.0
                    }
                } else {
                    amount
                };
                for c in 0..3 {
                    content[c] += (mixed[c] - content[c]) * amount;
                }
            }
            out[index] = crate::composite::blend(out[index], content, BlendMode::Normal, 1.0);
            if let (Some((to_inside, to_outside)), Some(color), Some(settings)) =
                (&distance, stroke_color, &styles.stroke)
            {
                let outer = (settings.size + 1.0 - to_inside[aindex].sqrt()).clamp(0.0, 1.0)
                    * (1.0 - original_alpha);
                let inner = (settings.size + 1.0 - to_outside[aindex].sqrt()).clamp(0.0, 1.0)
                    * original_alpha;
                let amount = match settings.position {
                    StrokePosition::Outside => outer,
                    StrokePosition::Inside => inner,
                    StrokePosition::Center => {
                        (settings.size / 2.0 + 1.0 - to_inside[aindex].sqrt()).clamp(0.0, 1.0)
                            * (1.0 - original_alpha)
                            + (settings.size / 2.0 + 1.0 - to_outside[aindex].sqrt())
                                .clamp(0.0, 1.0)
                                * original_alpha
                    }
                };
                out[index] = over(out[index], color, amount);
            }
        }
    }
    Ok(out)
}
fn over(back: [f64; 4], mut color: [f64; 4], amount: f64) -> [f64; 4] {
    color[3] *= amount;
    crate::composite::blend(back, color, BlendMode::Normal, 1.0)
}
fn linear_color(doc: &Document, color: [u16; 4]) -> Result<[f64; 4], DocumentError> {
    let pixels = if doc.bit_depth() == 8 {
        PixelBuffer::Rgba8(color.map(|v| v as u8).to_vec())
    } else {
        PixelBuffer::Rgba16(color.to_vec())
    };
    Ok(strata_color::to_linear_p3(&pixels, 1, 1, doc.working_icc())?[0].map(f64::from))
}
fn blur(source: &[f64], side: usize, sigma: f64) -> Vec<f64> {
    if sigma <= 0.0 {
        return source.to_vec();
    }
    let radius = (sigma * 3.0).ceil() as i32;
    let mut kernel: Vec<_> = (-radius..=radius)
        .map(|i| (-(f64::from(i * i)) / (2.0 * sigma * sigma)).exp())
        .collect();
    let total: f64 = kernel.iter().sum();
    for w in &mut kernel {
        *w /= total;
    }
    let mut horizontal = vec![0.0; source.len()];
    let mut result = vec![0.0; source.len()];
    for y in 0..side {
        for x in 0..side {
            for (i, w) in (-radius..=radius).zip(&kernel) {
                let sx = x as i32 + i;
                if sx >= 0 && sx < side as i32 {
                    horizontal[y * side + x] += source[y * side + sx as usize] * w;
                }
            }
        }
    }
    for y in 0..side {
        for x in 0..side {
            for (i, w) in (-radius..=radius).zip(&kernel) {
                let sy = y as i32 + i;
                if sy >= 0 && sy < side as i32 {
                    result[y * side + x] += horizontal[sy as usize * side + x] * w;
                }
            }
        }
    }
    result
}
fn distance_field(alpha: &[f64], side: usize, inside: bool) -> Vec<f64> {
    let mut first = vec![0.0; alpha.len()];
    let mut result = vec![0.0; alpha.len()];
    for y in 0..side {
        let row: Vec<_> = (0..side)
            .map(|x| {
                if (alpha[y * side + x] >= 0.5) == inside {
                    0.0
                } else {
                    1e12
                }
            })
            .collect();
        let row = distance_line(&row);
        first[y * side..(y + 1) * side].copy_from_slice(&row);
    }
    for x in 0..side {
        let column: Vec<_> = (0..side).map(|y| first[y * side + x]).collect();
        let column = distance_line(&column);
        for y in 0..side {
            result[y * side + x] = column[y];
        }
    }
    result
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
