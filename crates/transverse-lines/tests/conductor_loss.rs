//! The conductor loss checked against a second, independent way of computing it.
//!
//! Both solvers use Wheeler's rule: move all metal surfaces and take the change of Z0 in air. The same
//! quantity is the surface current squared, integrated over all metal surfaces, which moves nothing:
//! `dZ0_air/dn = eta0 * integral(J^2 ds) / I^2 = eta0 eps0^2 / C0^2 * integral(E_n^2 ds)` for 1 V.
//! Comparing the solvers with each other cannot find a mistake in the rule they share; this can.
//!
//! At a corner of the copper with the angle `b` on the side of the field, E^2 grows like `r^(2 pi / b - 2)`
//! (`r^(-2/3)` at 90 degrees, `b = 3 pi / 2`): integrable, but a mesh converges to that integral slowly. So within
//! `R` of a corner the discrete field is replaced by the integral of that power law, fitted on the ring `R..2R`,
//! which holds `2^(2 pi / b - 1) - 1` times as much as the disc inside it. Etched flanks give other angles than
//! 90 degrees, a different one at the wide and at the narrow face.

use std::collections::HashMap;

use transverse_core::fem::{FemInput, corner_gradients};
use transverse_core::{Ctl, EPS0};
use transverse_lines::planar::{
    C_BND, EtchMode, FemSolution, PlanarParams, Signals, WideSide, analyze, analyze_fem, loss_models_fem,
    wheeler_dzdn, wheeler_dzdn_fem,
};

/// Impedance of free space [Ohm].
const ETA0: f64 = 376.730_313;

/// `dZ0_air/dn` [Ohm/m] from the air solution `sol`, with the corners handled analytically within `r0` [mm].
/// The grounded box far from an open structure is no conductor and is left out.
fn from_surface_current(p: &PlanarParams, sol: &FemSolution, r0: f64) -> f64 {
    let m = &sol.mesh;
    let (er, tand) = (vec![1.0; m.tris.len()], vec![0.0; m.tris.len()]);
    let edge_cond: Vec<([u32; 2], u8)> =
        m.edges.iter().map(|e| ([e.a.min(e.b), e.a.max(e.b)], e.id)).collect();
    let inp = FemInput {
        pts: &m.pts,
        tris: &m.tris,
        er: &er,
        tand: &tand,
        node_cond: &m.node_cond,
        edge_cond: &edge_cond,
    };
    let grad = corner_gradients(&inp, &sol.edges, &sol.v0).unwrap();
    let tags: HashMap<(u32, u32), u8> =
        m.edges.iter().map(|e| ((e.a.min(e.b), e.a.max(e.b)), e.tag)).collect();
    // the corners of the copper, at the trace edges and, with a coplanar ground, at the gap edges: each with the
    // exponent `2 pi / b - 1` of the integral of E^2 around it
    let flank = std::f64::consts::PI * p.flank_angle_deg() / 180.0;
    let wide = if p.etch_wide == WideSide::Bottom { 0.0 } else { 1.0 };
    let stack = p.stackup();
    let corners: Vec<([f64; 2], f64)> = [0.0, 1.0]
        .into_iter()
        .flat_map(|f| {
            // the angle inside the copper: the flank angle at the wide face, its supplement at the narrow one
            let inside = if p.undercut() == 0.0 {
                std::f64::consts::FRAC_PI_2
            } else if f == wide {
                flank
            } else {
                std::f64::consts::PI - flank
            };
            let exponent = 2.0 * std::f64::consts::PI / (2.0 * std::f64::consts::PI - inside) - 1.0;
            stack.copper_edges_x_at(f).into_iter().map(move |x| ([x, f * p.t], exponent))
        })
        .collect();
    // the nearest corner: its distance and exponent
    let to_corner = |q: [f64; 2]| {
        corners
            .iter()
            .map(|(c, e)| ((q[0] - c[0]).hypot(q[1] - c[1]), *e))
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .unwrap()
    };

    // the integral far from the corners, and what the discs around them hold
    let (mut outside, mut discs) = (0.0, 0.0);
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
            let ((ra, ea), (rb, eb)) = (to_corner(pa), to_corner(pb));
            let beyond = |r: f64| match (ra >= r, rb >= r) {
                (true, true) => part(0.0, 1.0),
                (false, false) => 0.0,
                (false, true) => part((r - ra) / (rb - ra), 1.0),
                (true, false) => part(0.0, (r - ra) / (rb - ra)),
            };
            outside += beyond(r0);
            let exponent = if ra < rb { ea } else { eb };
            discs += (beyond(r0) - beyond(2.0 * r0)) / (2f64.powf(exponent) - 1.0);
        }
    }
    let scale = ETA0 * EPS0 * EPS0 / (sol.c0 * sol.c0);
    scale * (outside + discs)
}

