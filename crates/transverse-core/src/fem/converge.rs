//! Accuracy control without nested grids: solve a sequence of meshes whose element sizes shrink by a
//! constant factor, watch the change of the impedances, and extrapolate with the observed convergence order.
//! With several signals every entry of the impedance matrix is watched, relative to `sqrt(Z_ii Z_jj)`.
//! P1 energies converge monotonically from below, so the differences are a good error indicator.

use crate::control::Ctl;
use crate::error::{TvError, TvResult};
use crate::matrix::SymMatrix;
use crate::modes::{CapMatrices, impedance_matrix};

/// Element-size factor between successive levels (node count roughly doubles per level).
pub const LEVEL_FACTOR: f64 = std::f64::consts::FRAC_1_SQRT_2;

/// What a level solve reports.
pub struct LevelRun<T> {
    pub caps: CapMatrices,
    pub nodes: usize,
    /// Mesh and fields of this level; only the last one is kept.
    pub payload: T,
}

/// Accuracy information of a converged analysis.
#[derive(Clone, Debug, Default)]
pub struct Level {
    pub nodes: Vec<usize>,
    /// Impedance matrix of every level (before extrapolation).
    pub zc: Vec<SymMatrix>,
    /// Size factor of every level (relative to the start of the sequence).
    pub scales: Vec<f64>,
    /// Estimated relative error of the impedances of the last level: the change between the last two levels
    /// divided by
    /// `1 / LEVEL_FACTOR^order - 1`, with the observed order (1 if it cannot be observed).
    pub est_err: f64,
    /// Observed convergence order used for the extrapolation, `None` if the values were returned as they are.
    pub order: Option<f64>,
}

impl Level {
    /// Z0 of every level of a line with one signal (of the first signal otherwise).
    pub fn z0(&self) -> Vec<f64> {
        self.zc.iter().map(|z| z.get(0, 0)).collect()
    }
}

/// The largest change of an entry from the impedance matrix `a` to `b`, relative to `sqrt(b_ii b_jj)` (for one
/// signal `|dZ0| / Z0`), and that entry.
pub(super) fn impedance_change(a: &SymMatrix, b: &SymMatrix) -> (f64, (usize, usize)) {
    b.upper()
        .map(|(i, j)| ((b.get(i, j) - a.get(i, j)).abs() / (b.get(i, i) * b.get(j, j)).sqrt(), (i, j)))
        .fold((0.0, (0, 0)), |worst, x| if x.0 > worst.0 { x } else { worst })
}

/// The impedances for a progress message.
pub(super) fn summary(zc: &SymMatrix) -> String {
    if zc.n() == 1 {
        return format!("Z0 = {:.4} Ohm", zc.get(0, 0));
    }
    let own: Vec<String> =
        (0..zc.n()).map(|i| format!("Z{0}{0} = {1:.4} Ohm", i + 1, zc.get(i, i))).collect();
    own.join(", ")
}

/// Extrapolated limit of `x` from the last three levels, if they converge monotonically; also the order.
fn extrapolate(x: &[f64]) -> Option<(f64, f64)> {
    let n = x.len();
    if n < 3 {
        return None;
    }
    let (d1, d0) = (x[n - 2] - x[n - 3], x[n - 1] - x[n - 2]);
    if !(d0 != 0.0 && d1 * d0 > 0.0 && d0.abs() < d1.abs()) {
        return None;
    }
    let p = ((d1 / d0).ln() / (1.0 / LEVEL_FACTOR).ln()).clamp(0.5, 2.0);
    let denom = LEVEL_FACTOR.powf(-p) - 1.0;
    Some((x[n - 1] + d0 / denom, p))
}

/// Every capacitance entry extrapolated from the levels, with the order found for the first one; `None` unless
/// all of them converge monotonically (a mix would not belong to any one solution).
fn extrapolate_all(runs: &[CapMatrices]) -> Option<(CapMatrices, f64)> {
    let mut order = None;
    let mut matrix = |pick: fn(&CapMatrices) -> &SymMatrix| -> Option<SymMatrix> {
        let mut out = pick(runs.last()?).clone();
        for (i, j) in pick(runs.last()?).upper() {
            let (limit, p) = extrapolate(&runs.iter().map(|r| pick(r).get(i, j)).collect::<Vec<_>>())?;
            order.get_or_insert(p);
            out.set(i, j, limit);
        }
        Some(out)
    };
    let out = CapMatrices {
        c: matrix(|m| &m.c)?,
        c0: matrix(|m| &m.c0)?,
        g_over_omega: matrix(|m| &m.g_over_omega)?,
    };
    Some((out, order?))
}

