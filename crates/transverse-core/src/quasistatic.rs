//! Mesh levels, Richardson extrapolation, auto mesh and automatic domain size.
//!
//! The geometry comes from a [`GridSource`] (implemented by a line type), so this module
//! knows nothing about specific conductors.

use serde::{Deserialize, Serialize};

use crate::control::Ctl;
use crate::error::{TvError, TvResult};
use crate::fd2d::{Grid, solve_grid};
use crate::{C_LIGHT, fd2d};

/// A geometry that can be turned into a tensor grid at any resolution.
pub trait GridSource {
    /// Grid lines for cell sizes `dmin` (at conductor edges) .. `dmax` and domain extension `margin`.
    fn axes(&self, dmin: f64, dmax: f64, margin: f64) -> (Vec<f64>, Vec<f64>);
    /// Node categories and cell materials for the given grid lines.
    fn fill(&self, x: Vec<f64>, y: Vec<f64>, margin: f64) -> Grid;
    /// Default (dmin, dmax) of the first auto-mesh level.
    fn base_sizes(&self) -> (f64, f64);
    /// Fully enclosed by conductors: no domain extension needed.
    fn is_closed(&self) -> bool;
    /// Starting margin of the domain-size check for open structures.
    fn initial_margin(&self) -> f64;
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub enum MeshSpec {
    /// Refine until the estimated error is below `FdOptions::tol_pct`.
    Auto,
    /// Fixed cell sizes [mm], optionally with a second half-size level + extrapolation.
    Manual { dmin: f64, dmax: f64, richardson: bool },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FdOptions {
    pub mesh: MeshSpec,
    /// Target relative error [%] (auto mesh, domain size).
    pub tol_pct: f64,
    pub max_nodes: usize,
    /// Domain extension for open structures [mm]; 0 = automatic.
    pub margin: f64,
}

/// Mesh and error information of a solution.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Accuracy {
    pub nodes: usize,
    pub nx: usize,
    pub ny: usize,
    /// Smallest actual cell [mm].
    pub dmin_actual: f64,
    /// Z0 of the two finest levels (before extrapolation).
    pub levels: Option<(f64, f64)>,
    /// Relative difference of the two finest levels.
    pub est_err: Option<f64>,
    /// (dmin, dmax, two levels) of the finest level used.
    pub mesh_used: (f64, f64, bool),
    /// Domain extension [mm] (0 for closed structures).
    pub margin: f64,
}

/// Result of a quasi-static solve. Capacitances per metre.
pub struct Solution {
    pub grid: Grid,
    pub v: Vec<f64>,
    pub v0: Vec<f64>,
    pub edges: Vec<[u32; 2]>,
    pub coef_d: Vec<f64>,
    pub coef_0: Vec<f64>,
    pub c: f64,
    pub c0: f64,
    /// G / omega [F/m].
    pub g_over_omega: f64,
    pub z0: f64,
    pub eeff: f64,
    /// Inductance per metre.
    pub l: f64,
    pub accuracy: Accuracy,
}

impl Solution {
    fn new(grid: Grid, f: fd2d::LevelFields, accuracy: Accuracy) -> Self {
        let mut s = Self {
            grid,
            v: f.v,
            v0: f.v0,
            edges: f.edges,
            coef_d: f.coef_d,
            coef_0: f.coef_0,
            c: f.c,
            c0: f.c0,
            g_over_omega: f.g_over_omega,
            z0: 0.0,
            eeff: 0.0,
            l: 0.0,
            accuracy,
        };
        s.update_derived();
        s
    }

