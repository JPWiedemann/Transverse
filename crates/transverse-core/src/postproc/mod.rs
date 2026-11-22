//! Post-processing of nodal fields on the tensor grid: E field, equipotentials, field lines.

mod contour;
mod streamline;

pub use contour::contour_segments;
pub use streamline::{StreamOptions, field_lines};

/// Electric field components and magnitude at the grid nodes (V/mm for 1 V on the signal).
/// NaN inside conductors.
pub struct EField {
    pub ex: Vec<f64>,
    pub ey: Vec<f64>,
    pub e: Vec<f64>,
}

/// Derivative along one axis with numpy `gradient` semantics: second-order central
/// differences on non-uniform coordinates, first-order one-sided at the ends.
fn gradient_1d(f: impl Fn(usize) -> f64, c: &[f64], i: usize) -> f64 {
    let n = c.len();
    if i == 0 {
        (f(1) - f(0)) / (c[1] - c[0])
    } else if i == n - 1 {
        (f(n - 1) - f(n - 2)) / (c[n - 1] - c[n - 2])
    } else {
        let (hs, hd) = (c[i] - c[i - 1], c[i + 1] - c[i]);
        (hs * hs * f(i + 1) - hd * hd * f(i - 1) + (hd * hd - hs * hs) * f(i)) / (hs * hd * (hd + hs))
    }
}

/// (d/dx, d/dy) of a nodal field stored row-major (`j * nx + i`).
pub fn gradient(v: &[f64], x: &[f64], y: &[f64]) -> (Vec<f64>, Vec<f64>) {
    let (nx, ny) = (x.len(), y.len());
    let mut dx = vec![0.0; nx * ny];
    let mut dy = vec![0.0; nx * ny];
    for j in 0..ny {
        for i in 0..nx {
            dx[j * nx + i] = gradient_1d(|k| v[j * nx + k], x, i);
            dy[j * nx + i] = gradient_1d(|k| v[k * nx + i], y, j);
        }
    }
    (dx, dy)
}

/// E = -grad V; `fixed` marks conductor nodes (set to NaN).
pub fn e_field(v: &[f64], x: &[f64], y: &[f64], fixed: &[bool]) -> EField {
    let (gx, gy) = gradient(v, x, y);
    let mut ex: Vec<f64> = gx.iter().map(|g| -g).collect();
    let mut ey: Vec<f64> = gy.iter().map(|g| -g).collect();
    let mut e: Vec<f64> = ex.iter().zip(&ey).map(|(a, b)| a.hypot(*b)).collect();
    for (k, &fx) in fixed.iter().enumerate() {
        if fx {
            ex[k] = f64::NAN;
            ey[k] = f64::NAN;
            e[k] = f64::NAN;
        }
    }
    EField { ex, ey, e }
}

/// Percentile of the finite values (linear interpolation, numpy default).
pub fn percentile(values: impl Iterator<Item = f64>, q: f64) -> Option<f64> {
    let mut v: Vec<f64> = values.filter(|x| x.is_finite()).collect();
    if v.is_empty() {
        return None;
    }
    v.sort_by(|a, b| a.total_cmp(b));
    let pos = q / 100.0 * (v.len() - 1) as f64;
    let (lo, hi) = (pos.floor() as usize, pos.ceil() as usize);
    Some(v[lo] + (v[hi] - v[lo]) * (pos - lo as f64))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid() -> (Vec<f64>, Vec<f64>) {
        (vec![0.0, 0.1, 0.3, 0.7, 1.5, 2.0], vec![-1.0, -0.9, -0.5, 0.0, 0.2])
    }

    #[test]
    fn gradient_is_exact_for_linear_and_quadratic_fields() {
        let (x, y) = grid();
        let (nx, ny) = (x.len(), y.len());
        let v: Vec<f64> = (0..nx * ny).map(|k| 3.0 * x[k % nx] - 2.0 * y[k / nx]).collect();
        let (gx, gy) = gradient(&v, &x, &y);
        assert!(gx.iter().all(|g| (g - 3.0).abs() < 1e-12) && gy.iter().all(|g| (g + 2.0).abs() < 1e-12));
        // central differences on non-uniform grids are exact for quadratics in the interior
        let q: Vec<f64> = (0..nx * ny).map(|k| x[k % nx].powi(2)).collect();
        let (gx, _) = gradient(&q, &x, &y);
        for i in 1..nx - 1 {
            assert!((gx[i] - 2.0 * x[i]).abs() < 1e-12);
        }
    }

    #[test]
    fn e_field_is_nan_in_conductors() {
        let (x, y) = grid();
        let n = x.len() * y.len();
        let mut fixed = vec![false; n];
        fixed[7] = true;
        let f = e_field(&vec![0.0; n], &x, &y, &fixed);
        assert!(f.e[7].is_nan() && f.ex[7].is_nan() && f.e[8] == 0.0);
    }

    #[test]
    fn percentile_matches_numpy() {
        let v = [1.0, 2.0, 3.0, 4.0, 10.0];
        assert_eq!(percentile(v.into_iter(), 50.0), Some(3.0));
        assert!((percentile(v.into_iter(), 99.7).unwrap() - 9.928).abs() < 1e-9);
        assert_eq!(percentile([f64::NAN].into_iter(), 50.0), None);
    }
}
