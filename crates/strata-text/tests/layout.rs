use strata_pixels::memory::{MemoryLimits, MemoryPool};
use strata_text::{FontFamily, TextAlign, TextError, TextLayout, TextSpec, render, verify_fonts};
fn spec(text: &str) -> TextSpec {
    TextSpec {
        text: text.into(),
        font: FontFamily::Cjk,
        font_size: 20.,
        line_height: 1.2,
        tracking: 0.,
        align: TextAlign::Left,
        layout: TextLayout::Point,
    }
}
fn pool() -> MemoryPool {
    MemoryPool::new(MemoryLimits {
        cpu_bytes: 256 * 1024 * 1024,
        gpu_bytes: 0,
        disk_bytes: 128 * 1024 * 1024,
    })
}
#[test]
fn pinned_fonts_have_original_hashes_and_cjk_has_real_ink() {
    assert!(verify_fonts());
    let result = render(&spec("中文"), pool(), &|| false).unwrap();
    assert_eq!(result.metrics.glyph_count, 2);
    assert_eq!(result.metrics.line_count, 1);
    assert_eq!(result.metrics.missing_glyphs, 0);
    assert_eq!(result.metrics.first_baseline, 0.);
    assert!((result.metrics.layout_width - 40.).abs() < 0.01);
    let coverage = result.coverage.unwrap();
    let bounds = coverage.content_bounds().unwrap().unwrap();
    assert!(bounds.y < 0);
    assert!(bounds.width > 20 && bounds.width <= 41);
    assert!(
        coverage
            .tiles()
            .any(|(_, t)| t.unwrap().iter().any(|v| *v > 0 && *v < 65535))
    );
}
#[test]
fn paragraph_wraps_fullwidth_chinese_to_four_lines_and_clips_at_box_height() {
    let mut value = spec("天地玄黄宇宙洪荒");
    value.layout = TextLayout::Paragraph {
        width: 41,
        height: 50,
    };
    let result = render(&value, pool(), &|| false).unwrap();
    assert_eq!(result.metrics.glyph_count, 8);
    assert_eq!(result.metrics.line_count, 4);
    assert!((result.metrics.layout_height - 96.).abs() < 0.01);
    let bounds = result.metrics.ink_bounds.unwrap();
    assert!(bounds.x >= 0 && bounds.y >= 0);
    assert!(bounds.x + i64::from(bounds.width) <= 41);
    assert!(bounds.y + i64::from(bounds.height) <= 50);
}
#[test]
fn font_choice_tracking_alignment_and_repeated_layout_are_deterministic() {
    let mut value = spec("AAA");
    value.font = FontFamily::Sans;
    value.font_size = 32.;
    let p = pool();
    let a = render(&value, p.clone(), &|| false).unwrap();
    value.tracking = 4.;
    let b = render(&value, p.clone(), &|| false).unwrap();
    assert!(b.metrics.layout_width >= a.metrics.layout_width + 8.);
    value.tracking = 0.;
    value.font = FontFamily::Serif;
    let serif = render(&value, p.clone(), &|| false).unwrap();
    assert_ne!(serif.metrics.layout_width, a.metrics.layout_width);
    value.font = FontFamily::Sans;
    value.layout = TextLayout::Paragraph {
        width: 200,
        height: 100,
    };
    let left = render(&value, p.clone(), &|| false)
        .unwrap()
        .metrics
        .ink_bounds
        .unwrap();
    value.align = TextAlign::Right;
    let right = render(&value, p.clone(), &|| false)
        .unwrap()
        .metrics
        .ink_bounds
        .unwrap();
    assert!(right.x > left.x + 100);
    let c = render(&value, p.clone(), &|| false).unwrap();
    let d = render(&value, p, &|| false).unwrap();
    assert_eq!(c.metrics.ink_bounds, d.metrics.ink_bounds);
    let c = c.coverage.unwrap();
    let d = d.coverage.unwrap();
    for (coord, tile) in c.tiles() {
        assert_eq!(&*tile.unwrap(), &*d.encoded_tile(*coord).unwrap().unwrap());
    }
}
#[test]
fn empty_spaces_missing_glyph_invalid_inputs_cancel_and_budget_are_explicit() {
    assert!(
        render(&spec("   \n"), pool(), &|| false)
            .unwrap()
            .coverage
            .is_none()
    );
    assert!(matches!(
        render(&spec("\u{10ffff}"), pool(), &|| false),
        Err(TextError::Invalid(_))
    ));
    let mut bad = spec("hello");
    bad.font_size = f64::NAN;
    assert!(bad.validate().is_err());
    bad.font_size = 20.;
    bad.tracking = -11.;
    assert!(bad.validate().is_err());
    assert!(matches!(
        render(&spec("中文"), pool(), &|| true),
        Err(TextError::Cancelled)
    ));
    let p = MemoryPool::new(MemoryLimits {
        cpu_bytes: 8 * 1024 * 1024,
        gpu_bytes: 0,
        disk_bytes: 0,
    });
    assert!(render(&spec("中文"), p.clone(), &|| false).is_err());
    assert_eq!(p.stats().transient_bytes, 0);
}
