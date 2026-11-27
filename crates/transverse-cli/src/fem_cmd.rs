//! The finite-element backend on the command line: `analyze --solver fem` and `compare`.

use std::time::Instant;

use transverse_core::Ctl;
use transverse_core::NP2DB;
use transverse_lines::cpwg_inner::{
    CpwgFemAnalysis, CpwgParams, GroundCat, analyze, analyze_fem, full_analysis_fem,
};

fn progress() -> Ctl {
    Ctl::new().with_progress(|m| eprintln!("{m}"))
}

fn print_fem(p: &CpwgParams, a: &CpwgFemAnalysis, secs: f64) {
    let s = &a.sol;
    println!("=== FEM analysis (quadratic elements) ===");
    println!("Z0              = {:8.3} Ohm", s.z0);
    println!("eps_eff         = {:8.4}", s.eeff);
    println!("L'              = {:8.4} nH/mm", s.l * 1e6);
    println!("C'              = {:8.4} pF/mm", s.c * 1e9);
    println!("G'/omega        = {:8.4} pF/mm", s.g_over_omega * 1e9);
    let levels: Vec<String> =
        s.accuracy.nodes.iter().zip(&s.accuracy.z0).map(|(n, z)| format!("{n} nodes: {z:.4}")).collect();
    println!("Mesh levels     : {}", levels.join(", "));
    let order =
        s.accuracy.order.map_or("not extrapolated".to_string(), |p| format!("extrapolated, order {p:.2}"));
    println!("Accuracy        : estimated error {:.3} % ({order})", s.accuracy.est_err * 100.0);
    if s.margin > 0.0 {
        println!("Domain          : open structure, extension {:.3} mm", s.margin);
    }
    let lo = a.loss.at(p.f);
    let (ac, ad) = (lo.alpha_c * NP2DB / 1000.0, lo.alpha_d * NP2DB / 1000.0);
    println!("@ f = {} GHz", p.f / 1e9);
    println!("Conductor loss  = {ac:8.5} dB/mm  ({:.4} dB/in)", ac * 25.4);
    println!("Dielectric loss = {ad:8.5} dB/mm  ({:.4} dB/in)", ad * 25.4);
    println!("Total loss      = {:8.5} dB/mm  ({:.4} dB/in)", ac + ad, (ac + ad) * 25.4);
    println!("Ground split                     E-field   Return current (HF)");
    for c in GroundCat::ALL {
        let (f, i) = (a.split.field.get(c), a.split.current.get(c));
        if f.abs() >= 0.05 || i.abs() >= 0.05 {
            println!("  {:31}{f:6.1} %   {i:6.1} %", c.label());
        }
    }
    println!("Time            : {secs:.2} s");
}

/// `analyze --solver fem`.
pub fn analyze_command(p: &CpwgParams, json_out: bool) -> Result<(), String> {
    let t0 = Instant::now();
    let a = full_analysis_fem(p, &progress()).map_err(|e| e.to_string())?;
    let (s, secs) = (&a.sol, t0.elapsed().as_secs_f64());
    if json_out {
        let out = serde_json::json!({
            "params": p, "solver": "fem", "z0": s.z0, "eeff": s.eeff, "l": s.l, "c": s.c, "c0": s.c0,
            "g_over_omega": s.g_over_omega, "nodes": s.mesh.pts.len(), "margin": s.margin,
            "levels": { "nodes": s.accuracy.nodes, "z0": s.accuracy.z0 },
            "est_err": s.accuracy.est_err, "order": s.accuracy.order, "seconds": secs,
            "dzdn": a.loss.dzdn, "loss": a.loss.at(p.f), "split": a.split,
        });
        println!("{}", serde_json::to_string_pretty(&out).unwrap());
    } else {
        print_fem(p, &a, secs);
    }
    Ok(())
}

/// `compare`: both solvers on one geometry, relative differences. The FEM uses the domain extension the grid
/// found, so the difference shows the discretisation only.
pub fn compare_command(p: &CpwgParams) -> Result<(), String> {
    let ctl = progress();
    let t0 = Instant::now();
    let fd = analyze(p, &ctl).map_err(|e| e.to_string())?;
    let t_fd = t0.elapsed().as_secs_f64();
    let q = CpwgParams { margin: fd.accuracy.margin, ..p.clone() };
    let t1 = Instant::now();
    let fem = analyze_fem(&q, &q.fem_options(), &ctl).map_err(|e| e.to_string())?;
    let t_fem = t1.elapsed().as_secs_f64();
    println!("{:<14}{:>14}{:>14}{:>12}", "", "FD", "FEM", "difference");
    for (name, a, b) in [
        ("Z0 [Ohm]", fd.z0, fem.z0),
        ("eps_eff", fd.eeff, fem.eeff),
        ("C' [pF/mm]", fd.c * 1e9, fem.c * 1e9),
        ("C0' [pF/mm]", fd.c0 * 1e9, fem.c0 * 1e9),
        ("G'/omega", fd.g_over_omega * 1e9, fem.g_over_omega * 1e9),
    ] {
        println!("{name:<14}{a:>14.5}{b:>14.5}{:>11.2e}", b / a - 1.0);
    }
    println!("{:<14}{:>14}{:>14}", "nodes", fd.accuracy.nodes, fem.mesh.pts.len());
    println!("{:<14}{:>13.2}s{:>13.2}s", "time", t_fd, t_fem);
    println!(
        "FEM: {} levels, estimated error {:.3} %, domain extension {:.3} mm",
        fem.accuracy.nodes.len(),
        fem.accuracy.est_err * 100.0,
        fem.margin
    );
    Ok(())
}