#[test]
fn wheeler_agrees_with_the_integral_of_the_surface_current() {
    let base = PlanarParams::default();
    let cases = [
        ("default stack-up", base.clone()),
        ("narrow strip, 50 Ohm", PlanarParams { w: 0.18, s: 0.15, ..base.clone() }),
        ("tight gap", PlanarParams { w: 0.2, s: 0.075, ..base.clone() }),
        ("wide gap", PlanarParams { w: 0.3, s: 0.5, ..base.clone() }),
        ("35 um copper", PlanarParams { t: 0.035, ..base.clone() }),
        ("no plane 1, no fence 1", PlanarParams { plane1: false, fence1: false, ..base.clone() }),
        ("stripline", PlanarParams { coplanar: false, w: 0.2, ..base.clone() }),
        (
            "stripline, side walls close by",
            PlanarParams { coplanar: false, side_walls: true, w: 0.25, g: 0.12, t: 0.035, ..base.clone() },
        ),
        ("microstrip", PlanarParams { coplanar: false, upper: false, w: 0.5, t: 0.035, ..base.clone() }),
        ("CPWG, outer layer", PlanarParams { upper: false, w: 0.35, s: 0.15, g: 0.3, ..base }),
    ];
    for (name, p) in cases {
        // a fine mesh for the integral; the loss itself is taken at the default accuracy, as the apps do
        let fine = PlanarParams { tol: 0.001, ..p.clone() };
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
        // measured: within 2.4e-3; the old rule, which left planes and the gap face in place, was 14 to 22 % low
        assert!((fd / reference - 1.0).abs() < 5e-3, "{name}: FD {fd} vs integral {reference}");
        assert!((fem / reference - 1.0).abs() < 5e-3, "{name}: FEM {fem} vs integral {reference}");
    }
}

/// Etched flanks: the grid cannot follow them, so the finite elements stand against the integral alone.
#[test]
fn wheeler_with_etched_flanks_agrees_with_the_integral_of_the_surface_current() {
    let base = PlanarParams { etch: EtchMode::Factor, etch_factor: 2.0, t: 0.035, ..PlanarParams::default() };
    let top = WideSide::Top;
    let cases = [
        ("etch factor 2", base.clone()),
        ("etch factor 2, wide side up", PlanarParams { etch_wide: top, ..base.clone() }),
        ("etch factor 1 (45 deg)", PlanarParams { etch_factor: 1.0, ..base.clone() }),
        ("flat flanks, factor 0.6", PlanarParams { etch_factor: 0.6, w: 0.3, s: 0.15, ..base.clone() }),
        ("tight gap", PlanarParams { w: 0.2, s: 0.075, ..base.clone() }),
        ("stripline", PlanarParams { coplanar: false, w: 0.2, ..base.clone() }),
        ("microstrip", PlanarParams { coplanar: false, upper: false, w: 0.5, ..base.clone() }),
        (
            "stripline, side walls",
            PlanarParams { coplanar: false, side_walls: true, w: 0.25, g: 0.12, ..base.clone() },
        ),
        (
            "CPWG outer layer, width difference",
            PlanarParams { upper: false, w: 0.35, s: 0.15, g: 0.3, etch: EtchMode::Delta, ..base },
        ),
    ];
    for (name, p) in cases {
        let fine = PlanarParams { tol: 0.001, ..p.clone() };
        let air = analyze_fem(&fine, &fine.fem_options(), &Ctl::new()).unwrap();
        let reference = from_surface_current(&p, &air, 0.5e-3);
        let sol = analyze_fem(&p, &p.fem_options(), &Ctl::new()).unwrap();
        let fem = wheeler_dzdn_fem(&p, &sol, &Ctl::new()).unwrap();
        println!("{name:36} integral {reference:.0}, FEM {fem:.0} ({:+.2e})", fem / reference - 1.0);
        assert!((fem / reference - 1.0).abs() < 5e-3, "{name}: FEM {fem} vs integral {reference}");
    }
}

/// Pairs, both modes. The integral takes the metal around both traces while the current is that of one, so each
/// trace has half of it. The grid cannot solve pairs.
#[test]
fn wheeler_of_both_modes_of_a_pair_agrees_with_the_integral_of_the_surface_current() {
    let base = PlanarParams { signals: Signals::Pair, w: 0.2, s: 0.2, d: 0.15, ..PlanarParams::default() };
    let cases = [
        ("pair, CPWG", base.clone()),
        ("pair, tight pair gap", PlanarParams { d: 0.06, ..base.clone() }),
        ("pair, stripline", PlanarParams { coplanar: false, ..base.clone() }),
        ("pair, microstrip", PlanarParams { coplanar: false, upper: false, w: 0.4, ..base.clone() }),
        (
            "pair, side walls close by",
            PlanarParams { coplanar: false, side_walls: true, g: 0.12, t: 0.035, ..base.clone() },
        ),
        ("pair, etched", PlanarParams { etch: EtchMode::Factor, t: 0.035, ..base }),
    ];
    for (name, p) in cases {
        let fine = PlanarParams { tol: 0.001, ..p.clone() };
        let air = analyze_fem(&fine, &fine.fem_options(), &Ctl::new()).unwrap();
        let sol = analyze_fem(&p, &p.fem_options(), &Ctl::new()).unwrap();
        let losses = loss_models_fem(&p, &sol, &Ctl::new()).unwrap();
        for (k, loss) in losses.iter().enumerate() {
            let reference = from_surface_current(&p, &air.in_mode(k), 0.5e-3) / 2.0;
            let mode = air.modes[k].kind.key();
            println!(
                "{name:28} {mode:4}: integral {reference:.0}, FEM {:.0} ({:+.2e})",
                loss.dzdn,
                loss.dzdn / reference - 1.0
            );
            assert!(
                (loss.dzdn / reference - 1.0).abs() < 5e-3,
                "{name} {mode}: FEM {} vs integral {reference}",
                loss.dzdn
            );
        }
    }
}
