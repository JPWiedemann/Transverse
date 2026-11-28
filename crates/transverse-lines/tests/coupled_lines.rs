//! Two signal conductors through the whole chain (cross-section, mesh, finite elements, adaptive refinement,
//! modes), against references that share nothing with it: the exact conformal mappings of zero-thickness
//! striplines and the limits that follow from symmetry.

use transverse_core::fem::{Level, Order};
use transverse_core::modes::{CapMatrices, PairImpedances, impedance_matrix, modes, symmetric_pair_modes};
use transverse_core::special::k_ratio;
use transverse_core::{Ctl, ETA0};
use transverse_geom::{CrossSection, Item, Material, Paint, Shape, SizeSpec};
use transverse_lines::section_solve::{refine_adaptively, solve_mesh};

const ER: f64 = 3.5;
/// Target of the refinement [%].
const TOL_PCT: f64 = 0.05;

/// A zero-thickness strip from `x0` to `x1` at height `y`, driven as signal `id`.
#[derive(Clone, Copy)]
struct Strip {
    x0: f64,
    x1: f64,
    y: f64,
    id: u8,
}

impl Strip {
    fn centred(xc: f64, w: f64, y: f64, id: u8) -> Self {
        Self { x0: xc - w / 2.0, x1: xc + w / 2.0, y, id }
    }
}

/// Strips between two ground planes at y = 0 and y = `b`, in a grounded box that reaches `half` to each side.
fn stripline(b: f64, half: f64, strips: &[Strip]) -> CrossSection {
    CrossSection {
        domain: [-half, half, 0.0, b],
        background: 0,
        materials: vec![Material { er: ER, tand: 0.02 }],
        items: strips
            .iter()
            .map(|s| Item {
                shape: Shape::segment([s.x0, s.y], [s.x1, s.y]),
                paint: Paint::Conductor { id: s.id, tag: s.id },
            })
            .collect(),
        box_grounded: true,
        box_tag: 0,
    }
}

fn solve(cs: &CrossSection, w: f64) -> (CapMatrices, Level) {
    let b = cs.domain[3] - cs.domain[2];
    let spec = SizeSpec { dmin: w / 25.0, dmax: b / 3.0, growth: 1.3, power: 1.0, far: None };
    let ctl = Ctl::new();
    let (caps, _, level) = refine_adaptively(cs, TOL_PCT, 200_000, &ctl, || {
        let mesh = transverse_mesh::mesh(cs, &spec, 200_000, &ctl)?;
        solve_mesh(cs, mesh, Order::P2, &ctl)
    })
    .unwrap();
    assert!(level.est_err < TOL_PCT / 100.0, "not converged: {}", level.est_err);
    (caps, level)
}

fn rel(a: f64, b: f64) -> f64 {
    (a / b - 1.0).abs()
}

/// Cohn: `Z = eta0 / (4 sqrt(er)) * K(k') / K(k)` for a stripline mode with modulus `k`.
fn cohn(k: f64) -> f64 {
    ETA0 / (4.0 * ER.sqrt()) / k_ratio(k)
}

/// Even and odd impedance of edge-coupled zero-thickness striplines (exact).
fn cohn_coupled(w: f64, gap: f64, b: f64) -> (f64, f64) {
    let own = (std::f64::consts::PI * w / (2.0 * b)).tanh();
    let other = (std::f64::consts::PI * (w + gap) / (2.0 * b)).tanh();
    (cohn(own * other), cohn(own / other))
}

/// Every mode of a homogeneous dielectric travels at the same speed: `L C = mu eps`, so `C = er C0`.
fn assert_homogeneous(caps: &CapMatrices) {
    for (i, j) in caps.c.upper() {
        let (c, c0) = (caps.c.get(i, j), caps.c0.get(i, j));
        assert!((c - ER * c0).abs() < 1e-9 * caps.c.get(i, i), "({i}, {j}): {c} {c0}");
    }
    for m in modes(caps).unwrap() {
        assert!(rel(m.eps_eff, ER) < 1e-9, "{}", m.eps_eff);
    }
}

