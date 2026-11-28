//! FEM backend vs the finite-difference reference cases (tests/reference/cases.json).
//!
//! The stored values are the converged FD results, so they are the reference for the FEM numbers, not
//! the node counts or the mesh. Tolerances are relative.

use serde::Deserialize;
use transverse_core::Ctl;
use transverse_core::TvError;
use transverse_lines::planar::{
    FemOptions, GroundCat, MeshMode, PlanarParams, analyze, analyze_fem, ground_split_fem, loss_model_fem,
};

#[derive(Deserialize)]
struct Case {
    name: String,
    params: PlanarParams,
    z0: f64,
    eeff: f64,
    c: f64,
    c0: f64,
    g_over_omega: f64,
    analytic_z0: Option<f64>,
    margin: f64,
    dzdn: f64,
    loss: Loss,
    split: SplitRef,
}

#[derive(Deserialize)]
struct SplitRef {
    field: std::collections::HashMap<String, f64>,
    current: std::collections::HashMap<String, f64>,
    x90_planes: Option<f64>,
    x90_cop: f64,
}

#[derive(Deserialize)]
struct Loss {
    alpha_c: f64,
    alpha_d: f64,
}

#[derive(Deserialize)]
struct Cases {
    cases: Vec<Case>,
}

fn load() -> Vec<Case> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/reference/cases.json");
    serde_json::from_str::<Cases>(&std::fs::read_to_string(path).unwrap()).unwrap().cases
}

fn rel(a: f64, b: f64) -> f64 {
    (a / b - 1.0).abs()
}

/// The reference value belongs to the converged solution, so run the FEM in automatic mode.
fn auto(p: &PlanarParams) -> PlanarParams {
    PlanarParams { mesh_mode: MeshMode::Auto, tol: 0.5, ..p.clone() }
}

/// The FD domain extension is part of the reference value (an open structure is truncated at it), so the FEM
/// runs with the same one; this isolates the discretisation error.
fn same_domain(c: &Case) -> PlanarParams {
    PlanarParams { margin: c.margin, ..auto(&c.params) }
}

/// Reference cases whose stored values are converged (the two manual-mesh cases hold the FD value of a
/// deliberately coarse grid, which is not what the FEM converges to).
fn converged(c: &Case) -> bool {
    !c.name.starts_with("manual_mesh")
}

fn check(name: &str, what: &str, fem: f64, fd: f64, tol: f64) {
    assert!(
        rel(fem, fd) < tol,
        "{name}: {what} FEM {fem} vs FD {fd} (rel. difference {:.2e}, limit {tol:.0e})",
        rel(fem, fd)
    );
}

#[test]
fn fem_matches_fd_on_the_reference_cases() {
    // automatic mesh levels at the default tolerance (0.5 %): the measured differences are below
    // 2.5e-3 (Z0, C, C0, G/omega) and 3.1e-4 (eps_eff)
    for case in load().iter().filter(|c| converged(c)) {
        let p = same_domain(case);
        let sol = analyze_fem(&p, &p.fem_options(), &Ctl::new()).unwrap();
        check(&case.name, "z0", sol.z0, case.z0, 1e-3);
        check(&case.name, "eeff", sol.eeff, case.eeff, 1e-3);
        check(&case.name, "C", sol.c, case.c, 1e-3);
        check(&case.name, "C0", sol.c0, case.c0, 1e-3);
        check(&case.name, "G/omega", sol.g_over_omega, case.g_over_omega, 1.2e-3);
        assert!(sol.accuracy.nodes.len() >= 3, "{}: at least three levels", case.name);
        assert!(sol.accuracy.est_err < 5e-3, "{}: estimated error {}", case.name, sol.accuracy.est_err);
    }
}

#[test]
fn a_tighter_tolerance_brings_the_fem_within_one_per_mille() {
    // measured at tol 0.2 %: all differences below 7e-4
    for name in
        ["final_stackup", "closed_t0_w0.248_s0.15", "open_no_plane1_no_fence1", "no_fences_planes_kept"]
    {
        let case = load().into_iter().find(|c| c.name == name).unwrap();
        let p = PlanarParams { tol: 0.2, ..same_domain(&case) };
        let sol = analyze_fem(&p, &p.fem_options(), &Ctl::new()).unwrap();
        check(name, "z0", sol.z0, case.z0, 1e-3);
        check(name, "eeff", sol.eeff, case.eeff, 5e-4);
        check(name, "C", sol.c, case.c, 1e-3);
        check(name, "C0", sol.c0, case.c0, 1e-3);
        check(name, "G/omega", sol.g_over_omega, case.g_over_omega, 1e-3);
    }
}

