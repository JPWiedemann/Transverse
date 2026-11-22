//! Command-line front end: batch analysis, synthesis and self-test.
//!
//! ```text
//! transverse analyze [params.json|-] [--json]                 analyse one geometry (missing fields use defaults)
//! transverse synth <w|s|w_total> <target_ohm> [params.json|-] [--json]
//! transverse selftest [cases.json]                            compare against the Python reference cases
//! ```

mod selftest;

use std::io::Read;
use std::process::ExitCode;
use std::time::Instant;

use transverse_core::{C_LIGHT, Ctl};
use transverse_lines::cpwg_inner::{self, CpwgParams, SynthVar, full_analysis};

const USAGE: &str = "usage:
  transverse analyze [params.json|-] [--json]
  transverse synth <w|s|w_total> <target_ohm> [params.json|-] [--json]
  transverse selftest [cases.json]
  transverse --version";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("analyze") => cmd_analyze(&args[1..]),
        Some("synth") => cmd_synth(&args[1..]),
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
        let out = serde_json::json!({
            "params": p,
            "z0": a.sol.z0, "eeff": a.sol.eeff, "l": a.sol.l, "c": a.sol.c, "c0": a.sol.c0,
            "g_over_omega": a.sol.g_over_omega, "delay_ps": delay_ps,
            "dzdn": a.loss.dzdn, "loss": a.loss.at(p.f),
            "split": a.split, "accuracy": a.sol.accuracy, "seconds": secs,
        });
        println!("{}", serde_json::to_string_pretty(&out).unwrap());
    } else {
        print!("{}", cpwg_inner::report(title, p, a));
        println!("Time            : {secs:.2} s");
    }
}

fn cmd_analyze(args: &[String]) -> Result<ExitCode, String> {
    let json_out = args.iter().any(|a| a == "--json");
    let path = args.iter().find(|a| *a != "--json");
    let p = read_params(path.map(String::as_str))?;
    let t0 = Instant::now();
    let a = full_analysis(&p, &progress()).map_err(|e| e.to_string())?;
    print_analysis("Analysis", &p, &a, t0.elapsed().as_secs_f64(), json_out);
    Ok(ExitCode::SUCCESS)
}

fn cmd_synth(args: &[String]) -> Result<ExitCode, String> {
    let json_out = args.iter().any(|a| a == "--json");
    let pos: Vec<&String> = args.iter().filter(|a| *a != "--json").collect();
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
    let (pp, _) =
        cpwg_inner::synthesize(&p, target, var, cpwg_inner::DEFAULT_RTOL, &ctl).map_err(|e| e.to_string())?;
    eprintln!("Synthesis done, running full analysis ...");
    let a = full_analysis(&pp, &ctl).map_err(|e| e.to_string())?;
    let title = format!("Synthesis: Z0 = {target} Ohm");
    print_analysis(&title, &pp, &a, t0.elapsed().as_secs_f64(), json_out);
    Ok(ExitCode::SUCCESS)
}
