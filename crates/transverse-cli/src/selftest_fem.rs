//! Self-test of the FEM backend: the reference cases of the finite-difference tool (which are converged
//! values), compared with the tolerances of the FEM-vs-FD parity tests.

use std::collections::HashMap;
use std::process::ExitCode;
use std::time::Instant;

use serde::Deserialize;
use transverse_core::Ctl;
use transverse_lines::cpwg_inner::{CpwgParams, GroundCat, MeshMode, full_analysis_fem};

const EMBEDDED: &str = include_str!("../../../tests/reference/cases.json");

// Relative tolerances (measured differences are well below these at the default 0.5 % mesh tolerance).
const TOL_Z0: f64 = 3e-3;
const TOL_EEFF: f64 = 1e-3;
const TOL_LOSS: f64 = 1e-2;
const TOL_ANALYTIC: f64 = 3e-3;
/// Percentage points: thick copper / zero-thickness copper (the tip flux depends on the mesh there).
const TOL_SPLIT: f64 = 0.35;
const TOL_SPLIT_T0: f64 = 1.7;

#[derive(Deserialize)]
struct Case {
    name: String,
    params: CpwgParams,
    z0: f64,
    eeff: f64,
    margin: f64,
    analytic_z0: Option<f64>,
    loss: Loss,
    split: SplitRef,
}

#[derive(Deserialize)]
struct Loss {
    alpha_c: f64,
    alpha_d: f64,
}

#[derive(Deserialize)]
struct SplitRef {
    current: HashMap<String, f64>,
}

#[derive(Deserialize)]
struct Cases {
    cases: Vec<Case>,
}

fn rel(a: f64, b: f64) -> f64 {
    (a / b - 1.0).abs()
}

pub fn run(path: Option<&str>) -> Result<ExitCode, String> {
    let text = match path {
        Some(p) => std::fs::read_to_string(p).map_err(|e| format!("{p}: {e}"))?,
        None => EMBEDDED.to_string(),
    };
    let all = serde_json::from_str::<Cases>(&text).map_err(|e| format!("cases: {e}"))?;
    println!("FEM backend against the finite-difference reference cases\n");
    println!(
        "{:<26} {:>9} {:>9} {:>9} {:>9} {:>9} {:>9} {:>7}  result",
        "case", "Z0 [Ohm]", "ref Z0", "dZ0 [%]", "deps [%]", "dac [%]", "dad [%]", "time"
    );
    let (mut failed, mut total) = (0, 0);
    // the two manual-mesh cases hold the value of a deliberately coarse grid
    for c in all.cases.iter().filter(|c| !c.name.starts_with("manual_mesh")) {
        total += 1;
        let t0 = Instant::now();
        // the grid's domain extension is part of the reference value: same domain, automatic mesh levels
        let p = CpwgParams { mesh_mode: MeshMode::Auto, margin: c.margin, ..c.params.clone() };
        let a = match full_analysis_fem(&p, &Ctl::new()) {
            Ok(a) => a,
            Err(e) => {
                println!("{:<26} ERROR: {e}", c.name);
                failed += 1;
                continue;
            }
        };
        let (dz, de) = (rel(a.sol.z0, c.z0), rel(a.sol.eeff, c.eeff));
        let lp = a.loss.at(p.f);
        let (dac, dad) = (rel(lp.alpha_c, c.loss.alpha_c), rel(lp.alpha_d, c.loss.alpha_d));
        let tol_split = if p.t == 0.0 { TOL_SPLIT_T0 } else { TOL_SPLIT };
        let split_dev = GroundCat::ALL
            .iter()
            .map(|&g| (a.split.current.get(g) - c.split.current[g.key()]).abs())
            .fold(0.0, f64::max);
        let mut ok =
            dz < TOL_Z0 && de < TOL_EEFF && dac < TOL_LOSS && dad < TOL_LOSS && split_dev < tol_split;
        let mut note = format!(" ({} nodes, split {split_dev:.2} pp)", a.sol.mesh.pts.len());
        if let Some(az) = c.analytic_z0 {
            ok &= rel(a.sol.z0, az) < TOL_ANALYTIC;
            note.push_str(&format!(" analytic {:+.3} %", (a.sol.z0 / az - 1.0) * 100.0));
        }
        failed += usize::from(!ok);
        println!(
            "{:<26} {:>9.4} {:>9.4} {:>9.5} {:>9.5} {:>9.5} {:>9.5} {:>6.2}s  {}{note}",
            c.name,
            a.sol.z0,
            c.z0,
            dz * 100.0,
            de * 100.0,
            dac * 100.0,
            dad * 100.0,
            t0.elapsed().as_secs_f64(),
            if ok { "ok" } else { "FAIL" }
        );
    }
    println!("\n{} of {total} checks passed", total - failed);
    Ok(if failed == 0 { ExitCode::SUCCESS } else { ExitCode::FAILURE })
}