#[test]
fn t0_cases_match_conformal_mapping() {
    for case in load() {
        let Some(za) = case.analytic_z0 else { continue };
        let p = same_domain(&case);
        let sol = analyze_fem(&p, &p.fem_options(), &Ctl::new()).unwrap();
        assert!(rel(sol.z0, za) < 3e-3, "{}: FEM {} vs analytic {za}", case.name, sol.z0);
    }
}

#[test]
fn the_fem_closed_stackup_has_the_reference_values_of_the_project() {
    let p = PlanarParams::default();
    let sol = analyze_fem(&p, &p.fem_options(), &Ctl::new()).unwrap();
    assert!(rel(sol.z0, 35.001) < 2e-3, "z0 {}", sol.z0);
    assert!(rel(sol.eeff, 3.58) < 2e-3, "eeff {}", sol.eeff);
    assert_eq!(sol.margin, 0.0);
    assert!(sol.adaptive && sol.accuracy.est_err < 5e-3);
    // the finest mesh and its potentials are kept: nodes, then one value per edge for quadratic elements
    assert_eq!(sol.v.len(), sol.mesh.pts.len() + sol.edges.len());
    assert_eq!(sol.v.len(), sol.v0.len());
    // 1 V on the signal nodes, 0 V on the ground nodes
    for (n, c) in sol.mesh.node_cond.iter().enumerate() {
        match c {
            Some(1) => assert_eq!(sol.v[n], 1.0),
            Some(_) => assert_eq!(sol.v[n], 0.0),
            None => assert!((0.0..=1.0).contains(&sol.v[n])),
        }
    }
}

#[test]
fn an_open_structure_finds_its_own_domain_size() {
    let case = load().into_iter().find(|c| c.name == "open_no_plane1_no_fence1").unwrap();
    // margin 0 = automatic, as for the grid
    let p = PlanarParams { margin: 0.0, ..auto(&case.params) };
    let sol = analyze_fem(&p, &p.fem_options(), &Ctl::new()).unwrap();
    assert!(sol.margin > 0.0, "an open structure needs a margin");
    // the domain truncation costs up to tol/4 on each side (FD and FEM), so allow the sum
    assert!(rel(sol.z0, case.z0) < 5e-3, "{} vs {}", sol.z0, case.z0);
    // an explicit margin is used as it is
    let q = PlanarParams { margin: 3.0, ..p.clone() };
    assert_eq!(analyze_fem(&q, &q.fem_options(), &Ctl::new()).unwrap().margin, 3.0);
}

#[test]
fn limits_cancel_and_manual_meshes_are_honoured() {
    let p = PlanarParams::default();
    let mut opt = p.fem_options();
    opt.max_nodes = 300;
    assert!(matches!(analyze_fem(&p, &opt, &Ctl::new()), Err(TvError::MeshTooLarge { .. })));
    let ctl = Ctl::new();
    ctl.cancel();
    assert!(matches!(analyze_fem(&p, &p.fem_options(), &ctl), Err(TvError::Cancelled)));
    // a manual mesh is one level with the given sizes, and finer sizes give a closer result
    let base = PlanarParams { mesh_mode: MeshMode::Manual, ..p.clone() };
    let coarse = PlanarParams { dmin: 0.004, dmax: 0.05, ..base.clone() };
    let fine = PlanarParams { dmin: 0.001, dmax: 0.0125, ..base };
    let (c, f) = (
        analyze_fem(&coarse, &coarse.fem_options(), &Ctl::new()).unwrap(),
        analyze_fem(&fine, &fine.fem_options(), &Ctl::new()).unwrap(),
    );
    assert_eq!((c.accuracy.nodes.len(), f.accuracy.nodes.len()), (1, 1));
    assert!(f.mesh.pts.len() > 4 * c.mesh.pts.len());
    let fd = analyze(&p, &Ctl::new()).unwrap().z0;
    assert!(rel(f.z0, fd) < rel(c.z0, fd), "finer is closer: {} {} vs {fd}", c.z0, f.z0);
    // invalid parameters are reported
    let bad = PlanarParams { w: -1.0, ..p };
    assert!(analyze_fem(&bad, &bad.fem_options(), &Ctl::new()).is_err());
}

