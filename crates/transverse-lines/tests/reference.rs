//! Parity with the Python reference implementation (tests/reference/cases.json).
//! All tolerances are relative.

use std::collections::HashMap;

use serde::Deserialize;
use transverse_core::Ctl;
use transverse_lines::planar::{
    GroundCat, PlanarParams, SynthVar, analytic, analyze, full_analysis, synthesize,
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
    nodes: usize,
    margin: f64,
    analytic_z0: Option<f64>,
    dzdn: f64,
    loss: Loss,
    split: SplitRef,
}

#[derive(Deserialize)]
struct Loss {
    alpha_c: f64,
    alpha_d: f64,
    #[serde(rename = "R")]
    r: f64,
    #[serde(rename = "G")]
    g: f64,
    rs: f64,
    krough: f64,
    delta: f64,
}

#[derive(Deserialize)]
struct SplitRef {
    field: HashMap<String, f64>,
    current: HashMap<String, f64>,
    x90_planes: Option<f64>,
    x90_cop: f64,
    profiles: HashMap<String, ProfileRef>,
}

#[derive(Deserialize)]
struct ProfileRef {
    n: usize,
    max: f64,
    x_at_max: f64,
    sum: f64,
}

#[derive(Deserialize)]
struct SynthCase {
    name: String,
    params: PlanarParams,
    target: f64,
    var: SynthVar,
    result: SynthResult,
    z0: f64,
}

#[derive(Deserialize)]
struct SynthResult {
    w: f64,
    s: f64,
}

#[derive(Deserialize)]
struct Cases {
    cases: Vec<Case>,
    synthesis: Vec<SynthCase>,
}

fn rel(a: f64, b: f64) -> f64 {
    (a / b - 1.0).abs()
}

fn load_all() -> Cases {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/reference/cases.json");
    serde_json::from_str::<Cases>(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn load() -> Vec<Case> {
    load_all().cases
}

fn key(c: GroundCat) -> &'static str {
    match c {
        GroundCat::Plane1 => "plane1",
        GroundCat::Plane2 => "plane2",
        GroundCat::CopFlank => "cop_flank",
        GroundCat::CopTop => "cop_top",
        GroundCat::CopBottom => "cop_bottom",
        GroundCat::CopOuter => "cop_outer",
        GroundCat::Fence1 => "fence1",
        GroundCat::Fence2 => "fence2",
        GroundCat::Boundary => "boundary",
    }
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
            assert!(rel(analytic(&case.params).unwrap().z0, az) < 1e-12);
        }
    }
}

#[test]
fn cancel_stops_the_solver() {
    let ctl = Ctl::new();
    ctl.cancel();
    let r = analyze(&PlanarParams::default(), &ctl);
    assert!(matches!(r, Err(transverse_core::TvError::Cancelled)));
}

#[test]
fn node_limit_is_reported() {
    let p = PlanarParams { max_nodes: 1000, ..PlanarParams::default() };
    assert!(matches!(analyze(&p, &Ctl::new()), Err(transverse_core::TvError::MeshTooLarge { .. })));
}

