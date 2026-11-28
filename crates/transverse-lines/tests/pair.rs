//! The edge-coupled differential pair of the planar line, against references outside the pair code: the exact
//! values of coupled striplines, the single line a pair falls apart into, and identities of the modes.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use transverse_core::special::k_ratio;
use transverse_core::{Ctl, ETA0};
use transverse_lines::model::{LineModel, ParamValue};
use transverse_lines::planar::{
    DEFAULT_RTOL, FemSolution, ModeKind, PlanarLine, PlanarParams, Signals, SynthVar, analyze, analyze_fem,
    field_data_fem, full_analysis_fem, synthesize_fem, synthesize_pair_fem,
};

fn solve(p: &PlanarParams) -> FemSolution {
    analyze_fem(p, &p.fem_options(), &Ctl::new()).unwrap()
}

fn rel(a: f64, b: f64) -> f64 {
    (a / b - 1.0).abs()
}

fn pair() -> PlanarParams {
    PlanarParams { signals: Signals::Pair, w: 0.2, s: 0.2, d: 0.2, ..PlanarParams::default() }
}

/// Stripline pair without thickness in one material, centred between the planes.
fn exact_pair(d: f64) -> PlanarParams {
    PlanarParams {
        coplanar: false,
        t: 0.0,
        h1: 0.25,
        h2: 0.25,
        er1: 3.5,
        er2: 3.5,
        gap_mode: transverse_lines::planar::GapMode::Upper,
        tol: 0.05,
        d,
        ..pair()
    }
}

#[test]
fn the_stripline_pair_matches_cohns_exact_even_and_odd_impedances() {
    for d in [0.05, 0.2, 0.8] {
        let p = exact_pair(d);
        let sol = solve(&p);
        let b = p.h1 + p.h2;
        let own = (std::f64::consts::PI * p.w / (2.0 * b)).tanh();
        let other = (std::f64::consts::PI * (p.w + d) / (2.0 * b)).tanh();
        let cohn = |k: f64| ETA0 / (4.0 * p.er1.sqrt()) / k_ratio(k);
        let (zo, ze) = (cohn(own / other), cohn(own * other));
        let [odd, even] = [&sol.modes[0], &sol.modes[1]];
        assert_eq!((odd.kind, even.kind), (ModeKind::Odd, ModeKind::Even));
        println!("d {d}: Zo {:.3} ({zo:.3}), Ze {:.3} ({ze:.3}), margin {:.2}", odd.z0, even.z0, sol.margin);
        assert!(rel(odd.z0, zo) < 2e-3, "odd, d {d}: {} vs {zo}", odd.z0);
        assert!(rel(even.z0, ze) < 3e-3, "even, d {d}: {} vs {ze}", even.z0);
        // one dielectric: both modes travel at the same speed
        assert!(rel(odd.eeff, p.er1) < 1e-9 && rel(even.eeff, p.er1) < 1e-9);
        assert!(rel(sol.target_impedance(), 2.0 * zo) < 2e-3);
        assert!(rel(sol.common_impedance().unwrap(), ze / 2.0) < 3e-3);
        assert!((sol.coupling().unwrap() - (ze - zo) / (ze + zo)).abs() < 2e-3);
        // the values and potentials are those of the odd mode until the even one is chosen
        assert_eq!((sol.mode, sol.z0, sol.in_mode(1).z0), (0, odd.z0, even.z0));
        let (a, b) = (&sol.signal_v[0], &sol.signal_v[1]);
        assert!(sol.v.iter().zip(a.iter().zip(b)).all(|(v, (a, b))| (v - (a - b)).abs() < 1e-12));
    }
}

/// The traces of a stripline pair that are far apart see nothing of each other between the planes: both modes are
/// the single line, with its losses. (With a coplanar ground they would not be: it is only outside the pair.)
#[test]
fn a_pair_far_apart_is_two_single_lines() {
    let single = PlanarParams { coplanar: false, w: 0.2, tol: 0.1, ..PlanarParams::default() };
    let apart = PlanarParams { signals: Signals::Pair, d: 3.0, ..single.clone() };
    let one = full_analysis_fem(&single, &Ctl::new()).unwrap();
    let two = full_analysis_fem(&apart, &Ctl::new()).unwrap();
    let even = two.even.as_ref().unwrap();
    for (k, loss) in [&two.loss, &even.loss].into_iter().enumerate() {
        let m = &two.sol.modes[k];
        assert!(rel(m.z0, one.sol.z0) < 1.5e-3, "mode {k}: {} vs {}", m.z0, one.sol.z0);
        assert!(rel(m.eeff, one.sol.eeff) < 5e-4);
        let (a, b) = (loss.at(1e9), one.loss.at(1e9));
        assert!(rel(a.alpha_c, b.alpha_c) < 5e-3, "mode {k}: {} vs {}", a.alpha_c, b.alpha_c);
        assert!(rel(a.alpha_d, b.alpha_d) < 2e-3);
    }
    assert!(two.sol.coupling().unwrap().abs() < 1e-4);
}