#[test]
fn fem_losses_match_fd() {
    // Wheeler conductor loss and dielectric loss at the default tolerance (0.5 %); measured differences:
    // dZ0/dn below 5e-3, alpha_c below 5.5e-3, alpha_d below 6e-4 (the selftest bar for losses is 1e-2).
    // cbcpw_air_above (200k nodes) is left out to keep the test short.
    let names = [
        "closed_t0_w0.248_s0.15",
        "closed_t0_w0.2_s0.1",
        "final_stackup",
        "open_no_plane1_no_fence1",
        "no_fences_planes_kept",
        "gap_custom_air",
        "rough_copper_5ghz",
        "explicit_margin",
    ];
    for case in load().iter().filter(|c| names.contains(&c.name.as_str())) {
        let p = same_domain(case);
        let sol = analyze_fem(&p, &p.fem_options(), &Ctl::new()).unwrap();
        let lm = loss_model_fem(&p, &sol, &Ctl::new()).unwrap();
        let at = lm.at(p.f);
        check(&case.name, "dZ0/dn", lm.dzdn, case.dzdn, 2e-3);
        check(&case.name, "alpha_c", at.alpha_c, case.loss.alpha_c, 1e-3);
        check(&case.name, "alpha_d", at.alpha_d, case.loss.alpha_d, 1e-3);
        // roughness and skin depth are independent of the solver
        assert_eq!(lm.sigma, p.sigma);
    }
}

#[test]
fn fem_ground_split_matches_fd() {
    // shares in percentage points, x90 widths relative. Measured at the default tolerance: shares within 0.3 pp
    // for the structures with thick copper and for the t = 0 cases with the narrower gap, within 1.7 pp for the
    // t = 0 case with w = 0.2 / s = 0.1: for zero-thickness conductors the flank share is the flux around the
    // tip, which depends on the mesh there; x90 widths within 2 %.
    let names = [
        "closed_t0_w0.248_s0.15",
        "closed_t0_w0.2_s0.1",
        "cbcpw_air_above",
        "final_stackup",
        "open_no_plane1_no_fence1",
        "open_no_planes",
        "no_fences_planes_kept",
        "gap_lower",
        "explicit_margin",
    ];
    for case in load().iter().filter(|c| names.contains(&c.name.as_str())) {
        let p = same_domain(case);
        let sol = analyze_fem(&p, &p.fem_options(), &Ctl::new()).unwrap();
        let split = ground_split_fem(&p, &sol).unwrap();
        let t0 = p.t == 0.0;
        let limit = if t0 { 1.7 } else { 0.35 };
        for c in GroundCat::ALL {
            let key = cat_key(c);
            for (what, fem, fd) in [
                ("E-field share", split.field.get(c), case.split.field[key]),
                ("return current share", split.current.get(c), case.split.current[key]),
            ] {
                assert!(
                    (fem - fd).abs() < limit,
                    "{}: {what} {key}: FEM {fem:.2} % vs FD {fd:.2} % (limit {limit} pp)",
                    case.name
                );
            }
        }
        let total: f64 = GroundCat::ALL.iter().map(|&c| split.current.get(c)).sum();
        assert!((total - 100.0).abs() < 1e-9, "{}: shares add up to {total}", case.name);
        assert_eq!(split.x90_planes.is_some(), case.split.x90_planes.is_some(), "{}", case.name);
        if let (Some(a), Some(b)) = (split.x90_planes, case.split.x90_planes) {
            check(&case.name, "x90 planes", a, b, 2e-2);
        }
        check(&case.name, "x90 coplanar", split.x90_cop, case.split.x90_cop, 3e-2);
        // the plane current profiles integrate to their share (the edges are horizontal, so x is the length)
        for prof in &split.profiles {
            if !matches!(prof.cat, GroundCat::Plane1 | GroundCat::Plane2) {
                continue;
            }
            let integral: f64 = prof
                .x
                .windows(2)
                .zip(prof.js.windows(2))
                .map(|(x, j)| 0.5 * (j[0] + j[1]) * (x[1] - x[0]))
                .sum();
            let share = split.current.get(prof.cat) / 100.0;
            // the trapezoid rule over edge centres misses half an edge at each end
            assert!(
                (integral / share - 1.0).abs() < 0.05,
                "{}: {:?} profile integral {integral} vs share {share}",
                case.name,
                prof.cat
            );
        }
    }
}

