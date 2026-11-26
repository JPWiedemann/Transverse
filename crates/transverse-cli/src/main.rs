//! Command-line front end: batch analysis, synthesis and self-test.
//!
//! ```text
//! transverse analyze [params.json|-] [--solver fd|fem] [--json]   analyse one geometry (missing fields use defaults)
//! transverse compare [params.json|-]                         finite difference vs finite elements on one geometry
//! transverse synth <w|s|w_total> <target_ohm> [params.json|-] [--phase <deg>] [--json]
//! transverse selftest [cases.json]                            compare against the Python reference cases
//! ```

mod fem_cmd;
mod selftest;

use std::io::Read;
use std::process::ExitCode;
use std::time::Instant;

use transverse_core::{C_LIGHT, Ctl, guided_wavelength_mm, length_for_phase_mm, phase_deg};
use transverse_lines::cpwg_inner::{self, CpwgParams, SynthVar, full_analysis};

const USAGE: &str = "usage:
  transverse analyze [params.json|-] [--solver fd|fem] [--json]
  transverse compare [params.json|-]
  transverse synth <w|s|w_total> <target_ohm> [params.json|-] [--phase <deg>] [--json]
  transverse selftest [cases.json]
  transverse --version";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("analyze") => cmd_analyze(&args[1..]),
        Some("synth") => cmd_synth(&args[1..]),
        Some("compare") => cmd_compare(&args[1..]),
        Some("selftest") => selftest::run(args.get(1).map(String::as_str)),
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
fn read_params(path: Option<&str>) -> Result<CpwgParams, String> {
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

fn print_analysis(title: &str, p: &CpwgParams, a: &cpwg_inner::CpwgAnalysis, secs: f64, json_out: bool) {
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
        print!("{}", cpwg_inner::report(title, p, a));
        println!("Time            : {secs:.2} s");
    }
}

fn cmd_compare(args: &[String]) -> Result<ExitCode, String> {
    let p = read_params(args.first().map(String::as_str))?;
    fem_cmd::compare_command(&p)?;
    Ok(ExitCode::SUCCESS)
}

fn cmd_analyze(args: &[String]) -> Result<ExitCode, String> {
    let json_out = args.iter().any(|a| a == "--json");
    // --solver fd|fem (default fd)
    let mut fem = false;
    let mut path: Option<&String> = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--json" => {}
            "--solver" => match it.next().map(String::as_str) {
                Some("fd") => fem = false,
                Some("fem") => fem = true,
                _ => return Err("--solver needs fd or fem".to_string()),
            },
            _ => path = Some(a),
        }
    }
    let p = read_params(path.map(String::as_str))?;
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
    let mut pos: Vec<&String> = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--json" => {}
            "--phase" => {
                it.next();
            }
            _ => pos.push(a),
        }
    }
    let var = match pos.first().map(|s| s.as_str()) {
        Some("w") => SynthVar::W,
        Some("s") => SynthVar::S,
        Some("w_total") => SynthVar::WTotal,
        _ => return Err(USAGE.to_string()),
    };
    let target: f64 = pos.get(1).and_then(|s| s.parse().ok()).ok_or(USAGE.to_string())?;
    let p = read_params(pos.get(2).map(|s| s.as_str()))?;

    let ctl = progress();
    let t0 = Instant::now();

    let (mut pp, sol) =
        cpwg_inner::synthesize(&p, target, var, cpwg_inner::DEFAULT_RTOL, &ctl).map_err(|e| e.to_string())?;
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
