//! Straightening composes a real document rotation and an optional conservative
//! inscribed artboard crop before publication, producing only one history entry.
use super::*;
use crate::{Document, Frame, geometry::Homography};
fn invalid(value: impl std::fmt::Display) -> CommandError {
    CommandError::Invalid(value.to_string())
}
fn angle(reference: StraightenReference) -> Result<f64, CommandError> {
    let angle = match reference {
        StraightenReference::Angle { angle_degrees } => angle_degrees,
        StraightenReference::Line { start, end, align } => {
            if start
                .into_iter()
                .chain(end)
                .any(|v| !v.is_finite() || v.abs() > 1e12)
            {
                return Err(invalid(
                    "straighten reference points must be finite canvas coordinates inside +/-1e12",
                ));
            }
            let dx = end[0] - start[0];
            let dy = end[1] - start[1];
            if dx.hypot(dy) < 1e-6 {
                return Err(invalid("straighten needs two distinct reference points"));
            }
            let target = match align {
                StraightenAxis::Horizontal => 0.,
                StraightenAxis::Vertical => 90.,
            };
            (target - dy.atan2(dx).to_degrees() + 90.).rem_euclid(180.) - 90.
        }
    };
    if !angle.is_finite() || angle.abs() > 45. {
        return Err(invalid(
            "straighten correction must be inside +/-45 degrees; choose the other line alignment for a steeper reference",
        ));
    }
    Ok(if angle.abs() < 1e-12 { 0. } else { angle })
}
fn inscribed(
    before: &Frame,
    expanded: &Frame,
    transform: &Homography,
    angle: f64,
    sampling: TransformInterpolation,
) -> Result<Frame, CommandError> {
    // Bilinear centers need a half-pixel inward source margin to avoid any
    // interpolation with transparent samples beyond an otherwise opaque image.
    let inset = if matches!(sampling, TransformInterpolation::Bilinear) {
        0.5
    } else {
        0.
    };
    let w = f64::from(before.width) - 2. * inset;
    let h = f64::from(before.height) - 2. * inset;
    if w <= 0. || h <= 0. {
        return Err(invalid(
            "image is too narrow for interpolation-safe automatic crop; disable automatic crop",
        ));
    }
    let (c, s) = (
        angle.to_radians().cos().abs(),
        angle.to_radians().sin().abs(),
    );
    let long = w.max(h);
    let short = w.min(h);
    let (wr, hr) = if short <= 2. * s * c * long || (c - s).abs() < 1e-10 {
        let x = short / 2.;
        if w >= h {
            (x / s, x / c)
        } else {
            (x / c, x / s)
        }
    } else {
        let d = c * c - s * s;
        ((w * c - h * s) / d, (h * c - w * s) / d)
    };
    let center = transform
        .map([
            before.x as f64 + f64::from(before.width) / 2.,
            before.y as f64 + f64::from(before.height) / 2.,
        ])
        .map_err(invalid)?;
    let l = (center[0] - wr / 2.).ceil() as i64;
    let t = (center[1] - hr / 2.).ceil() as i64;
    let r = (center[0] + wr / 2.).floor() as i64;
    let b = (center[1] + hr / 2.).floor() as i64;
    // Exact inverse corner checks guard f64 extrema/rounding and integer edge
    // alignment. Usually the analytical box already passes without shrinking.
    for shrink in 0..8 {
        let (l, t, r, b) = (l + shrink, t + shrink, r - shrink, b - shrink);
        if r <= l || b <= t {
            break;
        }
        let corners = [
            [l as f64, t as f64],
            [r as f64, t as f64],
            [r as f64, b as f64],
            [l as f64, b as f64],
        ];
        let inside = corners.into_iter().all(|point| {
            transform.unmap(point).is_ok_and(|[x, y]| {
                x >= before.x as f64 + inset
                    && y >= before.y as f64 + inset
                    && x <= before.x as f64 + inset + w
                    && y <= before.y as f64 + inset + h
            })
        });
        if inside
            && l >= expanded.x
            && t >= expanded.y
            && r <= expanded.x + i64::from(expanded.width)
            && b <= expanded.y + i64::from(expanded.height)
        {
            return Ok(Frame {
                name: before.name.clone(),
                x: l,
                y: t,
                width: (r - l) as u32,
                height: (b - t) as u32,
            });
        }
    }
    Err(invalid(
        "image is too narrow for a nonempty integer inscribed crop at this angle; disable automatic crop",
    ))
}
pub(super) fn mutate(
    doc: &mut Document,
    id: &str,
    params: Value,
    cancel: &dyn Fn() -> bool,
) -> Result<Option<Value>, CommandError> {
    if id != "image.straighten" {
        return Ok(None);
    }
    if cancel() {
        return Err(CommandError::Expired);
    }
    let p: StraightenImage = parse(params)?;
    let angle = angle(p.reference)?;
    let before = doc.frame.clone();
    let result = |expanded: &Frame, frame: &Frame, changed, cropped| serde_json::json!({"angle_degrees":angle,"previous_frame":before,"expanded_frame":expanded,"frame":frame,"cropped":cropped,"changed":changed,"preserved_external_content":true,"preserved_layer_structure":true});
    if angle == 0. {
        return Ok(Some(result(&before, &before, false, false)));
    }
    // Determine the crop before sampling so impossible geometry fails cheaply.
    let (transform, expanded) = super::rotate_image::layout(&before, angle.rem_euclid(360.))?;
    let crop = if p.crop {
        Some(inscribed(
            &before,
            &expanded,
            &transform,
            angle,
            p.interpolation,
        )?)
    } else {
        None
    };
    super::rotate_image::mutate(
        doc,
        "image.rotate",
        serde_json::json!({"angle_degrees":angle,"interpolation":p.interpolation}),
        cancel,
    )?;
    if let Some(frame) = crop {
        super::canvas::publish_frame(doc, frame, cancel)?;
    }
    if cancel() {
        return Err(CommandError::Expired);
    }
    Ok(Some(result(&expanded, &doc.frame, true, p.crop)))
}
