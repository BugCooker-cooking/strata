//! Fixed-font, portable editable typography. No system font lookup or paths.
//! The caller owns color/ICC, pixel publication, metadata and document history.
use cosmic_text::{
    Attrs, Buffer, Fallback, Family, FontSystem, Metrics, Shaping, SwashCache, SwashContent, Wrap,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, sync::Arc};
use strata_pixels::{
    CoverageRect, MaskLayout, TileCoord, TiledMask,
    memory::{MemoryKind, MemoryPool},
};

pub const VERSION: u32 = 1;
const MAX_GLYPHS: usize = 16384;
const MAX_VISITS: u64 = 32_000_000;
const FONTS: [(&str, &[u8], &str); 3] = [
    (
        "Noto Sans",
        include_bytes!("../assets/fonts/NotoSans-Regular.ttf"),
        "b85c38ecea8a7cfb39c24e395a4007474fa5a4fc864f6ee33309eb4948d232d5",
    ),
    (
        "Noto Serif",
        include_bytes!("../assets/fonts/NotoSerif-Regular.ttf"),
        "c8f669ceb2c9c60ccf55198b305e08a997ffca79a38cc7eeb551e643cbe66505",
    ),
    (
        "Noto Sans CJK SC",
        include_bytes!("../assets/fonts/NotoSansCJKsc-Regular.otf"),
        "2c76254f6fc379fddfce0a7e84fb5385bb135d3e399294f6eeb6680d0365b74b",
    ),
];
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FontFamily {
    #[default]
    Sans,
    Serif,
    Cjk,
}
impl FontFamily {
    pub fn name(self) -> &'static str {
        match self {
            Self::Sans => "Noto Sans",
            Self::Serif => "Noto Serif",
            Self::Cjk => "Noto Sans CJK SC",
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TextAlign {
    #[default]
    Left,
    Center,
    Right,
    Justified,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum TextLayout {
    Point,
    Paragraph {
        #[schemars(range(min = 1, max = 100000))]
        width: u32,
        #[schemars(range(min = 1, max = 100000))]
        height: u32,
    },
}
fn size() -> f64 {
    32.
}
fn leading() -> f64 {
    1.2
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TextSpec {
    #[schemars(length(max = 16384))]
    pub text: String,
    #[serde(default)]
    pub font: FontFamily,
    #[serde(default = "size")]
    #[schemars(range(min = 1, max = 2048))]
    pub font_size: f64,
    /// Relative line-box height in em, independent of the chosen glyph font.
    #[serde(default = "leading")]
    #[schemars(range(min = 0.5, max = 10))]
    pub line_height: f64,
    /// Additional cluster spacing in canvas pixels, converted to em for shaping.
    #[serde(default)]
    #[schemars(range(min=-256,max=2048))]
    pub tracking: f64,
    #[serde(default)]
    pub align: TextAlign,
    pub layout: TextLayout,
}
impl TextSpec {
    pub fn validate(&self) -> Result<(), TextError> {
        if self.text.len() > 65536
            || self.text.chars().count() > MAX_GLYPHS
            || self.text.contains('\0')
            || self
                .text
                .chars()
                .any(|c| c.is_control() && !['\n', '\r', '\t'].contains(&c))
            || !self.font_size.is_finite()
            || !(1.0..=2048.).contains(&self.font_size)
            || !self.line_height.is_finite()
            || !(0.5..=10.).contains(&self.line_height)
            || !self.tracking.is_finite()
            || !(-256.0..=2048.).contains(&self.tracking)
            || self.tracking < -self.font_size * 0.5
        {
            return Err(TextError::Invalid(
                "text length/control characters or font size/line height/tracking out of range"
                    .into(),
            ));
        }
        if let TextLayout::Paragraph { width, height } = self.layout
            && ![width, height].iter().all(|v| (1..=100000).contains(v))
        {
            return Err(TextError::Invalid(
                "paragraph width/height must be finite1..100000px".into(),
            ));
        }
        Ok(())
    }
}
#[derive(Debug, thiserror::Error)]
pub enum TextError {
    #[error("invalid text: {0}")]
    Invalid(String),
    #[error("text work cancelled")]
    Cancelled,
    #[error("text work exceeds resource limit")]
    Limit,
    #[error(transparent)]
    Memory(#[from] strata_pixels::memory::MemoryError),
    #[error(transparent)]
    Tiles(#[from] strata_pixels::TileError),
}
fn check(cancel: &dyn Fn() -> bool) -> Result<(), TextError> {
    if cancel() {
        Err(TextError::Cancelled)
    } else {
        Ok(())
    }
}
struct FixedFallback;
impl Fallback for FixedFallback {
    fn common_fallback(&self) -> &[&'static str] {
        &["Noto Sans CJK SC", "Noto Sans", "Noto Serif"]
    }
    fn forbidden_fallback(&self) -> &[&'static str] {
        &[]
    }
    fn script_fallback(&self, _: unicode_script::Script, _: &str) -> &[&'static str] {
        &["Noto Sans CJK SC", "Noto Sans"]
    }
}
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TextMetrics {
    pub glyph_count: usize,
    pub line_count: usize,
    pub first_baseline: f64,
    pub layout_width: f64,
    pub layout_height: f64,
    pub ink_bounds: Option<CoverageRect>,
    pub missing_glyphs: usize,
    pub fonts_used: Vec<String>,
}
pub struct RenderedText {
    pub coverage: Option<TiledMask>,
    pub metrics: TextMetrics,
}
struct Glyph {
    key: cosmic_text::CacheKey,
    x: i64,
    y: i64,
}
pub fn font_catalog() -> Vec<(&'static str, &'static str)> {
    FONTS.iter().map(|(name, _, sha)| (*name, *sha)).collect()
}
pub fn verify_fonts() -> bool {
    FONTS.iter().all(|(_, data, hash)| {
        Sha256::digest(data)
            .iter()
            .map(|v| format!("{v:02x}"))
            .collect::<String>()
            == *hash
    })
}
/// A private font system per bounded operation keeps cancellation independent
/// of other documents. Embedded source bytes are shared; no host fonts load.
pub fn render(
    spec: &TextSpec,
    pool: MemoryPool,
    cancel: &dyn Fn() -> bool,
) -> Result<RenderedText, TextError> {
    spec.validate()?;
    check(cancel)?;
    static VERIFIED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if !*VERIFIED.get_or_init(verify_fonts) {
        return Err(TextError::Invalid("bundled font checksum mismatch".into()));
    }

    // Includes parsing/shaping caches, all embedded font tables, glyph/run lists
    // and uncached glyph rasterization. It is released after this operation.
    let _engine_budget = pool.reserve(
        MemoryKind::Transient,
        96 * 1024 * 1024 + spec.text.len() as u64 * 512,
    )?;
    let mut db = fontdb::Database::new();
    for (_, data, _) in FONTS {
        check(cancel)?;
        db.load_font_source(fontdb::Source::Binary(Arc::new(data)));
    }
    db.set_sans_serif_family("Noto Sans");
    db.set_serif_family("Noto Serif");
    db.set_monospace_family("Noto Sans CJK SC");
    let mut system =
        FontSystem::new_with_locale_and_db_and_fallback("zh-CN".into(), db, FixedFallback);
    let mut buffer = Buffer::new_empty(Metrics::new(
        spec.font_size as f32,
        (spec.font_size * spec.line_height) as f32,
    ));
    let paragraph = matches!(spec.layout, TextLayout::Paragraph { .. });
    buffer.set_wrap(if paragraph {
        Wrap::WordOrGlyph
    } else {
        Wrap::None
    });
    if let TextLayout::Paragraph { width, .. } = spec.layout {
        buffer.set_size(Some(width as f32), None)
    }
    let align = match spec.align {
        TextAlign::Left => cosmic_text::Align::Left,
        TextAlign::Center => cosmic_text::Align::Center,
        TextAlign::Right => cosmic_text::Align::Right,
        TextAlign::Justified => cosmic_text::Align::Justified,
    };
    let attrs = Attrs::new()
        .family(Family::Name(spec.font.name()))
        .letter_spacing((spec.tracking / spec.font_size) as f32);
    buffer.set_text(
        &spec.text,
        &attrs,
        Shaping::Advanced,
        Some(if paragraph {
            align
        } else {
            cosmic_text::Align::Left
        }),
    );
    check(cancel)?;
    buffer.shape_until_scroll(&mut system, false);
    check(cancel)?;
    let first = buffer.layout_runs().next().map_or(0., |r| r.line_y);
    let origin_y = if paragraph { 0. } else { first };
    let mut glyphs = Vec::new();
    let mut font_ids = BTreeSet::new();
    let mut lines = 0;
    let mut width = 0f64;
    let mut height = 0f64;
    let mut missing = 0;
    for run in buffer.layout_runs() {
        check(cancel)?;
        lines += 1;
        width = width.max(f64::from(run.line_w));
        height = height.max(f64::from(run.line_top + run.line_height));
        let point_x = if paragraph {
            0.
        } else {
            match spec.align {
                TextAlign::Left | TextAlign::Justified => 0.,
                TextAlign::Center => -run.line_w * 0.5,
                TextAlign::Right => -run.line_w,
            }
        };
        for glyph in run.glyphs {
            if glyphs.len() >= MAX_GLYPHS {
                return Err(TextError::Limit);
            }
            missing += usize::from(glyph.glyph_id == 0);
            font_ids.insert(glyph.font_id);
            let physical = glyph.physical((point_x, run.line_y - origin_y), 1.);
            glyphs.push(Glyph {
                key: physical.cache_key,
                x: i64::from(physical.x),
                y: i64::from(physical.y),
            });
        }
    }
    if width > 100000. || height > 100000. || lines > MAX_GLYPHS {
        return Err(TextError::Limit);
    }
    if missing > 0 {
        return Err(TextError::Invalid(format!(
            "{missing} glyphs are not present in the bundled fonts; choose supported characters or a future imported font"
        )));
    }
    let mut metrics = TextMetrics {
        glyph_count: glyphs.len(),
        line_count: lines,
        first_baseline: if paragraph { f64::from(first) } else { 0. },
        layout_width: width,
        layout_height: height,
        ink_bounds: None,
        missing_glyphs: missing,
        fonts_used: font_ids
            .into_iter()
            .filter_map(|id| {
                system
                    .db()
                    .face(id)
                    .and_then(|f| f.families.first().map(|x| x.0.clone()))
            })
            .collect(),
    };
    let mut cache = SwashCache::new();
    // First pass checks finite ink extent/visit budgets without storing a whole
    // dense image. Second pass publishes sparse u16 coverage one glyph at a time.
    let mut bounds: Option<[i64; 4]> = None;
    let mut visits = 0u64;
    for g in &glyphs {
        check(cancel)?;
        if let Some(image) = cache.get_image_uncached(&mut system, g.key) {
            let b = glyph_bounds(g, &image);
            visits = visits
                .checked_add(u64::from(image.placement.width) * u64::from(image.placement.height))
                .ok_or(TextError::Limit)?;
            if visits > MAX_VISITS {
                return Err(TextError::Limit);
            }
            if let Some(b) = clip(b, &spec.layout) {
                bounds = Some(bounds.map_or(b, |a| {
                    [
                        a[0].min(b[0]),
                        a[1].min(b[1]),
                        a[2].max(b[2]),
                        a[3].max(b[3]),
                    ]
                }));
            }
        }
    }
    let Some([x, y, r, b]) = bounds else {
        return Ok(RenderedText {
            coverage: None,
            metrics,
        });
    };
    let ink = CoverageRect {
        x,
        y,
        width: u32::try_from(r - x).map_err(|_| TextError::Limit)?,
        height: u32::try_from(b - y).map_err(|_| TextError::Limit)?,
    };
    if u64::from(ink.width) * u64::from(ink.height) > MAX_VISITS
        || ink.width > 100000
        || ink.height > 100000
    {
        return Err(TextError::Limit);
    }
    let mut mask = TiledMask::new(
        MaskLayout {
            bounds: ink,
            default_coverage: 0,
            inverted: false,
        },
        1024,
        pool.clone(),
    )?;
    for g in &glyphs {
        check(cancel)?;
        let Some(image) = cache.get_image_uncached(&mut system, g.key) else {
            continue;
        };
        if !matches!(image.content, SwashContent::Mask) {
            return Err(TextError::Invalid(
                "bundled glyph raster must be alpha coverage".into(),
            ));
        }
        let b = glyph_bounds(g, &image);
        let Some([left, top, right, bottom]) = clip(b, &spec.layout) else {
            continue;
        };
        let (first, _) = TileCoord::locate(left, top);
        let (last, _) = TileCoord::locate(right - 1, bottom - 1);
        for ty in first.y()..=last.y() {
            for tx in first.x()..=last.x() {
                check(cancel)?;
                let coord = TileCoord::new(tx, ty)?;
                let [ox, oy] = coord.origin();
                let _tile = pool.reserve(MemoryKind::Transient, 256 * 256 * 2)?;
                let mut values = mask.region_tile(ox, oy)?;
                for py in top.max(oy)..bottom.min(oy + 256) {
                    check(cancel)?;
                    for px in left.max(ox)..right.min(ox + 256) {
                        let i = (py - oy) as usize * 256 + (px - ox) as usize;
                        let a = u32::from(
                            image.data[(py - b[1]) as usize * image.placement.width as usize
                                + (px - b[0]) as usize],
                        ) * 257;
                        let old = u32::from(values[i]);
                        values[i] = (a + ((65535 - a) * old + 32767) / 65535) as u16;
                    }
                }
                mask.insert_encoded_tile(coord, values)?;
            }
        }
    }
    check(cancel)?;
    metrics.ink_bounds = Some(ink);
    Ok(RenderedText {
        coverage: Some(mask),
        metrics,
    })
}
fn glyph_bounds(g: &Glyph, image: &cosmic_text::SwashImage) -> [i64; 4] {
    let x = g.x + i64::from(image.placement.left);
    let y = g.y - i64::from(image.placement.top);
    [
        x,
        y,
        x + i64::from(image.placement.width),
        y + i64::from(image.placement.height),
    ]
}
fn clip(mut b: [i64; 4], layout: &TextLayout) -> Option<[i64; 4]> {
    if let TextLayout::Paragraph { width, height } = layout {
        b[0] = b[0].max(0);
        b[1] = b[1].max(0);
        b[2] = b[2].min(i64::from(*width));
        b[3] = b[3].min(i64::from(*height))
    };
    (b[0] < b[2] && b[1] < b[3]).then_some(b)
}