#[test]
fn both_modes_split_their_ground_currents() {
    let a = full_analysis_fem(&pair(), &Ctl::new()).unwrap();
    let even = &a.even.as_ref().unwrap().split;
    for split in [&a.split, even] {
        for shares in [&split.field, &split.current] {
            use transverse_lines::planar::GroundCat;
            let sum: f64 = GroundCat::ALL.iter().map(|&c| shares.get(c)).sum();
            assert!((sum - 100.0).abs() < 1e-9, "{sum}");
            assert!(GroundCat::ALL.iter().all(|&c| shares.get(c) > -0.01), "no negative shares");
        }
        assert!(split.x90_cop > 0.0 && split.x90_planes.unwrap() > 0.0);
    }
    // the planes: antisymmetric current in the odd mode, symmetric in the even one
    for (split, sign) in [(&a.split, -1.0), (even, 1.0)] {
        let plane = &split.profiles[0];
        let at = |x: f64| {
            let k = plane.x.partition_point(|&v| v < x).clamp(1, plane.x.len() - 1);
            let f = (x - plane.x[k - 1]) / (plane.x[k] - plane.x[k - 1]);
            plane.js[k - 1] + f * (plane.js[k] - plane.js[k - 1])
        };
        let peak = plane.js.iter().fold(0.0f64, |m, j| m.max(j.abs()));
        for x in [0.05, 0.15, 0.25, 0.35, 0.5] {
            assert!(at(x).abs() > 0.05 * peak, "x {x}: {}", at(x));
            assert!((at(-x) - sign * at(x)).abs() < 0.01 * peak, "x {x}: {} {}", at(-x), at(x));
        }
    }
    // in the middle between the traces the opposed currents cancel; driven together they add
    let centre = |split: &transverse_lines::planar::Split| {
        let plane = &split.profiles[0];
        let peak = plane.js.iter().fold(0.0f64, |m, j| m.max(j.abs()));
        let k = (0..plane.x.len()).min_by(|&a, &b| plane.x[a].abs().total_cmp(&plane.x[b].abs())).unwrap();
        plane.js[k].abs() / peak
    };
    assert!(centre(&a.split) < 0.1 && centre(even) > 0.7, "{} {}", centre(&a.split), centre(even));
}

#[test]
fn synthesis_finds_the_gap_and_the_width_for_a_differential_impedance() {
    let p = pair();
    let zdiff = solve(&p).target_impedance();
    for (var, start) in [
        (SynthVar::D, PlanarParams { d: 0.08, ..p.clone() }),
        (SynthVar::W, PlanarParams { w: 0.3, ..p.clone() }),
    ] {
        let (out, sol) = synthesize_fem(&start, zdiff, var, DEFAULT_RTOL, &Ctl::new()).unwrap();
        assert!(rel(sol.target_impedance(), zdiff) < 2e-3, "{var:?}: {}", sol.target_impedance());
        // Zdiff depends only weakly on the pair gap, so the gap is the less sharply found dimension
        let (found, want, tol) = match var {
            SynthVar::D => (out.d, p.d, 0.05),
            _ => (out.w, p.w, 0.01),
        };
        assert!(rel(found, want) < tol, "{var:?}: {found} vs {want}");
    }
    for var in [SynthVar::D, SynthVar::WTotal] {
        let q = if var == SynthVar::D { PlanarParams::default() } else { p.clone() };
        assert!(synthesize_fem(&q, 90.0, var, DEFAULT_RTOL, &Ctl::new()).is_err(), "{var:?}");
    }
}

