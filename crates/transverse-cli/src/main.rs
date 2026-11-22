//! Command-line front end: batch analysis and self-test.
//!
//! ```text
//! transverse analyze [params.json|-] [--json]   analyse one geometry (missing fields use defaults)
//! transverse selftest [cases.json]              compare against the Python reference cases
//! ```

mod selftest;

use std::io::Read;
use std::process::ExitCode;
use std::time::Instant;

use transverse_core::{C_LIGHT, Ctl};
use transverse_lines::cpwg_inner::{CpwgParams, analytic, analyze};

const USAGE: &str =
    "usage:\n  transverse analyze [params.json|-] [--json]\n  transverse selftest [cases.json]";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("analyze") => cmd_analyze(&args[1..]),
        Some("selftest") => selftest::run(args.get(1).map(String::as_str)),
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

fn cmd_analyze(args: &[String]) -> Result<ExitCode, String> {
    let json_out = args.iter().any(|a| a == "--json");
    let path = args.iter().find(|a| *a != "--json");
    let text = match path.map(String::as_str) {
        None | Some("-") => {
            let mut s = String::new();
            std::io::stdin().read_to_string(&mut s).map_err(|e| e.to_string())?;
            s
        }
        Some(p) => std::fs::read_to_string(p).map_err(|e| format!("{p}: {e}"))?,
    };
    let text = if text.trim().is_empty() { "{}".to_string() } else { text };
    let p: CpwgParams = serde_json::from_str(&text).map_err(|e| format!("parameters: {e}"))?;

    let ctl = Ctl::new().with_progress(|m| eprintln!("{m}"));
    let t0 = Instant::now();
    let sol = analyze(&p, &ctl).map_err(|e| e.to_string())?;
    let secs = t0.elapsed().as_secs_f64();
    let delay_ps = p.length * 1e-3 * sol.eeff.sqrt() / C_LIGHT * 1e12;

    if json_out {
        let out = serde_json::json!({
            "z0": sol.z0, "eeff": sol.eeff, "l": sol.l, "c": sol.c, "c0": sol.c0,
            "g_over_omega": sol.g_over_omega, "delay_ps": delay_ps,
            "accuracy": sol.accuracy, "seconds": secs,
        });
        println!("{}", serde_json::to_string_pretty(&out).unwrap());
    } else {
        println!("Z0      = {:.4} Ohm", sol.z0);
        println!("eps_eff = {:.5}", sol.eeff);
        println!("L'      = {:.4} nH/mm", sol.l * 1e6);
        println!("C'      = {:.4} pF/mm", sol.c * 1e9);
        println!("delay   = {delay_ps:.3} ps over {} mm", p.length);
        let a = &sol.accuracy;
        println!(
            "mesh    = {} nodes ({} x {}), finest cell {:.2} um, margin {:.3} mm",
            a.nodes,
            a.nx,
            a.ny,
            a.dmin_actual * 1e3,
            a.margin
        );
        if let Some(e) = a.est_err {
            println!("error   ~ {:.3} % (difference of the two finest levels)", e * 100.0);
        }
        if p.t == 0.0 {
            let (z, ee, exact) = analytic(&p);
            println!(
                "analytic (t = 0) Z0 = {z:.4} Ohm, eps_eff = {ee:.4}{}",
                if exact { "" } else { " (approximate)" }
            );
        }
        println!("time    = {secs:.2} s");
    }
    Ok(ExitCode::SUCCESS)
}
