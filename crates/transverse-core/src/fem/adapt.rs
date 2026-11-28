//! Adaptive refinement: a per-element error indicator, a marking rule, and the loop that repeats solve and
//! local refinement until the impedances have settled. The meshes are nested (marked triangles are split, nothing is moved),
//! so the capacitances fall monotonically towards their limits.

use std::collections::{HashMap, HashSet};

use super::converge::{Level, LevelRun, impedance_change, summary};
use super::element::{Tri, p2_gradients, tri};
use super::field::element_dofs;
use super::solve::{FemInput, Solved};
use crate::EPS0;
use crate::control::Ctl;
use crate::error::{TvError, TvResult};
use crate::matrix::SymMatrix;
use crate::modes::{CapMatrices, impedance_matrix};

/// Squared residual error indicator of every triangle for quadratic elements and the potential `v` with
/// element weights `weight(k)` (the permittivity, or 1), in the energy norm:
///
/// * the interior residual `h^2 * area * (div(w grad V))^2 / w`, and
/// * for every edge between two triangles, half to each, the jump of the normal flux `w dV/dn` across it,
///   `length * integral(jump^2 ds) / w_mean`.
///
/// The exact solution has neither, also not on dielectric interfaces (the normal flux is continuous there
/// although E is not). Edges on conductors are left out: the jump there is the surface charge. So the indicator
/// measures the discretisation error alone, whatever the shapes of the triangles: it is large next to singular
/// conductor edges and where strong fields vary over an element, and small where the field is weak or smooth.
pub fn residual_indicators(
    inp: &FemInput<'_>,
    edges: &[[u32; 2]],
    v: &[f64],
    weight: &dyn Fn(usize) -> f64,
) -> TvResult<Vec<f64>> {
    let dofs = element_dofs(inp, edges)?;
    if v.len() != inp.pts.len() + edges.len() {
        return Err(TvError::InvalidParam("fem: wrong number of potentials".into()));
    }
    let conductor: HashSet<(u32, u32)> =
        inp.edge_cond.iter().map(|(e, _)| (e[0].min(e[1]), e[0].max(e[1]))).collect();
    let geo: Vec<Tri> = inp.tris.iter().map(|&t| tri(inp.pts, t)).collect();
    let gradient = |k: usize, l: [f64; 3]| {
        let gr = p2_gradients(&geo[k], l);
        let mut g = [0.0; 2];
        for i in 0..6 {
            let value = v[dofs[k][i] as usize];
            g[0] += value * gr[i][0];
            g[1] += value * gr[i][1];
        }
        g
    };
    let dist = |a: u32, b: u32| {
        let (p, q) = (inp.pts[a as usize], inp.pts[b as usize]);
        (p[0] - q[0]).hypot(p[1] - q[1])
    };

    let mut eta = vec![0.0; inp.tris.len()];
    // interior edges, found at their second triangle
    let mut first: HashMap<(u32, u32), usize> = HashMap::new();
    for (k, t) in inp.tris.iter().enumerate() {
        // the gradient of a quadratic is linear: its divergence is the sum of its changes along the barycentric
        // coordinates, each in the direction of their gradients
        let corner =
            [gradient(k, [1.0, 0.0, 0.0]), gradient(k, [0.0, 1.0, 0.0]), gradient(k, [0.0, 0.0, 1.0])];
        let (b, c, area) = (geo[k].b, geo[k].c, geo[k].area);
        let div: f64 = (0..3).map(|i| (corner[i][0] * b[i] + corner[i][1] * c[i]) / (2.0 * area)).sum();
        let h = (0..3).map(|m| dist(t[m], t[(m + 1) % 3])).fold(0.0, f64::max);
        eta[k] += h * h * area * weight(k) * div * div;

        for m in 0..3 {
            let (a, b) = (t[(m + 1) % 3], t[(m + 2) % 3]);
            let key = (a.min(b), a.max(b));
            if conductor.contains(&key) {
                continue;
            }
            let Some(&other) = first.get(&key) else {
                first.insert(key, k);
                continue;
            };
            let len = dist(a, b);
            let (pa, pb) = (inp.pts[key.0 as usize], inp.pts[key.1 as usize]);
            let normal = [(pb[1] - pa[1]) / len, (pa[0] - pb[0]) / len];
            // normal flux of triangle q at the fraction s of the way from node key.0 to node key.1
            let flux = |q: usize, s: f64| {
                let tq = inp.tris[q];
                let l = [0, 1, 2].map(|i| {
                    if tq[i] == key.0 {
                        1.0 - s
                    } else if tq[i] == key.1 {
                        s
                    } else {
                        0.0
                    }
                });
                let g = gradient(q, l);
                weight(q) * (g[0] * normal[0] + g[1] * normal[1])
            };
            // the jump is linear along the edge: Simpson's rule integrates its square exactly
            let jump = [0.0, 0.5, 1.0].map(|s| flux(k, s) - flux(other, s));
            let integral = len / 6.0 * (jump[0].powi(2) + 4.0 * jump[1].powi(2) + jump[2].powi(2));
            let term = len * integral / (0.5 * (weight(k) + weight(other)));
            eta[k] += 0.5 * term;
            eta[other] += 0.5 * term;
        }
    }
    Ok(eta)
}