#[test]
fn the_line_model_reports_both_modes_and_the_pair() {
    let mut set = PlanarLine.defaults();
    set.set("signals", ParamValue::Num(1.0));
    set.set("w", ParamValue::Num(0.2));
    assert_eq!(PlanarParams::from_set(&set).unwrap().signals, Signals::Pair);
    let a = PlanarLine.analyze(&set, &Ctl::new()).unwrap();
    assert_eq!(a.modes.iter().map(|m| m.name.as_str()).collect::<Vec<_>>(), ["odd", "even"]);
    let pair = a.pair.unwrap();
    assert!(rel(pair.z_diff, 2.0 * a.modes[0].z0) < 1e-12);
    assert!(rel(pair.z_comm, a.modes[1].z0 / 2.0) < 1e-12);
    for needle in
        ["Zdiff", "Zcomm", "Coupling", "Odd mode", "Even mode", "d = 200.00 um", "differential pair"]
    {
        assert!(a.report.contains(needle), "{needle}\n{}", a.report);
    }
    assert!(a.modes.iter().all(|m| m.split.as_ref().unwrap().plot.signals.len() == 2));
    let single = PlanarLine.analyze(&PlanarLine.defaults(), &Ctl::new()).unwrap();
    assert!(single.pair.is_none() && single.modes.len() == 1 && single.modes[0].name == "single");
}

#[test]
fn bad_pairs_and_the_grid_solver_are_refused() {
    assert!(PlanarParams { d: 0.0, ..pair() }.validate().is_err());
    assert!(PlanarParams { d: 0.0, ..PlanarParams::default() }.validate().is_ok(), "unused for one trace");
    let err = analyze(&pair(), &Ctl::new()).err().unwrap();
    assert!(err.to_string().contains("finite elements"), "{err}");
}