#[test]
fn edge_coupled_striplines_match_the_exact_even_and_odd_impedances() {
    let (w, b) = (0.2, 0.5);
    for gap in [0.05, 0.2, 0.8] {
        let pitch = w + gap;
        let strips =
            [Strip::centred(-pitch / 2.0, w, b / 2.0, 1), Strip::centred(pitch / 2.0, w, b / 2.0, 2)];
        let (caps, level) = solve(&stripline(b, pitch / 2.0 + w / 2.0 + 8.0 * b, &strips), w);
        assert_homogeneous(&caps);
        let (ze, zo) = cohn_coupled(w, gap, b);
        let [even, odd] = symmetric_pair_modes(&caps).unwrap();
        let pair = PairImpedances::new(&impedance_matrix(&caps).unwrap()).unwrap();
        println!(
            "gap {gap}: Ze {:.3} ({ze:.3}), Zo {:.3} ({zo:.3}), {} nodes",
            even.impedance(),
            odd.impedance(),
            level.nodes.last().unwrap()
        );
        assert!(rel(even.impedance(), ze) < 2e-3, "even, gap {gap}: {} vs {ze}", even.impedance());
        assert!(rel(odd.impedance(), zo) < 2e-3, "odd, gap {gap}: {} vs {zo}", odd.impedance());
        assert!(rel(pair.differential, 2.0 * zo) < 2e-3 && rel(pair.common, ze / 2.0) < 2e-3);
        assert!((pair.coupling - (ze - zo) / (ze + zo)).abs() < 1e-3, "coupling {}", pair.coupling);
        // the mesh is not symmetric, the results of the two lines still agree
        assert!(pair.conversion.abs() < 2e-3 * pair.differential);
    }
}

/// Broadside strips at a quarter and three quarters of the height: in the odd mode the midplane is at 0 V, so
/// each strip is a symmetric stripline between planes at half the spacing.
#[test]
fn the_odd_mode_of_broadside_striplines_is_a_stripline_with_a_plane_in_the_middle() {
    let (w, h) = (0.2, 0.2);
    let strips = [Strip::centred(0.0, w, 3.0 * h, 1), Strip::centred(0.0, w, h, 2)];
    let (caps, _) = solve(&stripline(4.0 * h, 32.0 * h, &strips), w);
    let [even, odd] = symmetric_pair_modes(&caps).unwrap();
    let exact = cohn((std::f64::consts::PI * w / (4.0 * h)).tanh());
    assert!(rel(odd.impedance(), exact) < 2e-3, "{} vs {exact}", odd.impedance());
    assert!(even.impedance() > odd.impedance());
}

/// Strips of different widths, far apart: no coupling, and each is the line it would be alone. This needs the
/// refinement to follow both signals.
#[test]
fn lines_far_apart_are_two_single_lines() {
    let (b, widths, centres) = (0.5, [0.12, 0.35], [-2.5, 2.5]);
    let half = 6.5;
    let strips = [0, 1].map(|i| Strip::centred(centres[i], widths[i], b / 2.0, i as u8 + 1));
    let (caps, _) = solve(&stripline(b, half, &strips), widths[0]);
    let zc = impedance_matrix(&caps).unwrap();
    for i in 0..2 {
        let alone = Strip { id: 1, ..strips[i] };
        let (single, _) = solve(&stripline(b, half, &[alone]), widths[i]);
        let z = single.single().unwrap().z0();
        assert!(rel(zc.get(i, i), z) < 1e-3, "line {}: {} vs {z}", i + 1, zc.get(i, i));
        let exact = cohn((std::f64::consts::PI * widths[i] / (2.0 * b)).tanh());
        assert!(rel(zc.get(i, i), exact) < 2e-3, "line {}: {} vs {exact}", i + 1, zc.get(i, i));
    }
    assert!(zc.get(0, 1).abs() < 1e-5 * zc.get(0, 0), "coupling {}", zc.get(0, 1));
}
