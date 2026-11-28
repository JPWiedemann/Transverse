//! Command-line front end: batch analysis, synthesis and self-test.
//!
//! ```text
//! transverse analyze [params.json|-] [--solver fem|fd] [--json]   analyse one geometry (missing fields use defaults)
//! transverse compare [params.json|-]                         finite difference vs finite elements on one geometry
//! transverse synth <w|d|s|w_total> <target_ohm> [params.json|-] [--phase <deg>] [--solver fem|fd] [--json]
//! transverse synth w_d <zdiff_ohm> <zcomm_ohm> [params.json|-] [--phase <deg>] [--json]
//! transverse selftest [--solver fem|fd] [cases.json]          compare against the Python reference cases
//! ```
//!
//! With `"signals": "pair"` the line is a differential pair: `<target_ohm>` is Zdiff, `d` (the pair gap) can be
//! varied, and `w_d` finds width and gap for Zdiff and Zcomm together.
//!
//! The finite elements are the solver of the apps and the default here; `--solver fd` selects the finite-difference
//! grid, the reference of the tests. `selftest` without `--solver` checks both.

mod fem_cmd;
mod selftest;
mod selftest_fem;

use std::io::Read;
use std::process::ExitCode;
use std::time::Instant;

use transverse_core::{C_LIGHT, Ctl, guided_wavelength_mm, length_for_phase_mm, phase_deg};
use transverse_lines::planar::{self, PlanarParams, SynthVar, full_analysis};

const USAGE: &str = "usage:
  transverse analyze [params.json|-] [--solver fem|fd] [--json]
  transverse compare [params.json|-]
  transverse synth <w|d|s|w_total> <target_ohm> [params.json|-] [--phase <deg>] [--solver fem|fd] [--json]
  transverse synth w_d <zdiff_ohm> <zcomm_ohm> [params.json|-] [--phase <deg>] [--json]
  transverse selftest [--solver fem|fd] [cases.json]
  transverse --version";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("analyze") => cmd_analyze(&args[1..]),
        Some("synth") => cmd_synth(&args[1..]),
        Some("compare") => cmd_compare(&args[1..]),
        Some("selftest") => cmd_selftest(&args[1..]),
        Some("--version" | "-V" | "version") => {
            println!("transverse {}", env!("CARGO_PKG_VERSION"));
            Ok(ExitCode::SUCCESS)
        }
        _ => Err(USAGE.to_string()),
    };
    match result {
        Ok(code) => code,
        Err(msg) => {
            eprintln!("{msg}");
            ExitCode::from(2)
        }
    }
}

/// Parameters from a file, stdin ("-" or no argument); missing fields use the defaults.
fn read_params(path: Option<&str>) -> Result<PlanarParams, String> {
    let text = match path {
        None | Some("-") => {
            let mut s = String::new();
            std::io::stdin().read_to_string(&mut s).map_err(|e| e.to_string())?;
            s
        }
        Some(p) => std::fs::read_to_string(p).map_err(|e| format!("{p}: {e}"))?,
    };
    let text = if text.trim().is_empty() { "{}".to_string() } else { text };
    serde_json::from_str(&text).map_err(|e| format!("parameters: {e}"))
}

fn progress() -> Ctl {
    Ctl::new().with_progress(|m| eprintln!("{m}"))
}

fn print_analysis(title: &str, p: &PlanarParams, a: &planar::PlanarAnalysis, secs: f64, json_out: bool) {
    if json_out {
        let delay_ps = p.length * 1e-3 * a.sol.eeff.sqrt() / C_LIGHT * 1e12;
        let wavelength_mm = guided_wavelength_mm(p.f, a.sol.eeff);
        let out = serde_json::json!({
            "params": p,
            "z0": a.sol.z0, "eeff": a.sol.eeff, "l": a.sol.l, "c": a.sol.c, "c0": a.sol.c0,
            "g_over_omega": a.sol.g_over_omega, "delay_ps": delay_ps,
            "wavelength_mm": wavelength_mm, "phase_deg": phase_deg(p.length, wavelength_mm),
            "dzdn": a.loss.dzdn, "loss": a.loss.at(p.f),
            "split": a.split, "accuracy": a.sol.accuracy, "seconds": secs,
        });
        println!("{}", serde_json::to_string_pretty(&out).unwrap());
    } else {
        print!("{}", planar::report(title, p, a));
        println!("Time            : {secs:.2} s");
    }
}

/// The value of `--solver`: `true` for the finite elements, `None` if the option is not given.
fn solver_option(args: &[String]) -> Result<Option<bool>, String> {
    let Some(i) = args.iter().position(|a| a == "--solver") else { return Ok(None) };
    match args.get(i + 1).map(String::as_str) {
        Some("fem") => Ok(Some(true)),
        Some("fd") => Ok(Some(false)),
        _ => Err("--solver needs fem or fd".to_string()),
    }
}

/// The arguments that are not options (nor the values of options).
fn positional(args: &[String]) -> Vec<&String> {
    let mut out = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--json" => {}
            "--solver" | "--phase" => {
                it.next();
            }
            _ => out.push(a),
        }
    }
    out
}

