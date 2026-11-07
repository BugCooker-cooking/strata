//! Projective geometry for image-space editing, independent of UI and pixels.
//! Commands own publication and history; this module only validates geometry.
pub type Point = [f64; 2];
pub type Quad = [Point; 4];
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GeometryError {
    NonFinite,
    Singular,
    NonConvex,
    Horizon,
    OutOfRange,
}
impl std::fmt::Display for GeometryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::NonFinite => "geometry requires finite coordinates",
            Self::Singular => "transform is singular or numerically unstable",
            Self::NonConvex => "four corners must form a nondegenerate convex cyclic quad",
            Self::Horizon => "projective horizon crosses the source domain",
            Self::OutOfRange => "geometry exceeds canvas coordinate bounds",
        })
    }
}
impl std::error::Error for GeometryError {}
type Matrix = [[f64; 3]; 3];
fn multiply(a: Matrix, b: Matrix) -> Matrix {
    std::array::from_fn(|r| std::array::from_fn(|c| (0..3).map(|k| a[r][k] * b[k][c]).sum()))
}
fn inverse(m: Matrix) -> Result<Matrix, GeometryError> {
    let mut a = [[0.0; 6]; 3];
    for r in 0..3 {
        a[r][..3].copy_from_slice(&m[r]);
        a[r][r + 3] = 1.0;
    }
    for column in 0..3 {
        let pivot = (column..3)
            .max_by(|&a0, &b0| a[a0][column].abs().total_cmp(&a[b0][column].abs()))
            .unwrap();
        if a[pivot][column] == 0.0 {
            return Err(GeometryError::Singular);
        }
        a.swap(column, pivot);
        let value = a[column][column];
        for v in &mut a[column] {
            *v /= value;
        }
        let pivot_row = a[column];
        for (row, values) in a.iter_mut().enumerate() {
            if row != column {
                let factor = values[column];
                for (value, pivot) in values.iter_mut().zip(pivot_row) {
                    *value -= factor * pivot;
                }
            }
        }
    }
    let result = std::array::from_fn(|r| std::array::from_fn(|c| a[r][c + 3]));
    if result.iter().flatten().any(|v| !v.is_finite()) {
        return Err(GeometryError::Singular);
    }
    let product = multiply(m, result);
    if (0..3).any(|r| (0..3).any(|c| (product[r][c] - if r == c { 1.0 } else { 0.0 }).abs() > 1e-7))
    {
        return Err(GeometryError::Singular);
    }
    Ok(result)
}
fn point(matrix: Matrix, p: Point) -> Result<Point, GeometryError> {
    if p.iter().any(|v| !v.is_finite()) {
        return Err(GeometryError::NonFinite);
    }
    let w = matrix[2][0] * p[0] + matrix[2][1] * p[1] + matrix[2][2];
    let scale = (matrix[2][0] * p[0]).abs() + (matrix[2][1] * p[1]).abs() + matrix[2][2].abs();
    if !w.is_finite() || w.abs() <= 1e-12 * scale.max(1e-300) {
        return Err(GeometryError::Horizon);
    }
    let out =
        std::array::from_fn(|r| (matrix[r][0] * p[0] + matrix[r][1] * p[1] + matrix[r][2]) / w);
    if out.iter().any(|v| !v.is_finite()) {
        return Err(GeometryError::NonFinite);
    }
    if out.iter().any(|v| v.abs() > 1e12) {
        return Err(GeometryError::OutOfRange);
    }
    Ok(out)
}
fn normalized(quad: Quad) -> Result<(Quad, Matrix, Matrix), GeometryError> {
    if quad.iter().flatten().any(|v| !v.is_finite()) {
        return Err(GeometryError::NonFinite);
    }
    if quad.iter().flatten().any(|v| v.abs() > 1e12) {
        return Err(GeometryError::OutOfRange);
    }
    let center: Point = std::array::from_fn(|c| quad.iter().map(|p| p[c] / 4.0).sum());
    let rms = (quad
        .iter()
        .map(|p| (p[0] - center[0]).powi(2) + (p[1] - center[1]).powi(2))
        .sum::<f64>()
        / 4.0)
        .sqrt();
    if rms <= 1e-12 {
        return Err(GeometryError::NonConvex);
    }
    let s = std::f64::consts::SQRT_2 / rms;
    let q = quad.map(|p| [(p[0] - center[0]) * s, (p[1] - center[1]) * s]);
    let mut sign = 0.0;
    for i in 0..4 {
        let a = q[i];
        let b = q[(i + 1) % 4];
        let c = q[(i + 2) % 4];
        let cross = (b[0] - a[0]) * (c[1] - b[1]) - (b[1] - a[1]) * (c[0] - b[0]);
        if cross.abs() < 1e-10 || sign != 0.0 && sign * cross < 0.0 {
            return Err(GeometryError::NonConvex);
        }
        sign = cross;
    }
    Ok((
        q,
        [
            [s, 0.0, -center[0] * s],
            [0.0, s, -center[1] * s],
            [0.0, 0.0, 1.0],
        ],
        [
            [1.0 / s, 0.0, center[0]],
            [0.0, 1.0 / s, center[1]],
            [0.0, 0.0, 1.0],
        ],
    ))
}
#[derive(Clone, Copy, Debug)]
pub struct Homography {
    forward: Matrix,
    inverse: Matrix,
}
impl Homography {
    /// Row-major coefficients of the normalized mapping.
    pub fn values(&self) -> [f64; 9] {
        std::array::from_fn(|i| self.forward[i / 3][i % 3])
    }
    /// Apply this mapping first and `next` second.
    pub fn then(&self, next: &Self) -> Result<Self, GeometryError> {
        let result = multiply(next.forward, self.forward);
        Self::new(std::array::from_fn(|i| result[i / 3][i % 3]))
    }
    pub fn new(values: [f64; 9]) -> Result<Self, GeometryError> {
        if values.iter().any(|v| !v.is_finite()) {
            return Err(GeometryError::NonFinite);
        }
        let scale = values.iter().map(|v| v.abs()).fold(0.0, f64::max);
        if scale == 0.0 {
            return Err(GeometryError::Singular);
        }
        // Canonical affine coefficients retain exact signed permutations and
        // integer translations even when the translation dominates the matrix.
        let divisor = if values[6] == 0.0 && values[7] == 0.0 && values[8] != 0.0 {
            values[8]
        } else {
            scale
        };
        let matrix = std::array::from_fn(|r| std::array::from_fn(|c| values[r * 3 + c] / divisor));
        if matrix.iter().flatten().any(|v| !v.is_finite()) {
            return Err(GeometryError::NonFinite);
        }
        let inv = inverse(matrix)?;
        Ok(Self {
            forward: matrix,
            inverse: inv,
        })
    }
    pub fn affine(m: [f64; 6]) -> Result<Self, GeometryError> {
        Self::new([m[0], m[2], m[4], m[1], m[3], m[5], 0.0, 0.0, 1.0])
    }
    fn lattice(&self) -> Option<[i64; 6]> {
        let m = self.forward;
        if m[2] != [0.0, 0.0, 1.0] {
            return None;
        }
        let [a, b, c, d, tx, ty] = [m[0][0], m[1][0], m[0][1], m[1][1], m[0][2], m[1][2]];
        if [a, b, c, d].iter().any(|v| ![-1.0, 0.0, 1.0].contains(v))
            || a.abs() + b.abs() != 1.0
            || c.abs() + d.abs() != 1.0
            || a.abs() + c.abs() != 1.0
            || b.abs() + d.abs() != 1.0
            || [tx, ty].iter().any(|v| v.abs() > 2e12 || v.fract() != 0.0)
        {
            return None;
        }
        Some([a as i64, b as i64, c as i64, d as i64, tx as i64, ty as i64])
    }
    pub fn preserves_lattice(&self) -> bool {
        self.lattice().is_some()
    }
    pub fn integer_translation(&self) -> Option<[i64; 2]> {
        let [a, b, c, d, tx, ty] = self.lattice()?;
        ([a, b, c, d] == [1, 0, 0, 1]).then_some([tx, ty])
    }
    /// Inverse pixel indices for exact axis permutations, using integer math.
    /// Do not round a nearly orthogonal matrix: tiny shears matter at 1e12.
    pub fn inverse_lattice_pixel(&self, pixel: [i64; 2]) -> Option<[i64; 2]> {
        let [a, b, c, d, tx, ty] = self.lattice()?.map(i128::from);
        let x = i128::from(pixel[0]) - tx;
        let y = i128::from(pixel[1]) - ty;
        Some([
            i64::try_from(a * x + b * y + (a + b - 1) / 2).ok()?,
            i64::try_from(c * x + d * y + (c + d - 1) / 2).ok()?,
        ])
    }
    pub fn map(&self, p: Point) -> Result<Point, GeometryError> {
        point(self.forward, p)
    }
    pub fn unmap(&self, p: Point) -> Result<Point, GeometryError> {
        point(self.inverse, p)
    }
    pub fn quad(&self, source: Quad) -> Result<Quad, GeometryError> {
        normalized(source)?;
        let mut sign = 0.0;
        let mut result = [[0.0; 2]; 4];
        for (i, p) in source.into_iter().enumerate() {
            let w = self.forward[2][0] * p[0] + self.forward[2][1] * p[1] + self.forward[2][2];
            if sign != 0.0 && sign * w < 0.0 {
                return Err(GeometryError::Horizon);
            }
            result[i] = self.map(p)?;
            sign = w;
        }
        Ok(result)
    }
    pub fn between(source: Quad, destination: Quad) -> Result<Self, GeometryError> {
        let original = source;
        let original_destination = destination;
        let (source, src, _) = normalized(source)?;
        let (target, _, dst) = normalized(destination)?;
        // Recognize exact grid symmetries before a floating solve at huge offsets.
        if original
            .iter()
            .flatten()
            .chain(original_destination.iter().flatten())
            .all(|v| v.fract() == 0.0)
        {
            let src = original.map(|p| p.map(|v| v as i128));
            let dst = original_destination.map(|p| p.map(|v| v as i128));
            for [a, b, c, d] in [
                [1, 0, 0, 1],
                [-1, 0, 0, 1],
                [1, 0, 0, -1],
                [-1, 0, 0, -1],
                [0, 1, 1, 0],
                [0, -1, 1, 0],
                [0, 1, -1, 0],
                [0, -1, -1, 0],
            ] {
                let tx = dst[0][0] - a * src[0][0] - c * src[0][1];
                let ty = dst[0][1] - b * src[0][0] - d * src[0][1];
                if (0..4).all(|i| {
                    dst[i]
                        == [
                            a * src[i][0] + c * src[i][1] + tx,
                            b * src[i][0] + d * src[i][1] + ty,
                        ]
                }) {
                    let result = Self::affine([
                        a as f64, b as f64, c as f64, d as f64, tx as f64, ty as f64,
                    ])?;
                    result.quad(original)?;
                    return Ok(result);
                }
            }
        }
        let mut rows = [[0.0; 9]; 8];
        for i in 0..4 {
            let [x, y] = source[i];
            let [u, v] = target[i];
            rows[2 * i] = [x, y, 1.0, 0.0, 0.0, 0.0, -u * x, -u * y, u];
            rows[2 * i + 1] = [0.0, 0.0, 0.0, x, y, 1.0, -v * x, -v * y, v];
        }
        for column in 0..8 {
            let pivot = (column..8)
                .max_by(|&a, &b| rows[a][column].abs().total_cmp(&rows[b][column].abs()))
                .unwrap();
            if rows[pivot][column].abs() < 1e-12 {
                return Err(GeometryError::Singular);
            }
            rows.swap(column, pivot);
            let p = rows[column][column];
            for value in &mut rows[column] {
                *value /= p;
            }
            let pivot_row = rows[column];
            for (row, values) in rows.iter_mut().enumerate() {
                if row != column {
                    let factor = values[column];
                    for (value, pivot) in values.iter_mut().zip(pivot_row) {
                        *value -= factor * pivot;
                    }
                }
            }
        }
        let h = [
            [rows[0][8], rows[1][8], rows[2][8]],
            [rows[3][8], rows[4][8], rows[5][8]],
            [rows[6][8], rows[7][8], 1.0],
        ];
        let matrix = multiply(multiply(dst, h), src);
        let result = Self::new(std::array::from_fn(|i| matrix[i / 3][i % 3]))?;
        result.quad(original)?;
        Ok(result)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn close(a: Point, b: Point, tolerance: f64) {
        for c in 0..2 {
            assert!((a[c] - b[c]).abs() <= tolerance, "{a:?} != {b:?}");
        }
    }
    #[test]
    fn affine_identity_translation_rotation_shear_and_flip_have_reversible_centers() {
        for m in [
            [1., 0., 0., 1., 0., 0.],
            [1., 0., 0., 1., -129., 25.],
            [0., 1., -1., 0., 50., 0.],
            [1., 0.2, 0.4, 1., 0., 0.],
            [-1., 0., 0., 1., 16., 0.],
        ] {
            let h = Homography::affine(m).unwrap();
            for p in [[0.5, 0.5], [255.5, -7.5], [1024.5, 6000.5]] {
                let target = [
                    m[0] * p[0] + m[2] * p[1] + m[4],
                    m[1] * p[0] + m[3] * p[1] + m[5],
                ];
                close(h.map(p).unwrap(), target, 1e-9);
                close(h.unmap(target).unwrap(), p, 1e-9);
            }
        }
    }
    #[test]
    fn projective_four_corners_and_internal_roundtrips_match_independent_reference() {
        let source = [[0., 0.], [100., 0.], [100., 80.], [0., 80.]];
        let reference = Homography::new([1.2, 0.1, 7., -0.2, 0.9, 5., 0.001, 0.002, 1.]).unwrap();
        let target = reference.quad(source).unwrap();
        let solved = Homography::between(source, target).unwrap();
        for p in [
            [0., 0.],
            [100., 0.],
            [100., 80.],
            [0., 80.],
            [50., 40.],
            [12.5, 64.5],
        ] {
            let expected = [
                (1.2 * p[0] + 0.1 * p[1] + 7.) / (0.001 * p[0] + 0.002 * p[1] + 1.),
                (-0.2 * p[0] + 0.9 * p[1] + 5.) / (0.001 * p[0] + 0.002 * p[1] + 1.),
            ];
            close(solved.map(p).unwrap(), expected, 1e-8);
            close(solved.unmap(expected).unwrap(), p, 1e-8);
        }
    }
    #[test]
    fn normalized_quads_support_large_offsets_and_reversed_orientation() {
        let source = [
            [1e9, 1e9],
            [1e9 + 100., 1e9],
            [1e9 + 100., 1e9 + 80.],
            [1e9, 1e9 + 80.],
        ];
        let target = [[-20., 5.], [-120., 5.], [-120., 85.], [-20., 85.]];
        let h = Homography::between(source, target).unwrap();
        for (a, b) in source.into_iter().zip(target) {
            close(h.map(a).unwrap(), b, 1e-5);
            close(h.unmap(b).unwrap(), a, 1e-4);
        }
    }
    #[test]
    fn degenerate_nonfinite_horizon_and_bowtie_domains_fail() {
        assert!(Homography::new([0.; 9]).is_err());
        assert!(Homography::affine([1., 0., 0., 0., 0., 0.]).is_err());
        assert!(Homography::new([f64::NAN; 9]).is_err());
        let source = [[0., 0.], [10., 0.], [10., 10.], [0., 10.]];
        assert!(Homography::between(source, [[0., 0.], [10., 10.], [10., 0.], [0., 10.]]).is_err());
        let h = Homography::new([1., 0., 0., 0., 1., 0., 0.2, 0., -1.]).unwrap();
        assert_eq!(h.quad(source), Err(GeometryError::Horizon));
    }
    #[test]
    fn integer_corner_symmetries_at_large_offsets_keep_exact_lattice() {
        let source = [
            [999999999980., -800000000000.],
            [999999999996., -800000000000.],
            [999999999996., -799999999984.],
            [999999999980., -799999999984.],
        ];
        let destination = source.map(|[x, y]| [1999999999980. - x, y]);
        let h = Homography::between(source, destination).unwrap();
        assert!(h.preserves_lattice());
        assert_eq!(
            h.inverse_lattice_pixel([999999999999, -799999999995]),
            Some([999999999980, -799999999995])
        );
        let h = Homography::affine([0., 1., -1., 0., 1e11, -1.7e12]).unwrap();
        assert!(h.preserves_lattice());
        assert_eq!(
            h.inverse_lattice_pixel([899999999999, -800000000000]),
            Some([900000000000, -800000000000])
        );
    }
}
