//! The conductor loss checked against a second, independent way of computing it.
//!
//! Both solvers use Wheeler's rule: move all metal surfaces and take the change of Z0 in air. The same
//! quantity is the surface current squared, integrated over all metal surfaces, which moves nothing:
//! `dZ0_air/dn = eta0 * integral(J^2 ds) / I^2 = eta0 eps0^2 / C0^2 * integral(E_n^2 ds)` for 1 V.
//! Comparing the solvers with each other cannot find a mistake in the rule they share; this can.
//!
//! At the 90 degree corners of the copper E^2 grows like `r^(-2/3)`: integrable, but a mesh converges to that
//! integral slowly. So within `R` of a corner the discrete field is replaced by the integral of that power law,
//! fitted on the ring `R..2R`, which holds `2^(1/3) - 1` times as much as the disc inside it.

use std::collections::HashMap;

use transverse_core::fem::{FemInput, corner_gradients};
use transverse_core::{Ctl, EPS0};
use transverse_geom::SIGNAL;
use transverse_lines::cpwg_inner::{
    C_BND, CpwgParams, FemSolution, analyze, analyze_fem, wheeler_dzdn, wheeler_dzdn_fem,
};

/// Impedance of free space [Ohm].
const ETA0: f64 = 376.730_313;

/// `dZ0_air/dn` [Ohm/m] from the air solution `sol`, with the corners handled analytically within `r0` [mm].
/// The grounded box far from an open structure is no conductor and is left out.
fn from_surface_current(p: &CpwgParams, sol: &FemSolution, r0: f64) -> f64 {
    let m = &sol.mesh;
    let volt = |id: u8| if id == SIGNAL { 1.0 } else { 0.0 };
    let (er, tand) = (vec![1.0; m.tris.len()], vec![0.0; m.tris.len()]);
    let fixed: Vec<Option<f64>> = m.node_cond.iter().map(|c| c.map(volt)).collect();
    let fixed_edges: Vec<([u32; 2], f64)> =
        m.edges.iter().map(|e| ([e.a.min(e.b), e.a.max(e.b)], volt(e.id))).collect();
    let inp = FemInput {
        pts: &m.pts,
        tris: &m.tris,
        er: &er,
        tand: &tand,
        fixed: &fixed,
        fixed_edges: &fixed_edges,
    };
    let grad = corner_gradients(&inp, &sol.edges, &sol.v0).unwrap();
    let tags: HashMap<(u32, u32), u8> =
        m.edges.iter().map(|e| ((e.a.min(e.b), e.a.max(e.b)), e.tag)).collect();
    let corners: Vec<[f64; 2]> = [p.w / 2.0, p.w / 2.0 + p.s]
        .iter()
        .flat_map(|&x| [[x, 0.0], [x, p.t], [-x, 0.0], [-x, p.t]])
        .collect();
    let to_corner =
        |q: [f64; 2]| corners.iter().map(|c| (q[0] - c[0]).hypot(q[1] - c[1])).fold(f64::INFINITY, f64::min);

    let (mut outside, mut ring) = (0.0, 0.0);
    for (k, t) in m.tris.iter().enumerate() {
        for i in 0..3 {
            let (a, b) = ((i + 1) % 3, (i + 2) % 3);
            let Some(&tag) = tags.get(&(t[a].min(t[b]), t[a].max(t[b]))) else { continue };
            if tag == C_BND {
                continue;
            }
            let (pa, pb) = (m.pts[t[a] as usize], m.pts[t[b] as usize]);
            let length = (pa[0] - pb[0]).hypot(pa[1] - pb[1]);
            // E^2 over the part u0..u1 of the edge, in V^2/m: the field is linear along it, so Simpson is exact
            let part = |u0: f64, u1: f64| {
                let squared = |u: f64| {
                    let g = [0, 1].map(|c| grad[k][a][c] + u * (grad[k][b][c] - grad[k][a][c]));
                    g[0] * g[0] + g[1] * g[1]
                };
                // V/mm and mm: a factor of 1000 to V/m and m
                (u1 - u0) * length / 6.0
                    * (squared(u0) + 4.0 * squared(0.5 * (u0 + u1)) + squared(u1))
                    * 1000.0
            };
            // the part of the edge farther than `r` from every corner (along an edge at a corner the distance
            // is linear)
            let (ra, rb) = (to_corner(pa), to_corner(pb));
            let beyond = |r: f64| match (ra >= r, rb >= r) {
                (true, true) => part(0.0, 1.0),
                (false, false) => 0.0,
                (false, true) => part((r - ra) / (rb - ra), 1.0),
                (true, false) => part(0.0, (r - ra) / (rb - ra)),
            };
            outside += beyond(r0);
            ring += beyond(r0) - beyond(2.0 * r0);
        }
    }
    let scale = ETA0 * EPS0 * EPS0 / (sol.c0 * sol.c0);
    scale * (outside + ring / (2f64.cbrt() - 1.0))
}

#[test]
fn wheeler_agrees_with_the_integral_of_the_surface_current() {
    let base = CpwgParams::default();
    let cases = [
        ("default stack-up", base.clone()),
        ("narrow strip, 50 Ohm", CpwgParams { w: 0.18, s: 0.15, ..base.clone() }),
        ("tight gap", CpwgParams { w: 0.2, s: 0.075, ..base.clone() }),
        ("wide gap", CpwgParams { w: 0.3, s: 0.5, ..base.clone() }),
        ("35 um copper", CpwgParams { t: 0.035, ..base.clone() }),
        ("no plane 1, no fence 1", CpwgParams { plane1: false, fence1: false, ..base }),
    ];
    for (name, p) in cases {
        // a fine mesh for the integral; the loss itself is taken at the default accuracy, as the apps do
        let fine = CpwgParams { tol: 0.005, ..p.clone() };
        let air = analyze_fem(&fine, &fine.fem_options(), &Ctl::new()).unwrap();
        // 0.5 um is small against the copper thickness, where the power law holds
        let reference = from_surface_current(&p, &air, 0.5e-3);

        let grid = analyze(&p, &Ctl::new()).unwrap();
        let fd = wheeler_dzdn(&p, &grid, &Ctl::new()).unwrap();
        let sol = analyze_fem(&p, &p.fem_options(), &Ctl::new()).unwrap();
        let fem = wheeler_dzdn_fem(&p, &sol, &Ctl::new()).unwrap();
        println!(
            "{name:24} integral {reference:.0}, FD {fd:.0} ({:+.2e}), FEM {fem:.0} ({:+.2e})",
            fd / reference - 1.0,
            fem / reference - 1.0
        );
        // measured: within 3.5e-3; the old rule, which left planes and the gap face in place, was 14 to 22 % low
        assert!((fd / reference - 1.0).abs() < 5e-3, "{name}: FD {fd} vs integral {reference}");
        assert!((fem / reference - 1.0).abs() < 5e-3, "{name}: FEM {fem} vs integral {reference}");
    }
}
