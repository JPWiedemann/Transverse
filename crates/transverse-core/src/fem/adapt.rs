//! Adaptive refinement: a per-element error indicator from the jump between the element gradient and the
//! recovered (continuous) gradient, a marking rule, and the loop that repeats solve and refine until Z0 has
//! settled. Meshes are regenerated each round with smaller size targets at the marked elements (the mesher
//! takes them as a hint), so no mesh surgery is needed.

use super::converge::{Level, LevelRun};
use super::field::centroid_gradients;
use super::solve::{FemInput, Lumped};
use crate::control::Ctl;
use crate::error::{TvError, TvResult};
use crate::postproc::mesh::recover_nodal;

/// Squared error indicator of every triangle: `er * area * |recovered gradient - element gradient|^2` at the
/// centroid (the Zienkiewicz-Zhu estimator). Large where the field varies quickly compared to the element size,
/// i.e. at singular conductor edges and corners and wherever the mesh is too coarse.
pub fn error_indicators(inp: &FemInput<'_>, edges: &[[u32; 2]], v: &[f64]) -> TvResult<Vec<f64>> {
    let g = centroid_gradients(inp, edges, v)?;
    let nodal = recover_nodal(inp.pts, inp.tris, &g);
    Ok(inp
        .tris
        .iter()
        .zip(&g)
        .zip(inp.er)
        .map(|((t, g), er)| {
            let [a, b, c] = t.map(|i| inp.pts[i as usize]);
            let area = 0.5 * ((b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1])).abs();
            let r = t.map(|i| nodal[i as usize]);
            let (rx, ry) = ((r[0][0] + r[1][0] + r[2][0]) / 3.0, (r[0][1] + r[1][1] + r[2][1]) / 3.0);
            er * area * ((rx - g[0]).powi(2) + (ry - g[1]).powi(2))
        })
        .collect())
}

/// Elements to refine: the largest indicators that together carry `fraction` of the total (Doerfler marking).
pub fn mark(eta2: &[f64], fraction: f64) -> Vec<bool> {
    let total: f64 = eta2.iter().sum();
    let mut order: Vec<usize> = (0..eta2.len()).collect();
    order.sort_by(|&a, &b| eta2[b].total_cmp(&eta2[a]));
    let mut marked = vec![false; eta2.len()];
    let mut sum = 0.0;
    for k in order {
        if sum >= fraction * total {
            break;
        }
        marked[k] = true;
        sum += eta2[k];
    }
    marked
}

/// Relative change of Z0 and the error estimate of the last step: the change times `r / (1 - r)` for the observed
/// ratio `r` of the last two changes (clamped to 0.5..0.85, so a noisy sequence is not trusted).
fn estimate(z: &[f64]) -> f64 {
    let k = z.len();
    let step = |i: usize| (z[i] - z[i - 1]).abs() / z[i];
    let d = step(k - 1);
    if k < 3 {
        return d;
    }
    let prev = step(k - 2);
    let r = if prev > 0.0 { (d / prev).clamp(0.5, 0.85) } else { 0.5 };
    d * r / (1.0 - r)
}

/// Round 0, 1, 2, ...: `solve(round, previous)` meshes and solves (round 0 gets `None`; later rounds get the
/// payload of the previous one, to refine it) until the estimated error of Z0 is below `tol_pct / 100`, after
/// at least three rounds. A round that hits the node limit ends the loop with the rounds so far.
pub fn converge_adaptive<T>(
    tol_pct: f64,
    max_rounds: usize,
    ctl: &Ctl,
    mut solve: impl FnMut(usize, Option<&T>) -> TvResult<LevelRun<T>>,
) -> TvResult<(Lumped, T, Level)> {
    let mut info = Level::default();
    let mut last: Option<(Lumped, T)> = None;
    for round in 0..max_rounds.max(3) {
        ctl.check()?;
        let run = match solve(round, last.as_ref().map(|l| &l.1)) {
            Ok(r) => r,
            Err(TvError::MeshTooLarge { .. }) if info.z0.len() >= 2 => {
                ctl.msg("Node limit reached - using the current mesh");
                break;
            }
            Err(e) => return Err(e),
        };
        info.nodes.push(run.nodes);
        info.z0.push(run.lumped.z0());
        last = Some((run.lumped, run.payload));
        let k = info.z0.len();
        if k >= 2 {
            info.est_err = estimate(&info.z0);
            ctl.msg(&format!(
                "Refinement {k} ({} nodes): Z0 = {:.4} Ohm, estimated error {:.3} %",
                run.nodes,
                info.z0[k - 1],
                info.est_err * 100.0
            ));
            if k >= 3 && info.est_err < tol_pct / 100.0 {
                break;
            }
        }
    }
    let Some((lumped, payload)) = last else {
        return Err(TvError::Solver("no mesh could be solved".into()));
    };
    Ok((lumped, payload, info))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marking_takes_the_largest_indicators_up_to_the_fraction() {
        let eta = [1.0, 8.0, 0.5, 0.5];
        assert_eq!(mark(&eta, 0.5), vec![false, true, false, false]);
        assert_eq!(mark(&eta, 0.85), vec![true, true, false, false]);
        assert!(mark(&eta, 1.0).iter().all(|m| *m));
    }

    #[test]
    fn the_estimate_follows_the_decay_of_the_changes() {
        // changes halve: remaining error equals the last change
        let z = [100.0, 99.0, 98.5, 98.25];
        let e = estimate(&z);
        assert!((e / (0.25 / 98.25) - 1.0).abs() < 1e-2, "{e}");
        // changes that do not decay are not trusted: estimate well above the last change
        let z = [100.0, 99.0, 98.1, 97.3];
        assert!(estimate(&z) > 3.0 * (0.8 / 97.3));
    }
}