/// Solve level 0, 1, 2, ... with `solve(level_scale, level)` (scale `start_scale`, then times `LEVEL_FACTOR` per level)
/// until no impedance changes by more than the tolerance allows between two levels (at least three levels), then
/// extrapolate c, c0 and G/omega. A level that hits the node limit ends the loop with the levels so far.
pub fn converge<T>(
    tol_pct: f64,
    max_levels: usize,
    start_scale: f64,
    ctl: &Ctl,
    mut solve: impl FnMut(f64, usize) -> TvResult<LevelRun<T>>,
) -> TvResult<(CapMatrices, T, Level)> {
    let mut runs: Vec<CapMatrices> = Vec::new();
    let mut info = Level::default();
    let mut last: Option<T> = None;
    for level in 0..max_levels.max(3) {
        ctl.check()?;
        let scale = start_scale * LEVEL_FACTOR.powi(level as i32);
        let run = match solve(scale, level) {
            Ok(r) => r,
            Err(TvError::MeshTooLarge { .. }) if runs.len() >= 2 => {
                ctl.msg("Node limit reached - using the current level");
                break;
            }
            Err(e) => return Err(e),
        };
        info.nodes.push(run.nodes);
        info.zc.push(impedance_matrix(&run.caps)?);
        info.scales.push(scale);
        runs.push(run.caps);
        last = Some(run.payload);
        let k = info.zc.len();
        if k >= 2 {
            // the change between two levels underestimates the error of the finer one when convergence is slow:
            // the remaining error is the change divided by (1/factor^p - 1) for convergence order p
            let (step, (i, j)) = impedance_change(&info.zc[k - 2], &info.zc[k - 1]);
            let series: Vec<f64> = info.zc.iter().map(|z| z.get(i, j)).collect();
            let order = extrapolate(&series).map_or(1.0, |(_, p)| p);
            info.est_err = step / (LEVEL_FACTOR.powf(-order) - 1.0);
            ctl.msg(&format!(
                "Mesh level {k} ({} nodes): {}, estimated error {:.3} %",
                run.nodes,
                summary(&info.zc[k - 1]),
                info.est_err * 100.0
            ));
            if k >= 3 && info.est_err < tol_pct / 100.0 {
                break;
            }
        }
    }
    let Some(payload) = last else { return Err(TvError::Solver("no mesh level could be solved".into())) };
    let mut out = runs[runs.len() - 1].clone();
    if let Some((limit, p)) = extrapolate_all(&runs) {
        out = limit;
        info.order = Some(p);
    }
    Ok((out, payload, info))
}

/// Smallest relative change of Z0 the domain check can tell apart: each domain size is meshed anew, and the
/// coarse meshes differ by this much even where the domain no longer matters. Asking for less would double the
/// domain until the mesh grows beyond any limit.
const MARGIN_NOISE: f64 = 5e-4;

