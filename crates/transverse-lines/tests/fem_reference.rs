//! FEM backend vs the finite-difference reference cases (tests/reference/cases.json).
//!
//! The stored values are the converged FD results, so they are the reference for the FEM numbers, not
//! the node counts or the mesh. Tolerances are relative.

use serde::Deserialize;
use transverse_core::Ctl;
use transverse_core::TvError;
use transverse_lines::cpwg_inner::{CpwgParams, FemOptions, MeshMode, analyze, analyze_fem};

#[derive(Deserialize)]
struct Case {
    name: String,
    params: CpwgParams,
    z0: f64,
    eeff: f64,
    c: f64,
    c0: f64,
    g_over_omega: f64,
    analytic_z0: Option<f64>,
    margin: f64,
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
fn auto(p: &CpwgParams) -> CpwgParams {
    CpwgParams { mesh_mode: MeshMode::Auto, tol: 0.5, ..p.clone() }
}

/// The FD domain extension is part of the reference value (an open structure is truncated at it), so the FEM
/// runs with the same one; this isolates the discretisation error.
fn same_domain(c: &Case) -> CpwgParams {
    CpwgParams { margin: c.margin, ..auto(&c.params) }
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
        check(&case.name, "z0", sol.z0, case.z0, 3e-3);
        check(&case.name, "eeff", sol.eeff, case.eeff, 1e-3);
        check(&case.name, "C", sol.c, case.c, 3e-3);
        check(&case.name, "C0", sol.c0, case.c0, 3e-3);
        check(&case.name, "G/omega", sol.g_over_omega, case.g_over_omega, 3e-3);
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
        let p = CpwgParams { tol: 0.2, ..same_domain(&case) };
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
    let p = CpwgParams::default();
    let sol = analyze_fem(&p, &p.fem_options(), &Ctl::new()).unwrap();
    assert!(rel(sol.z0, 35.001) < 2e-3, "z0 {}", sol.z0);
    assert!(rel(sol.eeff, 3.58) < 2e-3, "eeff {}", sol.eeff);
    assert_eq!(sol.margin, 0.0);
    assert!(sol.accuracy.order.is_some() && sol.accuracy.est_err < 5e-3);
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
    let p = CpwgParams { margin: 0.0, ..auto(&case.params) };
    let sol = analyze_fem(&p, &p.fem_options(), &Ctl::new()).unwrap();
    assert!(sol.margin > 0.0, "an open structure needs a margin");
    // the domain truncation costs up to tol/4 on each side (FD and FEM), so allow the sum
    assert!(rel(sol.z0, case.z0) < 5e-3, "{} vs {}", sol.z0, case.z0);
    // an explicit margin is used as it is
    let q = CpwgParams { margin: 3.0, ..p.clone() };
    assert_eq!(analyze_fem(&q, &q.fem_options(), &Ctl::new()).unwrap().margin, 3.0);
}

#[test]
fn limits_cancel_and_manual_meshes_are_honoured() {
    let p = CpwgParams::default();
    let mut opt = p.fem_options();
    opt.max_nodes = 300;
    assert!(matches!(analyze_fem(&p, &opt, &Ctl::new()), Err(TvError::MeshTooLarge { .. })));
    let ctl = Ctl::new();
    ctl.cancel();
    assert!(matches!(analyze_fem(&p, &p.fem_options(), &ctl), Err(TvError::Cancelled)));
    // a manual mesh is one level with the given sizes, and finer sizes give a closer result
    let base = CpwgParams { mesh_mode: MeshMode::Manual, ..p.clone() };
    let coarse = CpwgParams { dmin: 0.004, dmax: 0.05, ..base.clone() };
    let fine = CpwgParams { dmin: 0.001, dmax: 0.0125, ..base };
    let (c, f) = (
        analyze_fem(&coarse, &coarse.fem_options(), &Ctl::new()).unwrap(),
        analyze_fem(&fine, &fine.fem_options(), &Ctl::new()).unwrap(),
    );
    assert_eq!((c.accuracy.nodes.len(), f.accuracy.nodes.len()), (1, 1));
    assert!(f.mesh.pts.len() > 4 * c.mesh.pts.len());
    let fd = analyze(&p, &Ctl::new()).unwrap().z0;
    assert!(rel(f.z0, fd) < rel(c.z0, fd), "finer is closer: {} {} vs {fd}", c.z0, f.z0);
    // invalid parameters are reported
    let bad = CpwgParams { w: -1.0, ..p };
    assert!(analyze_fem(&bad, &bad.fem_options(), &Ctl::new()).is_err());
}

/// Differences to FD for all cases, for tuning; run with `--ignored --nocapture` (FEM_TOL sets the tolerance in %).
#[test]
#[ignore]
fn report_fem_against_fd() {
    for case in load() {
        let tol: f64 = std::env::var("FEM_TOL").ok().and_then(|v| v.parse().ok()).unwrap_or(0.5);
        let p = CpwgParams { tol, ..same_domain(&case) };
        let t0 = std::time::Instant::now();
        let sol = analyze_fem(&p, &p.fem_options(), &Ctl::new()).unwrap();
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
    for (n, z) in sol.accuracy.nodes.iter().zip(&sol.accuracy.z0) {
        println!("   {n:8} nodes  z0 {z:.5}  rel {:+.2e}", z / case.z0 - 1.0);
    }
}