/// Differences to FD for all cases, for tuning; run with `--ignored --nocapture` (FEM_TOL sets the tolerance in %).
#[test]
#[ignore]
fn report_fem_against_fd() {
    for case in load() {
        let tol: f64 = std::env::var("FEM_TOL").ok().and_then(|v| v.parse().ok()).unwrap_or(0.5);
        let p = PlanarParams { tol, ..same_domain(&case) };
        let t0 = std::time::Instant::now();
        let start = std::env::var("FEM_START").ok().and_then(|v| v.parse().ok());
        let opt = FemOptions {
            adaptive: std::env::var("FEM_UNIFORM").is_err(),
            start_scale: start.unwrap_or(p.fem_options().start_scale),
            ..p.fem_options()
        };
        let sol = analyze_fem(&p, &opt, &Ctl::new()).unwrap();
        println!(
            "{:28} nodes {:7} levels {} | z0 {:+.2e} eeff {:+.2e} C {:+.2e} C0 {:+.2e} G {:+.2e} | {:.1}s order {:?} est {:.1e}",
            case.name,
            sol.mesh.pts.len(),
            sol.accuracy.nodes.len(),
            sol.z0 / case.z0 - 1.0,
            sol.eeff / case.eeff - 1.0,
            sol.c / case.c - 1.0,
            sol.c0 / case.c0 - 1.0,
            sol.g_over_omega / case.g_over_omega - 1.0,
            t0.elapsed().as_secs_f64(),
            sol.accuracy.order,
            sol.accuracy.est_err,
        );
    }
}

/// Z0 of every mesh level of one case; `FEM_START` and `FEM_LEVELS` set the level sequence.
#[test]
#[ignore]
fn report_levels() {
    let name = std::env::var("FEM_CASE").unwrap_or_else(|_| "final_stackup".into());
    let case = load().into_iter().find(|c| c.name == name).unwrap();
    let p = same_domain(&case);
    let start = std::env::var("FEM_START").ok().and_then(|v| v.parse().ok()).unwrap_or(4.0);
    let levels = std::env::var("FEM_LEVELS").ok().and_then(|v| v.parse().ok()).unwrap_or(6);
    let opt = FemOptions {
        tol_pct: 1e-6,
        start_scale: start,
        max_levels: levels,
        max_nodes: 400_000,
        ..p.fem_options()
    };
    let sol = analyze_fem(&p, &opt, &Ctl::new()).unwrap();
    println!("{name} (reference z0 {})", case.z0);
    for (n, z) in sol.accuracy.nodes.iter().zip(sol.accuracy.z0()) {
        println!("   {n:8} nodes  z0 {z:.5}  rel {:+.2e}", z / case.z0 - 1.0);
    }
}

/// Wheeler conductor loss and dielectric loss against FD, for tuning; `--ignored --nocapture`.
#[test]
#[ignore]
fn report_losses() {
    for case in load().into_iter().filter(converged) {
        let tol: f64 = std::env::var("FEM_TOL").ok().and_then(|v| v.parse().ok()).unwrap_or(0.5);
        let p = PlanarParams { tol, ..same_domain(&case) };
        let t0 = std::time::Instant::now();
        let sol = analyze_fem(&p, &p.fem_options(), &Ctl::new()).unwrap();
        let lm = loss_model_fem(&p, &sol, &Ctl::new()).unwrap();
        let at = lm.at(p.f);
        println!(
            "{:28} dzdn {:+.2e} alpha_c {:+.2e} alpha_d {:+.2e} | {:.1}s",
            case.name,
            lm.dzdn / case.dzdn - 1.0,
            at.alpha_c / case.loss.alpha_c - 1.0,
            at.alpha_d / case.loss.alpha_d - 1.0,
            t0.elapsed().as_secs_f64()
        );
    }
}

fn cat_key(c: GroundCat) -> &'static str {
    c.key()
}

