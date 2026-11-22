//! Assembly and solution of the Laplace problem on a [`Grid`].
//!
//! Signal nodes are at 1 V, all other conductor nodes at 0 V. The reduced system (free nodes
//! only) is symmetric positive definite and is solved with a sparse Cholesky factorisation.

use faer::linalg::solvers::Solve;
use faer::sparse::{SparseColMat, Triplet};
use faer::{Mat, Side};

use super::grid::{CAT_SIGNAL, Grid};
use crate::control::Ctl;
use crate::error::{TvError, TvResult};

/// Fields and energy sums of one grid, before any extrapolation. Capacitances are per metre.
pub struct LevelFields {
    pub v: Vec<f64>,
    pub v0: Vec<f64>,
    /// Edge list: horizontal edges first (`ny * (nx-1)`), then vertical (`(ny-1) * nx`).
    pub edges: Vec<[u32; 2]>,
    pub coef_d: Vec<f64>,
    pub coef_0: Vec<f64>,
    pub c: f64,
    pub c0: f64,
    /// G / omega [F/m].
    pub g_over_omega: f64,
}

/// Node pairs of all grid edges, in the order used by the edge coefficients.
pub fn edge_list(nx: usize, ny: usize) -> Vec<[u32; 2]> {
    let mut e = Vec::with_capacity(2 * nx * ny);
    for j in 0..ny {
        for i in 0..nx - 1 {
            e.push([(j * nx + i) as u32, (j * nx + i + 1) as u32]);
        }
    }
    for j in 0..ny - 1 {
        for i in 0..nx {
            e.push([(j * nx + i) as u32, ((j + 1) * nx + i) as u32]);
        }
    }
    e
}

/// Edge conductances for a per-cell value (finite-volume 5-point scheme).
pub fn edge_coefficients(x: &[f64], y: &[f64], cell: &[f64]) -> Vec<f64> {
    let (nx, ny) = (x.len(), y.len());
    let cx = nx - 1;
    let dx: Vec<f64> = x.windows(2).map(|w| w[1] - w[0]).collect();
    let dy: Vec<f64> = y.windows(2).map(|w| w[1] - w[0]).collect();
    let mut out = Vec::with_capacity(2 * nx * ny);
    // horizontal edge (i, j) - (i+1, j): cells above and below, weighted by dy/2
    for j in 0..ny {
        for i in 0..cx {
            let mut s = 0.0;
            if j >= 1 {
                s += cell[(j - 1) * cx + i] * dy[j - 1] / 2.0;
            }
            if j < ny - 1 {
                s += cell[j * cx + i] * dy[j] / 2.0;
            }
            out.push(s / dx[i]);
        }
    }
    // vertical edge (i, j) - (i, j+1): cells left and right, weighted by dx/2
    for j in 0..ny - 1 {
        for i in 0..nx {
            let mut s = 0.0;
            if i >= 1 {
                s += cell[j * cx + i - 1] * dx[i - 1] / 2.0;
            }
            if i < nx - 1 {
                s += cell[j * cx + i] * dx[i] / 2.0;
            }
            out.push(s / dy[j]);
        }
    }
    out
}

/// Solve for the node potentials; returns (potentials, sum of coef * dV^2).
fn laplace(grid: &Grid, edges: &[[u32; 2]], coef: &[f64], ctl: &Ctl) -> TvResult<(Vec<f64>, f64)> {
    ctl.check()?;
    ctl.count_solve();
    let n = grid.nodes();
    let vfix = |k: usize| if grid.cat[k] == CAT_SIGNAL { 1.0 } else { 0.0 };

    let mut free_idx = vec![usize::MAX; n];
    let mut nfree = 0;
    for (k, &c) in grid.cat.iter().enumerate() {
        if c == 0 {
            free_idx[k] = nfree;
            nfree += 1;
        }
    }
    if nfree == 0 {
        return Err(TvError::Solver("no free nodes in the grid".into()));
    }

    let mut diag = vec![0.0; nfree];
    let mut rhs = Mat::<f64>::zeros(nfree, 1);
    let mut trip = Vec::with_capacity(edges.len() + nfree);
    for (&[a, b], &c) in edges.iter().zip(coef) {
        let (a, b) = (a as usize, b as usize);
        let (fa, fb) = (free_idx[a], free_idx[b]);
        match (fa != usize::MAX, fb != usize::MAX) {
            (true, true) => {
                diag[fa] += c;
                diag[fb] += c;
                trip.push(Triplet::new(fa.max(fb), fa.min(fb), -c)); // lower triangle
            }
            (true, false) => {
                diag[fa] += c;
                rhs[(fa, 0)] += c * vfix(b);
            }
            (false, true) => {
                diag[fb] += c;
                rhs[(fb, 0)] += c * vfix(a);
            }
            (false, false) => {}
        }
    }
    trip.extend(diag.iter().enumerate().map(|(k, &d)| Triplet::new(k, k, d)));

    let a = SparseColMat::<usize, f64>::try_new_from_triplets(nfree, nfree, &trip)
        .map_err(|e| TvError::Solver(format!("matrix assembly: {e:?}")))?;
    let llt =
        a.sp_cholesky(Side::Lower).map_err(|e| TvError::Solver(format!("Cholesky factorisation: {e:?}")))?;
    llt.solve_in_place(rhs.as_mut());

    let mut x: Vec<f64> = (0..n).map(vfix).collect();
    for k in 0..n {
        if free_idx[k] != usize::MAX {
            x[k] = rhs[(free_idx[k], 0)];
        }
    }
    let w: f64 = edges
        .iter()
        .zip(coef)
        .map(|(&[a, b], &c)| {
            let dv = x[a as usize] - x[b as usize];
            c * dv * dv
        })
        .sum();
    Ok((x, w))
}

/// Solve one grid: dielectric solution, air solution, C, C0 and G/omega.
pub fn solve_grid(grid: &Grid, ctl: &Ctl) -> TvResult<LevelFields> {
    let (nx, ny) = (grid.nx(), grid.ny());
    let edges = edge_list(nx, ny);
    let coef_d = edge_coefficients(&grid.x, &grid.y, &grid.er);
    let ones = vec![1.0; grid.er.len()];
    let coef_0 = edge_coefficients(&grid.x, &grid.y, &ones);
    let er_td: Vec<f64> = grid.er.iter().zip(&grid.tand).map(|(e, t)| e * t).collect();
    let coef_t = edge_coefficients(&grid.x, &grid.y, &er_td);

    let (v, w) = laplace(grid, &edges, &coef_d, ctl)?;
    let (v0, w0) = laplace(grid, &edges, &coef_0, ctl)?;
    let gw: f64 = edges
        .iter()
        .zip(&coef_t)
        .map(|(&[a, b], &c)| {
            let dv = v[a as usize] - v[b as usize];
            c * dv * dv
        })
        .sum();
    Ok(LevelFields {
        v,
        v0,
        edges,
        coef_d,
        coef_0,
        c: crate::EPS0 * w,
        c0: crate::EPS0 * w0,
        g_over_omega: crate::EPS0 * gw,
    })
}
