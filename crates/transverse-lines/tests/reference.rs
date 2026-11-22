//! Parity with the Python reference implementation (tests/reference/cases.json).
//! All tolerances are relative.

use serde::Deserialize;
use transverse_core::Ctl;
use transverse_lines::cpwg_inner::{CpwgParams, analytic, analyze};

#[derive(Deserialize)]
struct Case {
    name: String,
    params: CpwgParams,
    z0: f64,
    eeff: f64,
    c: f64,
    c0: f64,
    g_over_omega: f64,
    nodes: usize,
    margin: f64,
    analytic_z0: Option<f64>,
}

#[derive(Deserialize)]
struct Cases {
    cases: Vec<Case>,
}

fn rel(a: f64, b: f64) -> f64 {
    (a / b - 1.0).abs()
}

fn load() -> Vec<Case> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/reference/cases.json");
    serde_json::from_str::<Cases>(&std::fs::read_to_string(path).unwrap()).unwrap().cases
}

#[test]
fn matches_python_on_all_reference_cases() {
    for case in load() {
        let sol = analyze(&case.params, &Ctl::new()).unwrap();
        // same algorithm: only the factorisation differs, so expect far better than the 0.05 % target
        assert!(rel(sol.z0, case.z0) < 1e-6, "{}: z0 {} vs {}", case.name, sol.z0, case.z0);
        assert!(rel(sol.eeff, case.eeff) < 1e-6, "{}: eeff {} vs {}", case.name, sol.eeff, case.eeff);
        assert!(rel(sol.c, case.c) < 1e-6, "{}: C", case.name);
        assert!(rel(sol.c0, case.c0) < 1e-6, "{}: C0", case.name);
        assert!(rel(sol.g_over_omega, case.g_over_omega) < 1e-5, "{}: G/omega", case.name);
        assert_eq!(sol.accuracy.nodes, case.nodes, "{}: node count (mesh differs)", case.name);
        assert!(rel(sol.accuracy.margin.max(1e-12), case.margin.max(1e-12)) < 1e-9, "{}: margin", case.name);
    }
}

#[test]
fn t0_cases_match_conformal_mapping() {
    for case in load() {
        if let Some(az) = case.analytic_z0 {
            let sol = analyze(&case.params, &Ctl::new()).unwrap();
            assert!(rel(sol.z0, az) < 2.5e-3, "{}: {} vs analytic {}", case.name, sol.z0, az);
            assert!(rel(analytic(&case.params).0, az) < 1e-12);
        }
    }
}

#[test]
fn cancel_stops_the_solver() {
    let ctl = Ctl::new();
    ctl.cancel();
    let r = analyze(&CpwgParams::default(), &ctl);
    assert!(matches!(r, Err(transverse_core::TvError::Cancelled)));
}

#[test]
fn node_limit_is_reported() {
    let p = CpwgParams { max_nodes: 1000, ..CpwgParams::default() };
    assert!(matches!(analyze(&p, &Ctl::new()), Err(transverse_core::TvError::MeshTooLarge { .. })));
}
