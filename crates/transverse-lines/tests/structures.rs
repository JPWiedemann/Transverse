//! The structures behind the switches: stripline, microstrip, side walls, no upper half. Agreement of the two
//! solvers cannot find a mistake they share (both build on the same description of the structure), so each
//! structure is also held against something independent of them: an exact formula, a published one, or a
//! second way of describing the same physics.

use transverse_core::{Ctl, TvError};
use transverse_lines::planar::{
    GapMode, GroundCat, PlanarParams, SynthVar, analytic, analyze, analyze_fem, full_analysis,
    full_analysis_fem, synthesize_fem,
};

fn rel(a: f64, b: f64) -> f64 {
    (a / b - 1.0).abs()
}

/// A symmetric stripline in one material.
fn stripline(w: f64, t: f64) -> PlanarParams {
    PlanarParams {
        coplanar: false,
        w,
        t,
        h1: 0.25,
        h2: 0.25,
        er1: 3.66,
        er2: 3.66,
        tand1: 0.02,
        tand2: 0.02,
        ..PlanarParams::default()
    }
}

fn microstrip(w: f64, t: f64, er: f64) -> PlanarParams {
    PlanarParams { coplanar: false, upper: false, w, t, h2: 0.254, er2: er, ..PlanarParams::default() }
}

/// Both solvers: (grid, finite elements) as (Z0, eps_eff).
fn both(p: &PlanarParams) -> ((f64, f64), (f64, f64)) {
    let fd = analyze(p, &Ctl::new()).unwrap();
    let fem = analyze_fem(p, &p.fem_options(), &Ctl::new()).unwrap();
    ((fd.z0, fd.eeff), (fem.z0, fem.eeff))
}

#[test]
fn the_symmetric_stripline_matches_the_exact_conformal_mapping() {
    for w in [0.1, 0.25, 0.6] {
        let p = stripline(w, 0.0);
        let exact = analytic(&p).unwrap();
        assert!(exact.exact && rel(exact.eeff, 3.66) < 1e-12);
        let (fd, fem) = both(&p);
        println!("w {w}: exact {:.4}, grid {:.4}, elements {:.4}", exact.z0, fd.0, fem.0);
        assert!(
            rel(fd.0, exact.z0) < 2e-3 && rel(fem.0, exact.z0) < 2e-3,
            "w {w}: {fd:?} {fem:?} vs {}",
            exact.z0
        );
        assert!(rel(fem.1, 3.66) < 1e-6, "one material: eps_eff = er");
    }
}

#[test]
fn the_microstrip_matches_hammerstad_and_jensen() {
    for (w, er) in [(0.12, 3.66), (0.5, 3.66), (0.25, 9.8), (1.0, 2.2)] {
        let p = microstrip(w, 0.0, er);
        let hj = analytic(&p).unwrap();
        assert_eq!(hj.method, "Hammerstad-Jensen");
        let (fd, fem) = both(&p);
        println!(
            "w {w} er {er}: formula {:.3} / {:.3}, grid {:.3} / {:.3}, elements {:.3} / {:.3}",
            hj.z0, hj.eeff, fd.0, fd.1, fem.0, fem.1
        );
        for (name, got) in [("grid", fd), ("elements", fem)] {
            assert!(
                rel(got.0, hj.z0) < 1e-2 && rel(got.1, hj.eeff) < 1e-2,
                "{name} w {w}: {got:?} vs {hj:?}"
            );
        }
    }
}

#[test]
fn a_wide_gap_approaches_the_structure_without_coplanar_ground() {
    let without = stripline(0.25, 0.018);
    let z = |p: &PlanarParams| analyze_fem(p, &p.fem_options(), &Ctl::new()).unwrap().z0;
    let reference = z(&without);
    let with =
        |s: f64| PlanarParams { coplanar: true, fence1: false, fence2: false, s, g: 1.0, ..without.clone() };
    let (near, far) = (z(&with(0.1)), z(&with(2.0)));
    assert!(rel(far, reference) < 2e-3, "{far} vs {reference}");
    assert!(near < 0.97 * reference, "a close ground lowers Z0: {near} vs {reference}");
}

#[test]
fn no_upper_half_is_air_above_whatever_the_upper_fields_say() {
    let off = PlanarParams { upper: false, er1: 9.0, h1: 0.01, ..PlanarParams::default() };
    // the same physics set up by hand, with another (meaningless) height of the air layer
    let by_hand = PlanarParams {
        er1: 1.0,
        tand1: 0.0,
        h1: 0.6,
        plane1: false,
        fence1: false,
        ..PlanarParams::default()
    };
    let z = |p: &PlanarParams| analyze_fem(p, &p.fem_options(), &Ctl::new()).unwrap();
    let (a, b) = (z(&off), z(&by_hand));
    assert!(
        rel(a.z0, b.z0) < 3e-3 && rel(a.eeff, b.eeff) < 2e-3,
        "{} {} vs {} {}",
        a.z0,
        a.eeff,
        b.z0,
        b.eeff
    );
    // and "same as lower" is nothing but copying er and tan d
    let same = PlanarParams { upper_same: true, ..PlanarParams::default() };
    let copied = PlanarParams { er1: same.er2, tand1: same.tand2, ..PlanarParams::default() };
    let (a, b) = (z(&same), z(&copied));
    assert_eq!((a.z0, a.eeff, a.g_over_omega), (b.z0, b.eeff, b.g_over_omega));
}

