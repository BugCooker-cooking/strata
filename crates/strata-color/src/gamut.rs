//! Constant-luminance chroma compression in the actual working RGB gamut.
use super::*;
#[derive(Clone, Copy)]
struct MatrixGamut {
    origin: [f64; 3],
    inverse: [[f64; 3]; 3],
}
impl MatrixGamut {
    fn coordinates(&self, rgb: [f64; 3]) -> [f64; 3] {
        std::array::from_fn(|r| {
            (0..3)
                .map(|c| self.inverse[r][c] * (rgb[c] - self.origin[c]))
                .sum()
        })
    }
    fn compress(&self, rgb: [f64; 3], neutral: f64) -> Option<[f64; 3]> {
        let base = self.coordinates([neutral; 3]);
        if base.iter().any(|v| !(-1e-5..=1.00001).contains(v)) {
            return None;
        }
        let target = self.coordinates(rgb);
        let mut scale: f64 = 1.0;
        for c in 0..3 {
            let n = base[c].clamp(0.0, 1.0);
            let d = target[c] - base[c];
            if d > 0.0 {
                scale = scale.min((1.0 - n) / d);
            } else if d < 0.0 {
                scale = scale.min(-n / d);
            }
        }
        Some(rgb.map(|v| neutral + (v - neutral) * scale.clamp(0.0, 1.0)))
    }
}
fn matrix(icc: &[u8]) -> Result<Option<MatrixGamut>, ColorError> {
    let profile = Profile::new_icc(icc).map_err(transform_error)?;
    if !profile.is_matrix_shaper()
        || [
            TagSignature::AToB0Tag,
            TagSignature::AToB1Tag,
            TagSignature::AToB2Tag,
            TagSignature::BToA0Tag,
            TagSignature::BToA1Tag,
            TagSignature::BToA2Tag,
            TagSignature::DToB0Tag,
            TagSignature::DToB1Tag,
            TagSignature::DToB2Tag,
            TagSignature::DToB3Tag,
        ]
        .iter()
        .any(|t| profile.has_tag(*t))
    {
        return Ok(None);
    }
    for tag in [
        TagSignature::RedTRCTag,
        TagSignature::GreenTRCTag,
        TagSignature::BlueTRCTag,
    ] {
        let Tag::ToneCurve(curve) = profile.read_tag(tag) else {
            return Ok(None);
        };
        if !curve.is_monotonic() {
            return Ok(None);
        }
    }
    let points = working_to_linear_p3(
        &[
            [0., 0., 0., 1.],
            [1., 0., 0., 1.],
            [0., 1., 0., 1.],
            [0., 0., 1., 1.],
            [1., 1., 1., 1.],
        ],
        icc,
    )?;
    let origin = std::array::from_fn(|c| f64::from(points[0][c]));
    let m: [[f64; 3]; 3] =
        std::array::from_fn(|r| std::array::from_fn(|c| f64::from(points[c + 1][r]) - origin[r]));
    if (0..3).any(|r| (origin[r] + m[r].iter().sum::<f64>() - f64::from(points[4][r])).abs() > 1e-4)
    {
        return Ok(None);
    }
    let [[a, b, c], [d, e, f], [g, h, i]] = m;
    let det = a * (e * i - f * h) - b * (d * i - f * g) + c * (d * h - e * g);
    if !det.is_finite() || det.abs() < 1e-12 {
        return Ok(None);
    }
    let inverse = [
        [e * i - f * h, c * h - b * i, b * f - c * e],
        [f * g - d * i, a * i - c * g, c * d - a * f],
        [d * h - e * g, b * g - a * h, a * e - b * d],
    ]
    .map(|row| row.map(|v| v / det));
    Ok(Some(MatrixGamut { origin, inverse }))
}
/// Fast matrix/TRC bounds for ordinary RGB profiles, with an unquantized ICC
/// round-trip fallback for LUT/nonmonotonic profiles. Callers budget ICC bytes.
pub struct WorkingGamut {
    matrix: Option<MatrixGamut>,
    icc: Vec<u8>,
}
impl WorkingGamut {
    pub fn new(icc: &[u8]) -> Result<Self, ColorError> {
        Ok(Self {
            matrix: matrix(icc)?,
            icc: icc.to_vec(),
        })
    }
    fn contains(&self, rgb: [f64; 3]) -> Result<bool, ColorError> {
        let p = [rgb[0] as f32, rgb[1] as f32, rgb[2] as f32, 1.0];
        let encoded = linear_p3_to_working(&[p], &self.icc)?[0];
        if encoded[..3].iter().any(|v| !v.is_finite()) {
            return Ok(false);
        }
        let encoded = encoded.map(|v| v.clamp(0.0, 1.0));
        let actual = working_to_linear_p3(&[encoded], &self.icc)?[0];
        Ok((0..3).all(|c| (f64::from(actual[c]) - rgb[c]).abs() <= 3e-6))
    }
    /// Compress only chroma along a line to neutral linear P3, preserving the
    /// caller's luminance. A target outside the working neutral range errors.
    pub fn compress(&self, rgb: [f64; 3], neutral: f64) -> Result<[f64; 3], ColorError> {
        if !neutral.is_finite() || rgb.iter().any(|v| !v.is_finite()) {
            return Err(ColorError::Transform("nonfinite gamut input".into()));
        }
        if let Some(matrix) = self.matrix
            && let Some(value) = matrix.compress(rgb, neutral)
        {
            return Ok(value);
        }
        if self.contains(rgb)? {
            return Ok(rgb);
        }
        if !self.contains([neutral; 3])? {
            return Err(ColorError::Transform(
                "requested luminance lies outside the working profile neutral range".into(),
            ));
        }
        let mut low = 0.0;
        let mut high = 1.0;
        for _ in 0..16 {
            let mid = (low + high) / 2.0;
            let value = rgb.map(|v| neutral + (v - neutral) * mid);
            if self.contains(value)? {
                low = mid;
            } else {
                high = mid;
            }
        }
        Ok(rgb.map(|v| neutral + (v - neutral) * low))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn y(p: [f64; 3]) -> f64 {
        0.22897456 * p[0] + 0.69173852 * p[1] + 0.07928692 * p[2]
    }
    #[test]
    fn actual_icc_bounds_keep_luminance_when_srgb_is_smaller_than_p3() {
        for space in ["srgb", "display_p3"] {
            let icc = working_profile(space).unwrap();
            let mapper = WorkingGamut::new(&icc).unwrap();
            assert!(mapper.matrix.is_some());
            for rgb in [[-0.2, 0.7, 0.9], [0.9, 0.1, 0.7], [1.4, 0.3, -0.1]] {
                let neutral = y(rgb);
                let mapped = mapper.compress(rgb, neutral).unwrap();
                assert!((y(mapped) - neutral).abs() < 1e-9);
                let enc = linear_p3_to_working(
                    &[[mapped[0] as f32, mapped[1] as f32, mapped[2] as f32, 1.0]],
                    &icc,
                )
                .unwrap()[0];
                assert!(
                    enc[..3].iter().all(|v| (-0.0001..=1.0001).contains(v)),
                    "{space}: {enc:?}"
                );
                let roundtrip =
                    working_to_linear_p3(&[enc.map(|v| v.clamp(0.0, 1.0))], &icc).unwrap()[0];
                assert!((0..3).all(|c| (f64::from(roundtrip[c]) - mapped[c]).abs() < 1e-5));
            }
        }
    }
    #[test]
    fn generic_floating_roundtrip_fallback_matches_matrix_without_quantization() {
        let icc = working_profile("srgb").unwrap();
        let matrix = WorkingGamut::new(&icc).unwrap();
        let fallback = WorkingGamut { matrix: None, icc };
        for rgb in [[-0.2, 0.7, 0.9], [0.9, 0.1, 0.7]] {
            let neutral = y(rgb);
            let a = matrix.compress(rgb, neutral).unwrap();
            let b = fallback.compress(rgb, neutral).unwrap();
            assert!((0..3).all(|c| (a[c] - b[c]).abs() < 0.0001));
            assert!((y(b) - neutral).abs() < 1e-9);
        }
    }
}