    fn update_derived(&mut self) {
        self.eeff = self.c / self.c0;
        self.z0 = 1.0 / (C_LIGHT * (self.c * self.c0).sqrt());
        self.l = 1.0 / (C_LIGHT * C_LIGHT * self.c0);
    }
}

fn solve_level(
    src: &dyn GridSource,
    opt: &FdOptions,
    dmin: f64,
    dmax: f64,
    margin: f64,
    ctl: &Ctl,
) -> TvResult<Solution> {
    ctl.check()?;
    let (x, y) = src.axes(dmin, dmax, margin);
    let n = x.len() * y.len();
    if opt.max_nodes < n {
        return Err(TvError::MeshTooLarge { nodes: n, max: opt.max_nodes });
    }
    let dmin_actual = x
        .windows(2)
        .map(|w| w[1] - w[0])
        .chain(y.windows(2).map(|w| w[1] - w[0]))
        .fold(f64::INFINITY, f64::min);
    let (nx, ny) = (x.len(), y.len());
    let grid = src.fill(x, y, margin);
    let fields = solve_grid(&grid, ctl)?;
    let acc = Accuracy { nodes: n, nx, ny, dmin_actual, ..Default::default() };
    Ok(Solution::new(grid, fields, acc))
}

/// First-order Richardson extrapolation of two levels; fields come from `b`.
fn combine(a: &Solution, b: Solution) -> Solution {
    let ex = |u: f64, v: f64| 2.0 * v - u;
    let mut acc = b.accuracy.clone();
    acc.levels = Some((a.z0, b.z0));
    acc.est_err = Some((b.z0 - a.z0).abs() / b.z0);
    let mut s = b;
    s.c = ex(a.c, s.c);
    s.c0 = ex(a.c0, s.c0);
    s.g_over_omega = ex(a.g_over_omega, s.g_over_omega);
    s.accuracy = acc;
    s.update_derived();
    s
}

/// Domain extension for open structures: doubled until Z0 changes by less than tol/4.
pub fn auto_margin(src: &dyn GridSource, opt: &FdOptions, ctl: &Ctl) -> TvResult<f64> {
    if src.is_closed() {
        return Ok(0.0);
    }
    let (d0, d1) = src.base_sizes();
    let (dmin, dmax) = (2.0 * d0, 2.0 * d1); // coarse mesh is enough for this check
    let mut m = src.initial_margin();
    let mut z_prev = solve_level(src, opt, dmin, dmax, m, ctl)?.z0;
    for _ in 0..8 {
        let m2 = 2.0 * m;
        ctl.msg(&format!("Domain size check: margin {m2:.3} mm ..."));
        let z = solve_level(src, opt, dmin, dmax, m2, ctl)?.z0;
        if (z - z_prev).abs() / z < opt.tol_pct / 100.0 / 4.0 {
            return Ok(m2);
        }
        m = m2;
        z_prev = z;
    }
    ctl.msg("Domain size: not fully converged, using largest margin");
    Ok(m)
}

/// Analysis of one geometry: auto or manual mesh with Richardson extrapolation.
pub fn solve(src: &dyn GridSource, opt: &FdOptions, ctl: &Ctl) -> TvResult<Solution> {
    let m = if opt.margin > 0.0 { opt.margin } else { auto_margin(src, opt, ctl)? };
    let mut sol = match opt.mesh {
        MeshSpec::Manual { dmin, dmax, richardson } => {
            ctl.msg(&format!("Solving (manual mesh, min cell {:.3} um) ...", dmin * 1e3));
            let a = solve_level(src, opt, dmin, dmax, m, ctl)?;
            if !richardson {
                let mut a = a;
                a.accuracy.mesh_used = (dmin, dmax, false);
                a
            } else {
                let b = solve_level(src, opt, dmin / 2.0, dmax / 2.0, m, ctl)?;
                let mut s = combine(&a, b);
                s.accuracy.mesh_used = (dmin, dmax, true);
                s
            }
        }
        MeshSpec::Auto => {
            let (d0, d1) = src.base_sizes();
            let mut q = 1.0;
            ctl.msg("Solving mesh level 1 ...");
            let mut a = solve_level(src, opt, d0 / q, d1 / q, m, ctl)?;
            ctl.msg(&format!("Solving mesh level 2 ({} nodes on level 1) ...", a.accuracy.nodes));
            let mut b = solve_level(src, opt, d0 / (2.0 * q), d1 / (2.0 * q), m, ctl)?;
            let mut lvl = 2;
            while (b.z0 - a.z0).abs() / b.z0 > opt.tol_pct / 100.0 {
                q *= 2.0;
                lvl += 1;
                ctl.msg(&format!(
                    "Refining: mesh level {lvl} (est. error {:.2} % > {} %) ...",
                    (b.z0 - a.z0).abs() / b.z0 * 100.0,
                    opt.tol_pct
                ));
                match solve_level(src, opt, d0 / (2.0 * q), d1 / (2.0 * q), m, ctl) {
                    Ok(c) => {
                        a = b;
                        b = c;
                    }
                    Err(TvError::MeshTooLarge { .. }) => {
                        ctl.msg("Node limit reached - using current level");
                        break;
                    }
                    Err(e) => return Err(e),
                }
            }
            let mut s = combine(&a, b);
            s.accuracy.mesh_used = (d0 / q, d1 / q, true);
            s
        }
    };
    sol.accuracy.margin = m;
    Ok(sol)
}