#[test]
fn losses_and_ground_split_match_python() {
    for case in load() {
        let a = full_analysis(&case.params, &Ctl::new()).unwrap();
        let name = &case.name;
        assert!(rel(a.loss.dzdn, case.dzdn) < 1e-6, "{name}: dzdn {} vs {}", a.loss.dzdn, case.dzdn);
        let lp = a.loss.at(case.params.f);
        for (what, got, want) in [
            ("alpha_c", lp.alpha_c, case.loss.alpha_c),
            ("alpha_d", lp.alpha_d, case.loss.alpha_d),
            ("R", lp.r, case.loss.r),
            ("G", lp.g, case.loss.g),
            ("rs", lp.rs, case.loss.rs),
            ("krough", lp.krough, case.loss.krough),
            ("delta", lp.delta, case.loss.delta),
        ] {
            assert!(rel(got, want) < 1e-6, "{name}: {what} {got} vs {want}");
        }
        // shares in percent: absolute tolerance of 1e-6 percentage points
        for (what, got, want) in
            [("field", &a.split.field, &case.split.field), ("current", &a.split.current, &case.split.current)]
        {
            for c in GroundCat::ALL {
                let w = want[key(c)];
                assert!((got.get(c) - w).abs() < 1e-6, "{name}: {what} {c:?} {} vs {w}", got.get(c));
            }
            assert!((got.coplanar - want["coplanar"]).abs() < 1e-6, "{name}: {what} coplanar");
        }
        match (a.split.x90_planes, case.split.x90_planes) {
            (Some(g), Some(w)) => assert!(rel(g, w) < 1e-9, "{name}: x90_planes"),
            (g, w) => assert_eq!(g, w, "{name}: x90_planes"),
        }
        assert!((a.split.x90_cop - case.split.x90_cop).abs() < 1e-9, "{name}: x90_cop");
        assert_eq!(a.split.profiles.len(), case.split.profiles.len(), "{name}: profile count");
        for pr in &a.split.profiles {
            let want = &case.split.profiles[key(pr.cat)];
            let (imax, jmax) =
                pr.js.iter().enumerate().fold((0, f64::MIN), |m, (i, &j)| if j > m.1 { (i, j) } else { m });
            assert_eq!(pr.x.len(), want.n, "{name}: {:?} points", pr.cat);
            assert!(rel(jmax, want.max) < 1e-6, "{name}: {:?} max", pr.cat);
            // the structure is symmetric: the maximum occurs at +x and -x, rounding picks one
            assert!((pr.x[imax].abs() - want.x_at_max.abs()).abs() < 1e-9, "{name}: {:?} x at max", pr.cat);
            assert!(rel(pr.js.iter().sum::<f64>(), want.sum) < 1e-6, "{name}: {:?} sum", pr.cat);
        }
    }
}

#[test]
fn final_stackup_reference_values() {
    // values quoted in CLAUDE.md
    let a = full_analysis(&PlanarParams::default(), &Ctl::new()).unwrap();
    assert!(rel(a.sol.z0, 35.001) < 5e-4);
    assert!((a.split.current.get(GroundCat::Plane1) - 39.2).abs() < 0.1);
    assert!((a.split.current.get(GroundCat::Plane2) - 31.5).abs() < 0.1);
    assert!(rel(a.split.current.coplanar, 29.2) < 0.03);
    // 1 GHz, tand = 0.02, sigma = 58 MS/m, smooth: about 0.0031 / 0.0035 dB/mm
    let lp = a.loss.at(1e9);
    assert!((lp.alpha_c * transverse_core::NP2DB / 1000.0 - 0.0031).abs() < 0.0001);
    assert!((lp.alpha_d * transverse_core::NP2DB / 1000.0 - 0.0035).abs() < 0.0004);
}

#[test]
fn synthesis_matches_python() {
    for case in load_all().synthesis {
        let (p, sol) = synthesize(
            &case.params,
            case.target,
            case.var,
            transverse_lines::planar::DEFAULT_RTOL,
            &Ctl::new(),
        )
        .unwrap();
        let name = &case.name;
        assert!(rel(p.w, case.result.w) < 1e-6, "{name}: w {} vs {}", p.w, case.result.w);
        assert!(rel(p.s, case.result.s) < 1e-6, "{name}: s {} vs {}", p.s, case.result.s);
        assert!(rel(sol.z0, case.z0) < 1e-6, "{name}: z0");
        assert!(rel(sol.z0, case.target) < 5e-4, "{name}: target missed, z0 = {}", sol.z0);
    }
}

#[test]
fn synthesis_of_35_ohm_gives_the_final_stackup_width() {
    let (p, sol) = synthesize(&PlanarParams::default(), 35.0, SynthVar::W, 1e-4, &Ctl::new()).unwrap();
    assert!(rel(sol.z0, 35.0) < 5e-4);
    assert!(rel(p.w, 0.3605) < 5e-4);
}

#[test]
fn unreachable_synthesis_target_is_reported() {
    let r = synthesize(&PlanarParams::default(), 1e4, SynthVar::W, 1e-4, &Ctl::new());
    assert!(matches!(r, Err(transverse_core::TvError::Synthesis(_))));
}
