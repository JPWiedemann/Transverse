//! Etched (trapezoidal) copper. The grid cannot represent slanted flanks, so these checks rest on what must
//! hold whatever the solver: limits, bounds, symmetries and mesh convergence. The conductor loss has its own
//! independent check in `conductor_loss.rs`.

use transverse_core::{Ctl, TvError};
use transverse_lines::planar::{
    DEFAULT_RTOL, EtchMode, GroundCat, PlanarFemAnalysis, PlanarParams, SynthVar, WideSide, analyze,
    analyze_fem, full_analysis_fem, report_fem, synthesize_fem,
};

fn etched(factor: f64) -> PlanarParams {
    PlanarParams { etch: EtchMode::Factor, etch_factor: factor, t: 0.035, ..PlanarParams::default() }
}

fn run(p: &PlanarParams) -> PlanarFemAnalysis {
    full_analysis_fem(p, &Ctl::new()).unwrap()
}

fn z0(p: &PlanarParams) -> f64 {
    analyze_fem(p, &p.fem_options(), &Ctl::new()).unwrap().z0
}

fn rel(a: f64, b: f64) -> f64 {
    (a / b - 1.0).abs()
}

#[test]
fn nearly_vertical_flanks_give_the_results_of_the_rectangle() {
    let rect = run(&PlanarParams { t: 0.035, ..PlanarParams::default() });
    for etch_wide in [WideSide::Bottom, WideSide::Top] {
        // an undercut of 35 nm
        let a = run(&PlanarParams { etch_wide, ..etched(1000.0) });
        assert!(rel(a.sol.z0, rect.sol.z0) < 1e-3, "{} vs {}", a.sol.z0, rect.sol.z0);
        assert!(rel(a.loss.dzdn, rect.loss.dzdn) < 3e-3, "{} vs {}", a.loss.dzdn, rect.loss.dzdn);
        for c in GroundCat::ALL {
            assert!((a.split.current.get(c) - rect.split.current.get(c)).abs() < 0.2, "{c:?}");
        }
    }
}

/// Copper that contains other copper (trace and ground alike) has the larger capacitance in air, so the
/// trapezoid lies between the rectangle of its wide face and the rectangle of its narrow face.
#[test]
fn z0_lies_between_the_rectangles_around_and_inside_the_trapezoid() {
    for factor in [2.0, 1.0] {
        let p = etched(factor);
        let u = p.undercut();
        let air = |q: PlanarParams| {
            z0(&PlanarParams { er1: 1.0, er2: 1.0, gap_mode: Default::default(), tol: 0.1, ..q })
        };
        let wide = air(PlanarParams { etch: EtchMode::Rect, ..p.clone() });
        let narrow =
            air(PlanarParams { etch: EtchMode::Rect, w: p.w - 2.0 * u, s: p.s + 2.0 * u, ..p.clone() });
        let trapezoid = air(p);
        assert!(wide < trapezoid && trapezoid < narrow, "factor {factor}: {wide} {trapezoid} {narrow}");
    }
}

#[test]
fn flipping_the_trace_of_a_symmetric_stripline_changes_nothing() {
    let p = PlanarParams { coplanar: false, w: 0.2, h1: 0.2, h2: 0.2, er1: 3.6, er2: 3.6, ..etched(1.0) };
    let a = run(&p);
    let b = run(&PlanarParams { etch_wide: WideSide::Top, ..p });
    assert!(rel(a.sol.z0, b.sol.z0) < 1e-3, "{} vs {}", a.sol.z0, b.sol.z0);
    assert!(rel(a.loss.dzdn, b.loss.dzdn) < 3e-3, "{} vs {}", a.loss.dzdn, b.loss.dzdn);
    let (c, d) = (&a.split.current, &b.split.current);
    assert!((c.get(GroundCat::Plane1) - d.get(GroundCat::Plane2)).abs() < 0.2);
}

#[test]
fn the_side_of_the_wide_face_matters_next_to_different_dielectrics() {
    let p = PlanarParams { er1: 1.0, gap_mode: Default::default(), ..etched(1.0) };
    let bottom = z0(&p);
    let top = z0(&PlanarParams { etch_wide: WideSide::Top, ..p });
    assert!(rel(bottom, top) > 3e-3, "{bottom} vs {top}");
}