fn cmd_selftest(args: &[String]) -> Result<ExitCode, String> {
    let path = positional(args).first().map(|s| s.as_str());
    let passed = match solver_option(args)? {
        Some(true) => selftest_fem::run(path)?,
        Some(false) => selftest::run(path)?,
        None => {
            let grid = selftest::run(path)?;
            println!();
            let elements = selftest_fem::run(path)?;
            grid && elements
        }
    };
    Ok(if passed { ExitCode::SUCCESS } else { ExitCode::FAILURE })
}

fn cmd_compare(args: &[String]) -> Result<ExitCode, String> {
    let p = read_params(args.first().map(String::as_str))?;
    fem_cmd::compare_command(&p)?;
    Ok(ExitCode::SUCCESS)
}

fn cmd_analyze(args: &[String]) -> Result<ExitCode, String> {
    let json_out = args.iter().any(|a| a == "--json");
    let fem = solver_option(args)?.unwrap_or(true);
    let p = read_params(positional(args).first().map(|s| s.as_str()))?;
    if fem {
        fem_cmd::analyze_command(&p, json_out)?;
        return Ok(ExitCode::SUCCESS);
    }
    let t0 = Instant::now();
    let a = full_analysis(&p, &progress()).map_err(|e| e.to_string())?;
    print_analysis("Analysis", &p, &a, t0.elapsed().as_secs_f64(), json_out);
    Ok(ExitCode::SUCCESS)
}

fn cmd_synth(args: &[String]) -> Result<ExitCode, String> {
    let json_out = args.iter().any(|a| a == "--json");
    // --phase <deg>: also set the line length for that electrical length (it follows from eps_eff)
    let phase = match args.iter().position(|a| a == "--phase") {
        Some(i) => Some(
            args.get(i + 1)
                .and_then(|s| s.parse::<f64>().ok())
                .filter(|v| v.is_finite() && *v > 0.0)
                .ok_or("--phase needs a positive number of degrees")?,
        ),
        None => None,
    };
    let pos = positional(args);
    if pos.first().is_some_and(|s| s.as_str() == "w_d") {
        return cmd_synth_pair(&pos[1..], phase, solver_option(args)?, json_out);
    }
    let var = match pos.first().map(|s| s.as_str()) {
        Some("w") => SynthVar::W,
        Some("s") => SynthVar::S,
        Some("w_total") => SynthVar::WTotal,
        Some("d") => SynthVar::D,
        _ => return Err(USAGE.to_string()),
    };
    let target: f64 = pos.get(1).and_then(|s| s.parse().ok()).ok_or(USAGE.to_string())?;
    let p = read_params(pos.get(2).map(|s| s.as_str()))?;

    let ctl = progress();
    let t0 = Instant::now();

    if solver_option(args)?.unwrap_or(true) {
        let (mut pp, sol) =
            planar::synthesize_fem(&p, target, var, planar::DEFAULT_RTOL, &ctl).map_err(|e| e.to_string())?;
        let pair_gap = if pp.pair() { format!(", d = {:.4} mm", pp.d) } else { String::new() };
        eprintln!(
            "Synthesis done: w = {:.4} mm, s = {:.4} mm{pair_gap}, running full analysis ...",
            pp.w, pp.s
        );
        if let Some(ph) = phase {
            pp.length = length_for_phase_mm(pp.f, sol.eeff, ph);
        }
        fem_cmd::analyze_command(&pp, json_out)?;
        return Ok(ExitCode::SUCCESS);
    }

    let (mut pp, sol) =
        planar::synthesize(&p, target, var, planar::DEFAULT_RTOL, &ctl).map_err(|e| e.to_string())?;
    let mut title = format!("Synthesis: Z0 = {target} Ohm");
    if let Some(ph) = phase {
        pp.length = length_for_phase_mm(pp.f, sol.eeff, ph);
        title += &format!(", phase = {ph} deg at {} GHz -> length", pp.f / 1e9);
    }
    eprintln!("Synthesis done, running full analysis ...");
    let a = full_analysis(&pp, &ctl).map_err(|e| e.to_string())?;
    print_analysis(&title, &pp, &a, t0.elapsed().as_secs_f64(), json_out);
    Ok(ExitCode::SUCCESS)
}

/// `synth w_d <zdiff> <zcomm>`: width and gap of a pair (finite elements, the only solver for pairs).
fn cmd_synth_pair(
    pos: &[&String],
    phase: Option<f64>,
    solver: Option<bool>,
    json_out: bool,
) -> Result<ExitCode, String> {
    let target = |k: usize| pos.get(k).and_then(|s| s.parse::<f64>().ok()).ok_or(USAGE.to_string());
    let (zdiff, zcomm) = (target(0)?, target(1)?);
    if solver == Some(false) {
        return Err("the grid solver has one signal conductor only: use the finite elements".to_string());
    }
    let p = read_params(pos.get(2).map(|s| s.as_str()))?;
    let (mut pp, sol) = planar::synthesize_pair_fem(&p, zdiff, zcomm, planar::DEFAULT_RTOL, &progress())
        .map_err(|e| e.to_string())?;
    eprintln!("Synthesis done: w = {:.4} mm, d = {:.4} mm, running full analysis ...", pp.w, pp.d);
    if let Some(ph) = phase {
        pp.length = length_for_phase_mm(pp.f, sol.eeff, ph);
    }
    fem_cmd::analyze_command(&pp, json_out)?;
    Ok(ExitCode::SUCCESS)
}
