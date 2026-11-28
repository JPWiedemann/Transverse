//! Small dense symmetric matrices, one row and column per signal conductor.
//!
//! N is the number of signals (2 for a pair), so the algorithms are the plain ones: Cholesky and cyclic Jacobi
//! rotations. They are deterministic and give the same results natively and on wasm.

use serde::{Deserialize, Serialize};

use crate::error::{TvError, TvResult};

/// A symmetric `n` x `n` matrix.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SymMatrix {
    n: usize,
    a: Vec<f64>,
}

impl SymMatrix {
    pub fn zeros(n: usize) -> Self {
        Self { n, a: vec![0.0; n * n] }
    }

    /// From `f(i, j)` for `i <= j`, mirrored.
    pub fn from_fn(n: usize, mut f: impl FnMut(usize, usize) -> f64) -> Self {
        let mut m = Self::zeros(n);
        for i in 0..n {
            for j in i..n {
                m.set(i, j, f(i, j));
            }
        }
        m
    }

    pub fn n(&self) -> usize {
        self.n
    }

    pub fn get(&self, i: usize, j: usize) -> f64 {
        self.a[i * self.n + j]
    }

    /// Sets `(i, j)` and `(j, i)`.
    pub fn set(&mut self, i: usize, j: usize, v: f64) {
        self.a[i * self.n + j] = v;
        self.a[j * self.n + i] = v;
    }

    /// The entries `(i, j)` with `i <= j`.
    pub fn upper(&self) -> impl Iterator<Item = (usize, usize)> + use<> {
        let n = self.n;
        (0..n).flat_map(move |i| (i..n).map(move |j| (i, j)))
    }

    pub fn mul_vec(&self, x: &[f64]) -> Vec<f64> {
        (0..self.n).map(|i| (0..self.n).map(|j| self.get(i, j) * x[j]).sum()).collect()
    }

    /// `x' M y`
    pub fn form(&self, x: &[f64], y: &[f64]) -> f64 {
        dot(x, &self.mul_vec(y))
    }

    /// Eigenvalues and eigenvectors (`vectors[k]` belongs to `values[k]`, orthonormal), in no particular order.
    pub fn eigen(&self) -> (Vec<f64>, Vec<Vec<f64>>) {
        let n = self.n;
        let mut a = self.clone();
        let mut q: Vec<Vec<f64>> = (0..n).map(|k| (0..n).map(|i| f64::from(i == k)).collect()).collect();
        let scale = self.a.iter().fold(0.0f64, |s, v| s.max(v.abs()));
        for _sweep in 0..60 {
            let mut rotated = false;
            for p in 0..n {
                for r in p + 1..n {
                    let apr = a.get(p, r);
                    if apr.abs() <= 1e-15 * scale {
                        continue;
                    }
                    rotated = true;
                    let theta = (a.get(r, r) - a.get(p, p)) / (2.0 * apr);
                    let t = theta.signum() / (theta.abs() + theta.hypot(1.0));
                    let c = 1.0 / t.hypot(1.0);
                    let s = t * c;
                    let (app, arr) = (a.get(p, p), a.get(r, r));
                    for k in (0..n).filter(|&k| k != p && k != r) {
                        let (akp, akr) = (a.get(k, p), a.get(k, r));
                        a.set(k, p, c * akp - s * akr);
                        a.set(k, r, s * akp + c * akr);
                    }
                    a.set(p, p, app - t * apr);
                    a.set(r, r, arr + t * apr);
                    a.set(p, r, 0.0);
                    let (qp, qr) = (q[p].clone(), q[r].clone());
                    for i in 0..n {
                        q[p][i] = c * qp[i] - s * qr[i];
                        q[r][i] = s * qp[i] + c * qr[i];
                    }
                }
            }
            if !rotated {
                break;
            }
        }
        ((0..n).map(|k| a.get(k, k)).collect(), q)
    }