/// Domain extension for open structures: doubled from `m0` until no impedance changes by more than
/// `tol_pct / 400` (relative), but not less than [`MARGIN_NOISE`]; `zc_of(margin)` solves a coarse mesh.
pub fn auto_margin_with(
    m0: f64,
    tol_pct: f64,
    ctl: &Ctl,
    mut zc_of: impl FnMut(f64) -> TvResult<SymMatrix>,
) -> TvResult<f64> {
    let mut m = m0;
    let mut z_prev = zc_of(m)?;
    for _ in 0..8 {
        let m2 = 2.0 * m;
        ctl.msg(&format!("Domain size check: margin {m2:.3} mm ..."));
        let z = zc_of(m2)?;
        if impedance_change(&z_prev, &z).0 < (tol_pct / 100.0 / 4.0).max(MARGIN_NOISE) {
            return Ok(m2);
        }
        m = m2;
        z_prev = z;
    }
    ctl.msg("Domain size: not fully converged, using largest margin");
    Ok(m)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(v: f64) -> SymMatrix {
        SymMatrix::from_fn(1, |_, _| v)
    }

    fn run(c: f64) -> LevelRun<()> {
        let caps = CapMatrices { c: one(c), c0: one(c / 3.5), g_over_omega: one(0.01 * c) };
        LevelRun { caps, nodes: 100, payload: () }
    }

    #[test]
    fn extrapolation_recovers_the_limit_of_a_second_order_sequence() {
        let h = |k: i32| LEVEL_FACTOR.powi(k);
        let x: Vec<f64> = (0..4).map(|k| 10.0 - 3.0 * h(k) * h(k)).collect();
        let (limit, p) = extrapolate(&x).unwrap();
        assert!((limit - 10.0).abs() < 1e-12 && (p - 2.0).abs() < 1e-12);
        // first order: the order is found
        let y: Vec<f64> = (0..4).map(|k| 4.0 + 2.0 * h(k)).collect();
        let (limit, p) = extrapolate(&y).unwrap();
        assert!((limit - 4.0).abs() < 1e-12 && (p - 1.0).abs() < 1e-12);
        // fewer than three values, non-monotone or non-converging sequences are not extrapolated
        assert!(extrapolate(&x[..2]).is_none());
        assert!(extrapolate(&[1.0, 2.0, 1.5]).is_none());
        assert!(extrapolate(&[1.0, 2.0, 4.0]).is_none());
        assert!(extrapolate(&[1.0, 1.0, 1.0]).is_none());
    }

    #[test]
    fn the_level_loop_stops_at_the_tolerance_and_returns_the_extrapolated_values() {
        let c_limit = 1.8e-10;
        let mut calls = Vec::new();
        let (out, (), info) = converge(0.1, 12, 1.0, &Ctl::new(), |scale, level| {
            calls.push((scale, level));
            // capacitance converges from below, second order in the size
            Ok(run(c_limit * (1.0 - 0.05 * scale * scale)))
        })
        .unwrap();
        assert_eq!(calls[0], (1.0, 0));
        assert!((calls[1].0 - LEVEL_FACTOR).abs() < 1e-15);
        assert!(info.nodes.len() >= 3 && info.nodes.len() < 12);
        assert!(info.est_err < 1e-3, "{}", info.est_err);
        assert!(
            (out.c.get(0, 0) / c_limit - 1.0).abs() < 1e-12,
            "extrapolated to the limit: {}",
            out.c.get(0, 0) / c_limit - 1.0
        );
        assert!((info.order.unwrap() - 2.0).abs() < 1e-9);
    }

    #[test]
    fn the_domain_check_does_not_chase_mesh_noise() {
        // Z0 settles like 1 / margin^2 but every solve is off by +-2e-4: a tight tolerance must still end
        let mut calls = 0;
        let m = auto_margin_with(1.0, 0.001, &Ctl::new(), |m| {
            calls += 1;
            let noise = if calls % 2 == 0 { 2e-4 } else { -2e-4 };
            Ok(one(50.0 * (1.0 - 0.02 / (m * m) + noise)))
        })
        .unwrap();
        assert!(m <= 16.0 && calls <= 5, "margin {m} after {calls} solves");
    }

    #[test]
    fn the_node_limit_ends_the_loop_but_only_after_two_levels() {
        let mut n = 0;
        let r = converge(1e-9, 12, 1.0, &Ctl::new(), |_, _| {
            n += 1;
            if n > 3 {
                Err(TvError::MeshTooLarge { nodes: 10, max: 5 })
            } else {
                Ok(run(1.8e-10 * (1.0 - 0.01 * n as f64)))
            }
        });
        assert_eq!(r.unwrap().2.nodes.len(), 3);
        let r: TvResult<(CapMatrices, (), Level)> =
            converge(1.0, 12, 1.0, &Ctl::new(), |_, _| Err(TvError::MeshTooLarge { nodes: 10, max: 5 }));
        assert!(matches!(r, Err(TvError::MeshTooLarge { .. })));
        let ctl = Ctl::new();
        ctl.cancel();
        assert!(matches!(converge(1.0, 12, 1.0, &ctl, |_, _| Ok(run(1.0e-10))), Err(TvError::Cancelled)));
    }

    #[test]
    fn the_margin_is_doubled_until_z0_stops_changing() {
        let z = |m: f64| Ok(one(50.0 * (1.0 + (-m).exp())));
        let m = auto_margin_with(1.0, 0.5, &Ctl::new(), z).unwrap();
        // |dz| / z < 0.125 % first holds for the step from 8 to 16 (1.8 % from 4 to 8)
        assert_eq!(m, 16.0);
        // a Z0 that never converges returns the largest margin tried
        let m = auto_margin_with(1.0, 0.5, &Ctl::new(), |m| Ok(one(50.0 + m))).unwrap();
        assert_eq!(m, 256.0);
    }

    #[test]
    fn with_two_signals_the_loop_waits_for_the_slowest_entry() {
        // all entries approach their limits with second order, the coupling from much farther away
        let sym = |d: f64, o: f64| SymMatrix::from_fn(2, |i, j| if i == j { d } else { o });
        let coupled = |h: f64| {
            let (own, mutual) = (1.0 - 1e-5 * h * h, 1.0 - 0.3 * h * h);
            let caps = CapMatrices {
                c: sym(1.5e-10 * own, -2.0e-11 * mutual),
                c0: sym(5.0e-11 * own, -9.0e-12 * mutual),
                g_over_omega: sym(2.0e-12 * own, -1.0e-13 * mutual),
            };
            LevelRun { caps, nodes: 100, payload: () }
        };
        let (out, (), info) = converge(0.05, 20, 1.0, &Ctl::new(), |h, _| Ok(coupled(h))).unwrap();
        assert!(info.zc.len() > 5, "the coupling needs several levels: {}", info.zc.len());
        assert!(info.est_err < 5e-4);
        assert!((out.c.get(0, 1) / -2.0e-11 - 1.0).abs() < 1e-9);
        assert!((out.c0.get(0, 1) / -9.0e-12 - 1.0).abs() < 1e-9);
        assert!((out.c.get(0, 0) / 1.5e-10 - 1.0).abs() < 1e-9);
    }
}