/// Ground split against FD; `--ignored --nocapture`.
#[test]
#[ignore]
fn report_split() {
    for case in load().into_iter().filter(converged) {
        let tol: f64 = std::env::var("FEM_TOL").ok().and_then(|v| v.parse().ok()).unwrap_or(0.5);
        let p = PlanarParams { tol, ..same_domain(&case) };
        let sol = analyze_fem(&p, &p.fem_options(), &Ctl::new()).unwrap();
        let split = ground_split_fem(&p, &sol).unwrap();
        let (mut worst_f, mut worst_c) = (0.0f64, 0.0f64);
        let mut line = String::new();
        for c in GroundCat::ALL {
            let (f, cu) = (split.field.get(c), split.current.get(c));
            let (rf, rc) = (case.split.field[cat_key(c)], case.split.current[cat_key(c)]);
            worst_f = worst_f.max((f - rf).abs());
            worst_c = worst_c.max((cu - rc).abs());
            if (cu - rc).abs() > 0.05 {
                line += &format!(" {}:{:+.2}", cat_key(c), cu - rc);
            }
        }
        println!(
            "{:28} worst field {:.3} pp, current {:.3} pp | x90_planes {:?} vs {:?} | x90_cop {:.4} vs {:.4} |{line}",
            case.name,
            worst_f,
            worst_c,
            split.x90_planes,
            case.split.x90_planes,
            split.x90_cop,
            case.split.x90_cop
        );
    }
}

#[test]
fn adaptive_and_uniform_refinement_agree_and_the_adaptive_mesh_is_far_smaller() {
    let case = load().into_iter().find(|c| c.name == "final_stackup").unwrap();
    let p = PlanarParams { tol: 0.1, ..same_domain(&case) };
    let run = |adaptive: bool| {
        let opt = if adaptive { p.fem_options() } else { p.fem_options().uniform() };
        analyze_fem(&p, &opt, &Ctl::new()).unwrap()
    };
    let (a, u) = (run(true), run(false));
    for (name, s) in [("adaptive", &a), ("uniform", &u)] {
        check("final_stackup", name, s.z0, case.z0, 1e-3);
    }
    assert!(a.adaptive && !u.adaptive);
    assert!(rel(a.z0, u.z0) < 1e-3, "adaptive {} vs uniform {}", a.z0, u.z0);
    // measured: 6.6k against 24.7k nodes
    assert!(a.mesh.pts.len() * 3 < u.mesh.pts.len(), "{} vs {} nodes", a.mesh.pts.len(), u.mesh.pts.len());
}

/// The refinement goes where the error is: to the conductor edges, where the field is singular. Asking for more
/// accuracy adds elements there and hardly any away from them.
#[test]
fn adaptive_refinement_goes_to_the_conductor_edges() {
    let p = PlanarParams::default();
    // triangles within 30 um of an edge of the strip or of the coplanar ground at the gap, and beyond 150 um
    let count = |tol: f64| {
        let q = PlanarParams { tol, ..p.clone() };
        let sol = analyze_fem(&q, &q.fem_options(), &Ctl::new()).unwrap();
        let (mut near, mut far) = (0usize, 0usize);
        for t in &sol.mesh.tris {
            let [x, y] = [0, 1].map(|k| t.iter().map(|&i| sol.mesh.pts[i as usize][k]).sum::<f64>() / 3.0);
            let distance = [p.w / 2.0, p.w / 2.0 + p.s]
                .iter()
                .flat_map(|&edge| [0.0, p.t].map(|ye| (x.abs() - edge).hypot(y - ye)))
                .fold(f64::INFINITY, f64::min);
            near += usize::from(distance < 0.03);
            far += usize::from(distance > 0.15);
        }
        (near, far, sol.mesh.pts.len())
    };
    let (coarse, fine) = (count(0.5), count(0.05));
    // measured: 2.7k -> 4.7k near the edges, 3.7k -> 3.9k far from them, 4.8k -> 6.6k nodes
    assert!(coarse.2 < 8_000, "{} nodes at the default tolerance", coarse.2);
    assert!(fine.0 as f64 > 1.5 * coarse.0 as f64, "near the edges: {} -> {}", coarse.0, fine.0);
    assert!((fine.1 as f64) < 1.15 * coarse.1 as f64, "far from them: {} -> {}", coarse.1, fine.1);
}
