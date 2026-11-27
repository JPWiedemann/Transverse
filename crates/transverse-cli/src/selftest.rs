//! Self-test: re-run the reference cases exported from the Python implementation.

use std::collections::HashMap;
use std::process::ExitCode;
use std::time::Instant;

use serde::Deserialize;
use transverse_core::Ctl;
use transverse_lines::planar::{self, GroundCat, PlanarParams, SynthVar, full_analysis};

/// Reference cases compiled into the binary, so the self-test works without any files.
const EMBEDDED: &str = include_str!("../../../tests/reference/cases.json");

// Relative tolerances (phase 1 exit criteria: 0.05 % for Z0 / eps_eff / synthesis, 1 % for losses).
const TOL_Z0: f64 = 5e-4;
const TOL_EEFF: f64 = 5e-4;
const TOL_LOSS: f64 = 1e-2;
const TOL_SYNTH: f64 = 5e-4;
const TOL_ANALYTIC: f64 = 2.5e-3;
/// Percentage points.
const TOL_SPLIT: f64 = 0.1;

#[derive(Deserialize)]
struct Case {
    name: String,
    params: PlanarParams,
    z0: f64,
    eeff: f64,
    nodes: usize,
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
struct SynthCase {
    name: String,
    params: PlanarParams,
    target: f64,
    var: SynthVar,
    result: SynthResult,
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

pub fn run(path: Option<&str>) -> Result<ExitCode, String> {
    let text = match path {
        Some(p) => std::fs::read_to_string(p).map_err(|e| format!("{p}: {e}"))?,
        None => EMBEDDED.to_string(),
    };
    let all = serde_json::from_str::<Cases>(&text).map_err(|e| format!("cases: {e}"))?;

    println!(
        "{:<26} {:>9} {:>9} {:>9} {:>9} {:>9} {:>9} {:>7}  result",
        "case", "Z0 [Ohm]", "py Z0", "dZ0 [%]", "deps [%]", "dac [%]", "dad [%]", "time"
    );
    let mut failed = 0;
    for c in &all.cases {
        let t0 = Instant::now();
        let a = match full_analysis(&c.params, &Ctl::new()) {
            Ok(a) => a,
            Err(e) => {
                println!("{:<26} ERROR: {e}", c.name);
                failed += 1;
                continue;
            }
        };
        let (dz, de) = (rel(a.sol.z0, c.z0), rel(a.sol.eeff, c.eeff));
        let lp = a.loss.at(c.params.f);
        let (dac, dad) = (rel(lp.alpha_c, c.loss.alpha_c), rel(lp.alpha_d, c.loss.alpha_d));
        let split_dev = GroundCat::ALL
            .iter()
            .map(|&g| (a.split.current.get(g) - c.split.current[key(g)]).abs())
            .fold(0.0, f64::max);
        let mut ok =
            dz < TOL_Z0 && de < TOL_EEFF && dac < TOL_LOSS && dad < TOL_LOSS && split_dev < TOL_SPLIT;
        let mut note = String::new();
        if a.sol.accuracy.nodes != c.nodes {
            note.push_str(&format!(" (nodes {} vs {})", a.sol.accuracy.nodes, c.nodes));
        }
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

    println!("\n{:<26} {:>9} {:>11} {:>11} {:>9}  result", "synthesis", "target", "w [mm]", "py w", "dw [%]");
    for c in &all.synthesis {
        let t0 = Instant::now();
        let r = planar::synthesize(&c.params, c.target, c.var, planar::DEFAULT_RTOL, &Ctl::new());
        match r {
            Ok((p, sol)) => {
                let dev = rel(p.w, c.result.w).max(rel(p.s, c.result.s));
                let ok = dev < TOL_SYNTH && rel(sol.z0, c.target) < TOL_Z0;
                failed += usize::from(!ok);
                println!(
                    "{:<26} {:>9.3} {:>11.6} {:>11.6} {:>9.5}  {} ({:.2}s)",
                    c.name,
                    c.target,
                    p.w,
                    c.result.w,
                    dev * 100.0,
                    if ok { "ok" } else { "FAIL" },
                    t0.elapsed().as_secs_f64()
                );
            }
            Err(e) => {
                println!("{:<26} ERROR: {e}", c.name);
                failed += 1;
            }
        }
    }
    let total = all.cases.len() + all.synthesis.len();
    println!("\n{} of {total} checks passed", total - failed);
    Ok(if failed == 0 { ExitCode::SUCCESS } else { ExitCode::FAILURE })
}
