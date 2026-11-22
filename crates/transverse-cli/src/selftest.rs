//! Self-test: re-run the reference cases exported from the Python implementation.

use std::process::ExitCode;
use std::time::Instant;

use serde::Deserialize;
use transverse_core::Ctl;
use transverse_lines::cpwg_inner::{CpwgParams, analyze};

/// Reference cases compiled into the binary, so the self-test works without any files.
const EMBEDDED: &str = include_str!("../../../tests/reference/cases.json");

/// Relative tolerances (phase 1 exit criterion is 0.05 % for Z0 and eps_eff).
const TOL_Z0: f64 = 5e-4;
const TOL_EEFF: f64 = 5e-4;
const TOL_ANALYTIC: f64 = 2.5e-3;

#[derive(Deserialize)]
struct Case {
    name: String,
    params: CpwgParams,
    z0: f64,
    eeff: f64,
    nodes: usize,
    analytic_z0: Option<f64>,
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
    let cases = serde_json::from_str::<Cases>(&text).map_err(|e| format!("cases: {e}"))?.cases;

    println!(
        "{:<28} {:>10} {:>10} {:>9} {:>9} {:>8}  result",
        "case", "Z0 [Ohm]", "py Z0", "dZ0 [%]", "deps [%]", "time"
    );
    let mut failed = 0;
    for c in &cases {
        let t0 = Instant::now();
        let sol = match analyze(&c.params, &Ctl::new()) {
            Ok(s) => s,
            Err(e) => {
                println!("{:<28} ERROR: {e}", c.name);
                failed += 1;
                continue;
            }
        };
        let (dz, de) = (rel(sol.z0, c.z0), rel(sol.eeff, c.eeff));
        let mut ok = dz < TOL_Z0 && de < TOL_EEFF;
        let mut note = String::new();
        if sol.accuracy.nodes != c.nodes {
            note.push_str(&format!(" (nodes {} vs {})", sol.accuracy.nodes, c.nodes));
        }
        if let Some(az) = c.analytic_z0 {
            let da = rel(sol.z0, az);
            ok &= da < TOL_ANALYTIC;
            note.push_str(&format!(" analytic {:+.3} %", (sol.z0 / az - 1.0) * 100.0));
        }
        failed += usize::from(!ok);
        println!(
            "{:<28} {:>10.4} {:>10.4} {:>9.5} {:>9.5} {:>7.2}s  {}{note}",
            c.name,
            sol.z0,
            c.z0,
            dz * 100.0,
            de * 100.0,
            t0.elapsed().as_secs_f64(),
            if ok { "ok" } else { "FAIL" }
        );
    }
    println!("{} of {} cases passed", cases.len() - failed, cases.len());
    Ok(if failed == 0 { ExitCode::SUCCESS } else { ExitCode::FAILURE })
}