    /// Lower Cholesky factor `l` (row-major rows) with `l l' = self`; fails unless positive definite.
    pub(crate) fn cholesky(&self) -> TvResult<Vec<Vec<f64>>> {
        let n = self.n;
        let mut l = vec![vec![0.0; n]; n];
        for i in 0..n {
            for j in 0..=i {
                let s = self.get(i, j) - dot(&l[i][..j], &l[j][..j]);
                if i == j {
                    if !(s > 0.0 && s.is_finite()) {
                        return Err(TvError::Solver("matrix is not positive definite".into()));
                    }
                    l[i][i] = s.sqrt();
                } else {
                    l[i][j] = s / l[j][j];
                }
            }
        }
        Ok(l)
    }

    /// Inverse of a positive definite matrix.
    pub fn inverse_spd(&self) -> TvResult<Self> {
        let l = self.cholesky()?;
        let columns: Vec<Vec<f64>> = (0..self.n)
            .map(|k| {
                let unit: Vec<f64> = (0..self.n).map(|i| f64::from(i == k)).collect();
                solve_upper(&l, &solve_lower(&l, &unit))
            })
            .collect();
        Ok(Self::from_fn(self.n, |i, j| 0.5 * (columns[j][i] + columns[i][j])))
    }
}

pub(crate) fn dot(x: &[f64], y: &[f64]) -> f64 {
    x.iter().zip(y).map(|(a, b)| a * b).sum()
}

/// `x` with `l x = b`.
pub(crate) fn solve_lower(l: &[Vec<f64>], b: &[f64]) -> Vec<f64> {
    let mut x = b.to_vec();
    for i in 0..x.len() {
        x[i] = (x[i] - dot(&l[i][..i], &x[..i])) / l[i][i];
    }
    x
}

/// `x` with `l' x = b`.
pub(crate) fn solve_upper(l: &[Vec<f64>], b: &[f64]) -> Vec<f64> {
    let mut x = b.to_vec();
    for i in (0..x.len()).rev() {
        let s: f64 = (i + 1..x.len()).map(|k| l[k][i] * x[k]).sum();
        x[i] = (x[i] - s) / l[i][i];
    }
    x
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> SymMatrix {
        let rows = [[4.0, -1.0, 0.5], [-1.0, 3.0, -0.2], [0.5, -0.2, 2.0]];
        SymMatrix::from_fn(3, |i, j| rows[i][j])
    }

    #[test]
    fn the_eigenvectors_are_orthonormal_and_reproduce_the_matrix() {
        let m = sample();
        let (values, vectors) = m.eigen();
        for (k, v) in vectors.iter().enumerate() {
            for (a, b) in m.mul_vec(v).iter().zip(v) {
                assert!((a - values[k] * b).abs() < 1e-13);
            }
            for (j, w) in vectors.iter().enumerate() {
                assert!((dot(v, w) - f64::from(j == k)).abs() < 1e-13);
            }
        }
        let trace: f64 = values.iter().sum();
        assert!((trace - 9.0).abs() < 1e-13);
    }

    #[test]
    fn a_two_by_two_with_equal_diagonals_gives_the_sum_and_difference_vectors() {
        let m = SymMatrix::from_fn(2, |i, j| if i == j { 5.0 } else { -1.0 });
        let (values, vectors) = m.eigen();
        for (l, v) in values.iter().zip(&vectors) {
            assert!((v[0].abs() - v[1].abs()).abs() < 1e-15, "{v:?}");
            let want = if v[0] * v[1] > 0.0 { 4.0 } else { 6.0 };
            assert!((l - want).abs() < 1e-14);
        }
    }

    #[test]
    fn the_inverse_is_an_inverse_and_indefinite_matrices_are_refused() {
        let m = sample();
        let inv = m.inverse_spd().unwrap();
        for i in 0..3 {
            let column: Vec<f64> = (0..3).map(|k| inv.get(k, i)).collect();
            for (j, v) in m.mul_vec(&column).iter().enumerate() {
                assert!((v - f64::from(i == j)).abs() < 1e-14);
            }
        }
        let bad = SymMatrix::from_fn(2, |i, j| if i == j { 1.0 } else { 2.0 });
        assert!(bad.inverse_spd().is_err());
    }
}