/// The indicators of all solutions (every signal, with dielectrics and in air; the impedances depend on both
/// capacitances), each relative to the energy of its solution.
pub fn error_indicators(inp: &FemInput<'_>, solved: &Solved) -> TvResult<Vec<f64>> {
    let mut sum = vec![0.0; inp.tris.len()];
    for i in 0..solved.v.len() {
        let with = residual_indicators(inp, &solved.edges, &solved.v[i], &|k| inp.er[k])?;
        let air = residual_indicators(inp, &solved.edges, &solved.v0[i], &|_| 1.0)?;
        let (c, c0) = (solved.caps.c.get(i, i), solved.caps.c0.get(i, i));
        for (s, (a, b)) in sum.iter_mut().zip(with.iter().zip(&air)) {
            *s += a * EPS0 / c + b * EPS0 / c0;
        }
    }
    Ok(sum)
}

/// Elements to refine: the largest indicators that together carry `fraction` of the total (Doerfler marking),
/// and at least the fraction `min_count` of all elements.
pub fn mark(eta2: &[f64], fraction: f64, min_count: f64) -> Vec<bool> {
    let total: f64 = eta2.iter().sum();
    let mut order: Vec<usize> = (0..eta2.len()).collect();
    order.sort_by(|&a, &b| eta2[b].total_cmp(&eta2[a]));
    let mut marked = vec![false; eta2.len()];
    let mut sum = 0.0;
    let fewest = (min_count * eta2.len() as f64).ceil() as usize;
    for (n, k) in order.into_iter().enumerate() {
        if sum >= fraction * total && n >= fewest {
            break;
        }
        marked[k] = true;
        sum += eta2[k];
    }
    marked
}

/// Growth of the node count over which the change of Z0 is measured.
const WINDOW: f64 = 1.6;
/// Fewest meshes before the loop may stop: the first ones are too coarse for their changes to mean much.
const MIN_ROUNDS: usize = 4;

/// Estimated relative error of the last impedances. They are compared with those of the latest mesh that had
/// `WINDOW` times fewer nodes (one round can change very little by chance). With the error falling at least like
/// `1 / nodes` (adaptive quadratic elements reach `1 / nodes^2` once the edges are resolved) the remaining error
/// is at most that change divided by `growth - 1`.
fn estimate(zc: &[SymMatrix], nodes: &[usize]) -> f64 {
    let k = zc.len() - 1;
    let ratio = |j: usize| nodes[k] as f64 / nodes[j] as f64;
    let j = (0..k).rev().find(|&j| ratio(j) >= WINDOW).unwrap_or(0);
    impedance_change(&zc[j], &zc[k]).0 / (ratio(j) - 1.0).max(0.1)
}

/// Round 0, 1, 2, ...: `solve(round, previous)` meshes and solves (round 0 gets `None`; later rounds get the
/// payload of the previous one, to refine it) until the estimated error of every entry of the impedance matrix
/// is below `tol_pct / 100`, after at least `MIN_ROUNDS` rounds. A round that hits the node limit ends the loop
/// with the rounds so far.
pub fn converge_adaptive<T>(
    tol_pct: f64,
    max_rounds: usize,
    ctl: &Ctl,
    mut solve: impl FnMut(usize, Option<&T>) -> TvResult<LevelRun<T>>,
) -> TvResult<(CapMatrices, T, Level)> {
    let mut info = Level::default();
    let mut last: Option<(CapMatrices, T)> = None;
    for round in 0..max_rounds.max(3) {
        ctl.check()?;
        let run = match solve(round, last.as_ref().map(|l| &l.1)) {
            Ok(r) => r,
            Err(TvError::MeshTooLarge { .. }) if info.zc.len() >= 2 => {
                ctl.msg("Node limit reached - using the current mesh");
                break;
            }
            Err(e) => return Err(e),
        };
        info.nodes.push(run.nodes);
        info.zc.push(impedance_matrix(&run.caps)?);
        last = Some((run.caps, run.payload));
        let k = info.zc.len();
        if k >= 2 {
            info.est_err = estimate(&info.zc, &info.nodes);
            ctl.msg(&format!(
                "Refinement {k} ({} nodes): {}, estimated error {:.3} %",
                run.nodes,
                summary(&info.zc[k - 1]),
                info.est_err * 100.0
            ));
            if k >= MIN_ROUNDS && info.est_err < tol_pct / 100.0 {
                break;
            }
        }
    }
    let Some((caps, payload)) = last else {
        return Err(TvError::Solver("no mesh could be solved".into()));
    };
    Ok((caps, payload, info))
}

