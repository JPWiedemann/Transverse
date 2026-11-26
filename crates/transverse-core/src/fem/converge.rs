//! Accuracy control without nested grids: solve a sequence of meshes whose element sizes shrink by a
//! constant factor, watch the change of Z0, and extrapolate with the observed convergence order.
//! P1 energies converge monotonically from below, so the differences are a good error indicator.

use super::solve::Lumped;
use crate::control::Ctl;
use crate::error::{TvError, TvResult};

/// Element-size factor between successive levels (node count roughly doubles per level).
pub const LEVEL_FACTOR: f64 = std::f64::consts::FRAC_1_SQRT_2;

/// What a level solve reports.
pub struct LevelRun<T> {
    pub lumped: Lumped,
    pub nodes: usize,
    /// Mesh and fields of this level; only the last one is kept.
    pub payload: T,
}

/// Accuracy information of a converged analysis.
#[derive(Clone, Debug, Default)]
pub struct Level {
    pub nodes: Vec<usize>,
    /// Z0 of every level (before extrapolation).
    pub z0: Vec<f64>,
    /// Estimated relative error of Z0 of the last level: the change between the last two levels divided by
    /// `1 / LEVEL_FACTOR^order - 1`, with the observed order (1 if it cannot be observed).
    pub est_err: f64,
    /// Observed convergence order used for the extrapolation, `None` if the values were returned as they are.
    pub order: Option<f64>,
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

/// Solve level 0, 1, 2, ... with `solve(level_scale, level)` (scale `start_scale`, then times `LEVEL_FACTOR` per level)
/// until Z0 changes by less than `tol_pct / 100` between two levels (at least three levels), then extrapolate
/// c, c0 and G/omega. A level that hits the node limit ends the loop with the levels so far.
pub fn converge<T>(
    tol_pct: f64,
    max_levels: usize,
    start_scale: f64,
    ctl: &Ctl,
    mut solve: impl FnMut(f64, usize) -> TvResult<LevelRun<T>>,
) -> TvResult<(Lumped, T, Level)> {
    let mut runs: Vec<Lumped> = Vec::new();
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
        info.z0.push(run.lumped.z0());
        runs.push(run.lumped);
        last = Some(run.payload);
        let k = info.z0.len();
        if k >= 2 {
            // the change between two levels underestimates the error of the finer one when convergence is slow:
            // the remaining error is the change divided by (1/factor^p - 1) for convergence order p
            let order = extrapolate(&info.z0).map_or(1.0, |(_, p)| p);
            let step = (info.z0[k - 1] - info.z0[k - 2]).abs() / info.z0[k - 1];
            info.est_err = step / (LEVEL_FACTOR.powf(-order) - 1.0);
            ctl.msg(&format!(
                "Mesh level {k} ({} nodes): Z0 = {:.4} Ohm, estimated error {:.3} %",
                run.nodes,
                info.z0[k - 1],
                info.est_err * 100.0
            ));
            if k >= 3 && info.est_err < tol_pct / 100.0 {
                break;
            }
        }
    }
    let Some(payload) = last else { return Err(TvError::Solver("no mesh level could be solved".into())) };
    let mut out = *runs.last().unwrap();
    let pick = |f: fn(&Lumped) -> f64| extrapolate(&runs.iter().map(f).collect::<Vec<_>>());
    if let (Some((c, p)), Some((c0, _)), Some((g, _))) =
        (pick(|l| l.c), pick(|l| l.c0), pick(|l| l.g_over_omega))
    {
        out = Lumped { c, c0, g_over_omega: g };
        info.order = Some(p);
    }
    Ok((out, payload, info))
}

/// Domain extension for open structures: doubled from `m0` until Z0 changes by less than `tol_pct / 400`
/// (relative); `z0_of(margin)` solves a coarse mesh.
pub fn auto_margin_with(
    m0: f64,
    tol_pct: f64,
    ctl: &Ctl,
    mut z0_of: impl FnMut(f64) -> TvResult<f64>,
) -> TvResult<f64> {
    let mut m = m0;
    let mut z_prev = z0_of(m)?;
    for _ in 0..8 {
        let m2 = 2.0 * m;
        ctl.msg(&format!("Domain size check: margin {m2:.3} mm ..."));
        let z = z0_of(m2)?;
        if (z - z_prev).abs() / z < tol_pct / 100.0 / 4.0 {
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

    fn run(z0: f64, c: f64) -> LevelRun<()> {
        // z0 = 1 / (c_light sqrt(c c0)) with c0 = c / 3.5: pick c from z0
        let _ = z0;
        LevelRun { lumped: Lumped { c, c0: c / 3.5, g_over_omega: 0.01 * c }, nodes: 100, payload: () }
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
            Ok(run(0.0, c_limit * (1.0 - 0.05 * scale * scale)))
        })
        .unwrap();
        assert_eq!(calls[0], (1.0, 0));
        assert!((calls[1].0 - LEVEL_FACTOR).abs() < 1e-15);
        assert!(info.nodes.len() >= 3 && info.nodes.len() < 12);
        assert!(info.est_err < 1e-3, "{}", info.est_err);
        assert!(
            (out.c / c_limit - 1.0).abs() < 1e-12,
            "extrapolated to the limit: {}",
            out.c / c_limit - 1.0
        );
        assert!((info.order.unwrap() - 2.0).abs() < 1e-9);
    }

    #[test]
    fn the_node_limit_ends_the_loop_but_only_after_two_levels() {
        let mut n = 0;
        let r = converge(1e-9, 12, 1.0, &Ctl::new(), |_, _| {
            n += 1;
            if n > 3 {
                Err(TvError::MeshTooLarge { nodes: 10, max: 5 })
            } else {
                Ok(run(0.0, 1.8e-10 * (1.0 - 0.01 * n as f64)))
            }
        });
        assert_eq!(r.unwrap().2.nodes.len(), 3);
        let r: TvResult<(Lumped, (), Level)> =
            converge(1.0, 12, 1.0, &Ctl::new(), |_, _| Err(TvError::MeshTooLarge { nodes: 10, max: 5 }));
        assert!(matches!(r, Err(TvError::MeshTooLarge { .. })));
        let ctl = Ctl::new();
        ctl.cancel();
        assert!(matches!(
            converge(1.0, 12, 1.0, &ctl, |_, _| Ok(run(0.0, 1.0e-10))),
            Err(TvError::Cancelled)
        ));
    }

    #[test]
    fn the_margin_is_doubled_until_z0_stops_changing() {
        let z = |m: f64| Ok(50.0 * (1.0 + (-m).exp()));
        let m = auto_margin_with(1.0, 0.5, &Ctl::new(), z).unwrap();
        // |dz| / z < 0.125 % first holds for the step from 8 to 16 (1.8 % from 4 to 8)
        assert_eq!(m, 16.0);
        // a Z0 that never converges returns the largest margin tried
        let m = auto_margin_with(1.0, 0.5, &Ctl::new(), |m| Ok(50.0 + m)).unwrap();
        assert_eq!(m, 256.0);
    }
}