/// Pairs with and without coplanar ground: the structures a synthesis of a pair has to work on.
fn pair_structures() -> [(&'static str, PlanarParams); 5] {
    let base = pair();
    [
        ("CPWG", base.clone()),
        ("stripline", PlanarParams { coplanar: false, w: 0.15, ..base.clone() }),
        ("microstrip", PlanarParams { coplanar: false, upper: false, w: 0.35, ..base.clone() }),
        ("buried microstrip", PlanarParams { coplanar: false, plane1: false, ..base.clone() }),
        (
            "stripline, side walls",
            PlanarParams { coplanar: false, side_walls: true, w: 0.15, g: 0.3, ..base },
        ),
    ]
}

#[test]
fn width_and_gap_synthesis_work_with_and_without_coplanar_ground() {
    for (name, p) in pair_structures() {
        let zdiff = solve(&p).target_impedance();
        for (var, start) in [
            (SynthVar::D, PlanarParams { d: 0.12, ..p.clone() }),
            (SynthVar::W, PlanarParams { w: 1.3 * p.w, ..p.clone() }),
        ] {
            let (_, sol) = synthesize_fem(&start, zdiff, var, DEFAULT_RTOL, &Ctl::new())
                .unwrap_or_else(|e| panic!("{name} {var:?}: {e}"));
            assert!(rel(sol.target_impedance(), zdiff) < 3e-3, "{name} {var:?}: {}", sol.target_impedance());
        }
        // the gap s needs the coplanar ground it separates
        let s = synthesize_fem(&p, zdiff, SynthVar::S, DEFAULT_RTOL, &Ctl::new());
        if p.coplanar {
            assert!(s.is_ok(), "{name}: {:?}", s.err());
        } else {
            let err = s.err().unwrap().to_string();
            assert!(err.contains("gap s"), "{name}: {err}");
        }
    }
}

#[test]
fn width_and_gap_are_found_together_for_zdiff_and_zcomm() {
    for (name, p) in pair_structures() {
        let want = solve(&p);
        let (zdiff, zcomm) = (want.target_impedance(), want.common_impedance().unwrap());
        let start = PlanarParams { w: 1.35 * p.w, d: 0.55 * p.d, ..p.clone() };
        // every step is one field solve of a geometry
        let steps = Arc::new(AtomicUsize::new(0));
        let count = steps.clone();
        let ctl = Ctl::new().with_progress(move |m| {
            if m.starts_with("Synthesis step") {
                count.fetch_add(1, Ordering::Relaxed);
            }
        });
        let (out, sol) = synthesize_pair_fem(&start, zdiff, zcomm, DEFAULT_RTOL, &ctl)
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        let (zd, zc) = (sol.target_impedance(), sol.common_impedance().unwrap());
        println!(
            "{name:24} targets {zdiff:.2} / {zcomm:.2}: {zd:.2} / {zc:.2} Ohm, w {:.4} ({:.4}), d {:.4} ({:.4}), {} steps",
            out.w,
            p.w,
            out.d,
            p.d,
            steps.load(Ordering::Relaxed)
        );
        assert!(rel(zd, zdiff) < 2e-3 && rel(zc, zcomm) < 2e-3, "{name}: {zd} {zc}");
        assert!(rel(out.w, p.w) < 0.02, "{name}: w {} vs {}", out.w, p.w);
        // the weaker the coupling, the less the impedances say about the gap
        assert!(rel(out.d, p.d) < 0.1, "{name}: d {} vs {}", out.d, p.d);
        assert_eq!((out.s, out.tol), (p.s, p.tol), "{name}: nothing else changes");
    }
}

/// Targets from Cohn's exact formulas, not from this solver: the dimensions behind them must come out.
#[test]
fn the_two_target_synthesis_finds_the_dimensions_of_an_exact_stripline_pair() {
    let p = exact_pair(0.1);
    let b = p.h1 + p.h2;
    let own = (std::f64::consts::PI * p.w / (2.0 * b)).tanh();
    let other = (std::f64::consts::PI * (p.w + p.d) / (2.0 * b)).tanh();
    let cohn = |k: f64| ETA0 / (4.0 * p.er1.sqrt()) / k_ratio(k);
    let (zdiff, zcomm) = (2.0 * cohn(own / other), cohn(own * other) / 2.0);
    let start = PlanarParams { w: 0.3, d: 0.3, tol: 0.2, ..p.clone() };
    let (out, _) = synthesize_pair_fem(&start, zdiff, zcomm, DEFAULT_RTOL, &Ctl::new()).unwrap();
    println!("exact pair: w {:.5} ({}), d {:.5} ({})", out.w, p.w, out.d, p.d);
    assert!(rel(out.w, p.w) < 5e-3 && rel(out.d, p.d) < 2e-2, "{} {}", out.w, out.d);
}

#[test]
fn impossible_pairs_of_targets_are_refused_with_the_reason() {
    let p = pair();
    let err = |q: &PlanarParams, zd, zc| {
        synthesize_pair_fem(q, zd, zc, DEFAULT_RTOL, &Ctl::new()).err().unwrap().to_string()
    };
    // Zcomm = Zdiff / 4 is two traces without coupling: nothing lies below
    assert!(err(&p, 100.0, 24.0).contains("Zdiff / 4"));
    assert!(err(&PlanarParams::default(), 100.0, 30.0).contains("differential pair"));
    assert!(err(&p, -1.0, 30.0).contains("positive"));
    // refused before any solve: so much coupling that the traces would vanish, and so little that every wide gap
    // gives it
    assert!(err(&p, 100.0, 1000.0).contains("coupling of 0.95"));
    let weak = err(&p, 100.0, 25.001);
    assert!(weak.contains("too weak to fix the gap") && weak.contains("at least 25.16"), "{weak}");
}

/// Next to the weak limit the gap still follows from the targets: two starts find the same one.
#[test]
fn the_gap_of_a_weakly_coupled_pair_does_not_depend_on_the_start() {
    let gap = |d: f64| {
        let start = PlanarParams { coplanar: false, w: 0.2, d, ..pair() };
        synthesize_pair_fem(&start, 100.0, 25.2, DEFAULT_RTOL, &Ctl::new()).unwrap().0.d
    };
    let (a, b) = (gap(0.2), gap(1.2));
    assert!(rel(a, b) < 0.02, "{a} {b}");
}

#[test]
fn the_line_model_synthesizes_both_pair_targets() {
    let mut set = PlanarLine.defaults();
    set.set("signals", ParamValue::Num(1.0));
    set.set("coplanar", ParamValue::Bool(false));
    set.set("w", ParamValue::Num(0.15));
    let target = PlanarLine.synth_targets().iter().find(|t| t.id == "pair_wd").unwrap();
    assert_eq!((target.quantity, target.second), ("Zdiff", Some("Zcomm")));
    assert!(PlanarLine.synthesize(&set, "pair_wd", 100.0, None, None, &Ctl::new()).is_err());
    let (out, a) = PlanarLine.synthesize(&set, "pair_wd", 100.0, Some(28.0), None, &Ctl::new()).unwrap();
    let pair = a.pair.unwrap();
    assert!(rel(pair.z_diff, 100.0) < 3e-3 && rel(pair.z_comm, 28.0) < 3e-3, "{pair:?}");
    assert!(a.report.contains("Zdiff = 100 Ohm, Zcomm = 28 Ohm, vary w and d"), "{}", a.report);
    let out = PlanarParams::from_set(&out).unwrap();
    assert!(out.w != 0.15 && out.d != 0.2);
}

/// Each excitation of the field puts its voltages on the traces, and the combined fields are those made directly
/// from a solution in that mode.
#[test]
fn the_field_excitations_of_a_pair_apply_their_voltages() {
    let p = pair();
    let sol = solve(&p);
    let mut f = field_data_fem(&p, &sol).unwrap();
    let keys: Vec<&str> = f.excitations.iter().map(|x| x.key).collect();
    assert_eq!(keys, ["odd", "even", "trace1", "trace2"]);
    assert_eq!(f.excitation, 0);
    let traces: [Vec<usize>; 2] =
        [1, 2].map(|id| (0..sol.mesh.pts.len()).filter(|&k| sol.mesh.node_cond[k] == Some(id)).collect());
    let grounds: Vec<usize> = (0..sol.mesh.pts.len()).filter(|&k| sol.mesh.node_cond[k] == Some(0)).collect();
    for k in 0..4 {
        assert!(f.select(k));
        let x = f.excitations[k].clone();
        for (nodes, want) in traces.iter().zip(&x.voltages) {
            assert!(!nodes.is_empty() && nodes.iter().all(|&n| (f.v[n] - want).abs() < 1e-12), "{}", x.key);
        }
        assert!(grounds.iter().all(|&n| f.v[n].abs() < 1e-12));
        assert_eq!(f.drive, x.drive);
    }
    assert!(!f.select(4) && f.excitation == 3, "there is no fifth");
    // mirrored points: the odd mode is antisymmetric, the even mode symmetric, one trace is the other one mirrored
    let mut at = |k: usize, point: [f64; 2]| {
        f.select(k);
        (f.potential(point, false).unwrap(), f.e_vector(point).unwrap())
    };
    for point in [[0.05, 0.1], [0.25, -0.1], [0.6, 0.05]] {
        let mirrored = [-point[0], point[1]];
        for (k, sign) in [(0, -1.0), (1, 1.0)] {
            let (a, b) = (at(k, point), at(k, mirrored));
            assert!((a.0 - sign * b.0).abs() < 3e-3, "excitation {k} at {point:?}: {} {}", a.0, b.0);
        }
        let (one, two) = (at(2, point), at(3, mirrored));
        assert!((one.0 - two.0).abs() < 3e-3, "{point:?}: {} {}", one.0, two.0);
    }
    // the even excitation equals the field of a solution in the even mode
    let direct = field_data_fem(&p, &sol.in_mode(1)).unwrap();
    f.select(1);
    assert_eq!(direct.excitation, 1);
    assert!(f.v.iter().zip(&direct.v).all(|(a, b)| (a - b).abs() < 1e-12));
    let peak = direct.e_mag_nodal().iter().fold(0.0f64, |m, &e| m.max(e));
    for (a, b) in f.e_nodal.iter().zip(&direct.e_nodal) {
        assert!((a[0] - b[0]).hypot(a[1] - b[1]) < 1e-9 * peak);
    }
    assert!(f.drive.contains("even mode") && f.drive.contains("Zcomm"), "{}", f.drive);
    // a single line has no choice
    let single = PlanarParams::default();
    assert!(field_data_fem(&single, &solve(&single)).unwrap().excitations.is_empty());
}

#[test]
fn a_pair_has_loss_over_frequency_for_both_modes_and_no_z0_in_its_report() {
    let mut set = PlanarLine.defaults();
    set.set("signals", ParamValue::Num(1.0));
    set.set("w", ParamValue::Num(0.2));
    let a = PlanarLine.analyze(&set, &Ctl::new()).unwrap();
    assert_eq!(a.freq.len(), 2);
    for (mode, freq) in a.modes.iter().zip(&a.freq) {
        let at = freq.at(mode.f_hz);
        assert_eq!((at.z0, at.alpha_c, at.alpha_d), (mode.z0, mode.at_f.alpha_c, mode.at_f.alpha_d));
    }
    assert!(a.freq[0].at(1e9).alpha_c != a.freq[1].at(1e9).alpha_c);
    assert!(!a.report.contains("Z0"), "{}", a.report);
    for needle in ["Zodd            =", "Zeven           =", "those of one trace", "of the mode as a whole"] {
        assert!(a.report.contains(needle), "{needle}\n{}", a.report);
    }
    assert!(a.modes.iter().all(|m| !m.split.as_ref().unwrap().info.contains("Z0")));
    let single = PlanarLine.analyze(&PlanarLine.defaults(), &Ctl::new()).unwrap();
    assert!(single.report.contains("Z0              =") && !single.report.contains("Zodd"));
    assert_eq!(single.freq.len(), 1);
}