#[cfg(test)]
mod tests {
    use super::super::solve::{Order, solve_signals};
    use super::*;

    #[test]
    fn marking_takes_the_largest_indicators_up_to_the_fraction() {
        let eta = [1.0, 8.0, 0.5, 0.5];
        assert_eq!(mark(&eta, 0.5, 0.0), vec![false, true, false, false]);
        assert_eq!(mark(&eta, 0.85, 0.0), vec![true, true, false, false]);
        assert!(mark(&eta, 1.0, 0.0).iter().all(|m| *m));
        // the minimum count adds the next largest ones
        assert_eq!(mark(&eta, 0.5, 0.5), vec![true, true, false, false]);
    }

    #[test]
    fn the_estimate_compares_with_a_mesh_that_had_clearly_fewer_nodes() {
        let z = |v: &[f64]| v.iter().map(|&v| SymMatrix::from_fn(1, |_, _| v)).collect::<Vec<_>>();
        // doubling nodes: the change over the last doubling
        let e = estimate(&z(&[99.0, 99.5, 99.75]), &[100, 200, 400]);
        assert!((e - 0.25 / 99.75).abs() < 1e-12, "{e}");
        // small steps: the last one is nearly zero by chance, the window still sees the change before it
        let e = estimate(&z(&[99.0, 99.5, 99.7, 99.7]), &[100, 250, 400, 440]);
        assert!(e > 0.1 / 99.7, "{e}");
    }

    /// A strip corner in a box: the indicator is largest next to the singular edge and nothing for an exact
    /// solution, whatever the triangles look like and also across a dielectric interface.
    #[test]
    fn the_indicator_vanishes_for_an_exact_solution_across_an_interface() {
        // two layers (er 4 below y = 1, er 1 above) between plates at y = 0 and y = 2, on a skewed mesh: the
        // potential is linear in each layer, with different slopes, and quadratic elements hold it exactly
        let pts = vec![
            [0.0, 0.0],
            [1.7, 0.0],
            [3.0, 0.0],
            [0.0, 1.0],
            [0.4, 1.0],
            [3.0, 1.0],
            [0.0, 2.0],
            [2.2, 2.0],
            [3.0, 2.0],
        ];
        let tris: Vec<[u32; 3]> =
            vec![[0, 1, 4], [0, 4, 3], [1, 2, 5], [1, 5, 4], [3, 4, 7], [3, 7, 6], [4, 5, 8], [4, 8, 7]];
        let er: Vec<f64> = (0..8).map(|k| if k < 4 { 4.0 } else { 1.0 }).collect();
        let tand = vec![0.0; 8];
        let node_cond: Vec<Option<u8>> = (0..9)
            .map(|k| {
                if k < 3 {
                    Some(1)
                } else if k >= 6 {
                    Some(0)
                } else {
                    None
                }
            })
            .collect();
        let ec: Vec<([u32; 2], u8)> = vec![([0, 1], 1), ([1, 2], 1), ([6, 7], 0), ([7, 8], 0)];
        let inp =
            FemInput { pts: &pts, tris: &tris, er: &er, tand: &tand, node_cond: &node_cond, edge_cond: &ec };
        // the sides of the box are open (no flux): their edges have one triangle only
        let s = solve_signals(&inp, Order::P2, &Ctl::new()).unwrap();
        let eta = residual_indicators(&inp, &s.edges, &s.v[0], &|k| er[k]).unwrap();
        assert!(eta.iter().all(|e| *e < 1e-20), "{eta:?}");
        // the same potential judged with equal permittivities has a kink: a jump at the interface
        let wrong = residual_indicators(&inp, &s.edges, &s.v[0], &|_| 1.0).unwrap();
        assert!(wrong.iter().sum::<f64>() > 1e-2, "{wrong:?}");
    }
}