#[test]
fn the_default_accuracy_is_reached_against_a_much_finer_mesh() {
    for p in [etched(2.0), PlanarParams { etch_wide: WideSide::Top, ..etched(0.6) }] {
        let coarse = run(&p);
        let fine = run(&PlanarParams { tol: 0.01, ..p });
        assert!(rel(coarse.sol.z0, fine.sol.z0) < 5e-3, "{} vs {}", coarse.sol.z0, fine.sol.z0);
        assert!(rel(coarse.loss.dzdn, fine.loss.dzdn) < 5e-3);
    }
}

#[test]
fn etch_factor_and_width_difference_describe_the_same_copper() {
    let by_factor = etched(2.0);
    let by_delta = PlanarParams { etch: EtchMode::Delta, etch_delta: 2.0 * 0.035 / 2.0, ..by_factor.clone() };
    assert!((by_factor.undercut() - by_delta.undercut()).abs() < 1e-15);
    assert!((by_factor.narrow_width() - (by_factor.w - 0.035)).abs() < 1e-15);
    assert!((by_factor.flank_angle_deg() - 2.0f64.atan().to_degrees()).abs() < 1e-9);
    assert!(rel(z0(&by_factor), z0(&by_delta)) < 1e-9);
}

/// With flanks flatter than 45 degrees the flank of the coplanar ground must still count as the flank.
#[test]
fn flat_flanks_are_not_counted_as_top_or_bottom_faces() {
    let share = |p: &PlanarParams| run(p).split.current;
    let rect = share(&PlanarParams { t: 0.035, ..PlanarParams::default() });
    // 40 degrees and 31 degrees
    for factor in [0.84, 0.6] {
        let a = share(&etched(factor));
        let flank = a.get(GroundCat::CopFlank);
        assert!(flank > 0.5 * rect.get(GroundCat::CopFlank), "factor {factor}: flank {flank} %");
        assert!(a.get(GroundCat::CopOuter).abs() < 0.05);
        assert!((GroundCat::ALL.iter().map(|&c| a.get(c)).sum::<f64>() - 100.0).abs() < 1e-9);
    }
}

#[test]
fn synthesis_keeps_the_etching_and_finds_the_wide_width() {
    let p = PlanarParams { etch: EtchMode::Delta, etch_delta: 0.03, ..etched(2.0) };
    for var in [SynthVar::W, SynthVar::S, SynthVar::WTotal] {
        let target = if var == SynthVar::W { 40.0 } else { 37.0 };
        let (q, sol) = synthesize_fem(&p, target, var, DEFAULT_RTOL, &Ctl::new()).unwrap();
        assert!(rel(sol.z0, target) < 1e-3, "{var:?}: {}", sol.z0);
        assert_eq!((q.etch, q.etch_delta), (p.etch, p.etch_delta));
        assert!(q.narrow_width() > 0.0);
    }
}

#[test]
fn impossible_shapes_are_refused_and_flat_copper_ignores_the_etching() {
    let message = |p: PlanarParams| match p.validate() {
        Err(TvError::InvalidParam(m)) => m,
        other => panic!("{other:?}"),
    };
    assert!(message(PlanarParams { w: 0.03, ..etched(1.0) }).contains("leaves no trace"));
    assert!(message(PlanarParams { g: 0.03, ..etched(1.0) }).contains("via fences"));
    assert!(message(etched(0.0)).contains("etch factor"));
    let flat = PlanarParams { t: 0.0, ..etched(0.0) };
    flat.validate().unwrap();
    assert_eq!(flat.undercut(), 0.0);
    // without fences g only sizes the computed region, which still has to hold the etched ground edge
    let open = PlanarParams { fence1: false, fence2: false, g: 0.001, ..etched(0.6) };
    open.validate().unwrap();
    assert!(open.core_half_width() > open.inner_half_width() + open.undercut());
    run(&open);
}

#[test]
fn the_report_gives_the_derived_shape_and_the_grid_refuses_slanted_flanks() {
    let p = etched(2.0);
    let report = report_fem("Analysis", &p, &run(&p));
    assert!(report.contains("etch factor 2, wide side at the bottom"), "{report}");
    assert!(report.contains("narrow width 325.50 um, undercut 17.50 um per side, flank angle 63.4 deg"));
    let err = analyze(&p, &Ctl::new()).err().unwrap().to_string();
    assert!(err.contains("grid solver cannot represent etched"), "{err}");
}