#[test]
fn side_walls_far_from_the_trace_change_nothing_and_near_ones_lower_z0() {
    let open = stripline(0.25, 0.018);
    let walls = |g: f64| PlanarParams { side_walls: true, g, ..open.clone() };
    let z = |p: &PlanarParams| analyze_fem(p, &p.fem_options(), &Ctl::new()).unwrap();
    let (free, far, near) = (z(&open), z(&walls(2.0)), z(&walls(0.1)));
    assert!(free.margin > 0.0 && far.margin == 0.0, "walls close the structure");
    assert!(rel(far.z0, free.z0) < 2e-3, "{} vs {}", far.z0, free.z0);
    assert!(near.z0 < 0.98 * free.z0, "{} vs {}", near.z0, free.z0);
}

#[test]
fn both_solvers_agree_on_the_new_structures_including_losses_and_ground_split() {
    let cases = [
        ("stripline", stripline(0.25, 0.018)),
        ("stripline with side walls", PlanarParams { side_walls: true, g: 0.2, ..stripline(0.25, 0.035) }),
        ("asymmetric stripline", PlanarParams { h1: 0.15, er1: 3.0, ..stripline(0.2, 0.018) }),
        ("microstrip", microstrip(0.5, 0.035, 4.3)),
        ("buried microstrip", PlanarParams { plane1: false, ..stripline(0.25, 0.018) }),
        (
            "CPWG, outer layer",
            PlanarParams { upper: false, w: 0.35, s: 0.15, g: 0.3, ..PlanarParams::default() },
        ),
        (
            "CPW without planes",
            PlanarParams { upper: false, plane2: false, fence2: false, ..PlanarParams::default() },
        ),
    ];
    for (name, p) in cases {
        let fd = full_analysis(&p, &Ctl::new()).unwrap();
        // the same domain for both, so that only the discretisation differs
        let q = PlanarParams { margin: fd.sol.accuracy.margin, ..p.clone() };
        let fem = full_analysis_fem(&q, &Ctl::new()).unwrap();
        let (a, b) = (fd.loss.at(p.f), fem.loss.at(p.f));
        println!(
            "{name:28} Z0 {:.3} / {:.3} ({:+.1e}), eeff {:+.1e}, alpha_c {:+.1e}, alpha_d {:+.1e}",
            fd.sol.z0,
            fem.sol.z0,
            fem.sol.z0 / fd.sol.z0 - 1.0,
            fem.sol.eeff / fd.sol.eeff - 1.0,
            b.alpha_c / a.alpha_c - 1.0,
            b.alpha_d / a.alpha_d - 1.0
        );
        assert!(rel(fem.sol.z0, fd.sol.z0) < 2e-3, "{name}: Z0");
        assert!(rel(fem.sol.eeff, fd.sol.eeff) < 1e-3, "{name}: eps_eff");
        assert!(rel(b.alpha_c, a.alpha_c) < 5e-3, "{name}: alpha_c {} vs {}", b.alpha_c, a.alpha_c);
        assert!(rel(b.alpha_d, a.alpha_d) < 2e-3, "{name}: alpha_d");
        let mut total = 0.0;
        for cat in GroundCat::ALL {
            let (x, y) = (fd.split.current.get(cat), fem.split.current.get(cat));
            assert!((x - y).abs() < 0.6, "{name}: current share {} {x} vs {y}", cat.key());
            if !p.effective().coplanar {
                assert!(!GroundCat::COPLANAR.contains(&cat) || y == 0.0, "{name}: no coplanar ground");
            }
            total += y;
        }
        assert!((total - 100.0).abs() < 1e-6, "{name}: shares add to {total}");
    }
}

#[test]
fn structures_without_ground_and_gap_synthesis_without_coplanar_ground_are_refused() {
    let none = PlanarParams { plane2: false, ..microstrip(0.3, 0.018, 4.3) };
    assert!(matches!(analyze_fem(&none, &none.fem_options(), &Ctl::new()), Err(TvError::InvalidParam(_))));
    let p = microstrip(0.3, 0.018, 4.3);
    for var in [SynthVar::S, SynthVar::WTotal] {
        assert!(matches!(synthesize_fem(&p, 50.0, var, 1e-4, &Ctl::new()), Err(TvError::Synthesis(_))));
    }
    // the width of a 50 ohm microstrip: against the formula, within its accuracy and that of the thickness
    let thin = microstrip(0.3, 0.0, 4.3);
    let (found, sol) = synthesize_fem(&thin, 50.0, SynthVar::W, 1e-4, &Ctl::new()).unwrap();
    assert!(rel(sol.z0, 50.0) < 2e-3);
    let hj = analytic(&found).unwrap();
    assert!(rel(hj.z0, 50.0) < 1e-2, "w = {} gives {} by the formula", found.w, hj.z0);
    // unused fields may hold anything
    let junk = PlanarParams { s: -1.0, g: f64::NAN, h1: -3.0, er1: 0.0, gap_mode: GapMode::Upper, ..p };
    junk.validate().unwrap();
}
