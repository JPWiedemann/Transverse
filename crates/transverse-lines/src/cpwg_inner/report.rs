//! Text report of an analysis (same content as the Python tool).

use std::fmt::Write;

use transverse_core::{C_LIGHT, NP2DB};
use transverse_lines::cpwg_inner::{Analysis, CpwgParams, GroundCat, analytic};

pub fn report(title: &str, p: &CpwgParams, a: &Analysis) -> String {
    let sol = &a.sol;
    let lo = a.loss.at(p.f);
    let to_db_mm = NP2DB / 1000.0;
    let (ac, ad) = (lo.alpha_c * to_db_mm, lo.alpha_d * to_db_mm);
    let tot = ac + ad;
    let vp = C_LIGHT / sol.eeff.sqrt();
    let yn = |on: bool, name: &str| if on { name.to_string() } else { format!("no {name}") };
    let mut s = String::new();
    let w = &mut s;
    let _ = writeln!(w, "=== {title} ===");
    let _ = writeln!(
        w,
        "Geometry : w = {:.2} um, s = {:.2} um, t = {:.2} um, g = {:.3} mm",
        p.w * 1e3,
        p.s * 1e3,
        p.t * 1e3,
        p.g
    );
    let _ = writeln!(
        w,
        "           2s + w = {:.2} um   [{}, {}, {}, {}]\n",
        (2.0 * p.s + p.w) * 1e3,
        yn(p.plane1, "plane 1"),
        yn(p.plane2, "plane 2"),
        yn(p.fence1, "fence 1"),
        yn(p.fence2, "fence 2")
    );
    let _ = writeln!(w, "Z0              = {:8.3} Ohm", sol.z0);
    let _ = writeln!(w, "eps_eff         = {:8.4}", sol.eeff);
    let _ = writeln!(w, "v_p             = {:8.4} c", vp / C_LIGHT);
    let _ =
        writeln!(w, "Delay           = {:8.3} ps/mm   ({:.2} ps/in)", 1e12 / vp * 1e-3, 1e12 / vp * 0.0254);
    let _ = writeln!(w, "L'              = {:8.4} nH/mm", sol.l * 1e6);
    let _ = writeln!(w, "C'              = {:8.4} pF/mm\n", sol.c * 1e9);
    let _ = writeln!(w, "@ f = {} GHz", p.f / 1e9);
    let mut warn = String::new();
    if p.t > 0.0 && p.t * 1e-3 < 3.0 * lo.delta {
        warn.push_str("   (!) t < 3*delta: Rs model inaccurate");
    }
    if p.t == 0.0 {
        warn.push_str("   (!) t = 0: conductor loss only indicative");
    }
    let _ = writeln!(w, "Skin depth      = {:8.3} um{warn}", lo.delta * 1e6);
    let _ = writeln!(w, "Rs              = {:8.3} mOhm/sq  (roughness factor {:.3})", lo.rs * 1e3, lo.krough);
    let _ = writeln!(w, "R'              = {:8.4} Ohm/mm", lo.r * 1e-3);
    let _ = writeln!(w, "G'              = {:8.5} mS/mm", lo.g * 1e3 * 1e-3);
    let _ = writeln!(w, "Conductor loss  = {ac:8.5} dB/mm  ({:.4} dB/in)", ac * 25.4);
    let _ = writeln!(w, "Dielectric loss = {ad:8.5} dB/mm  ({:.4} dB/in)", ad * 25.4);
    let _ = writeln!(w, "Total loss      = {tot:8.5} dB/mm  ({:.4} dB/in)", tot * 25.4);
    let _ = writeln!(
        w,
        "Length {} mm : {:.3} dB, delay {:.1} ps\n",
        p.length,
        tot * p.length,
        1e12 / vp * p.length * 1e-3
    );

    let (fd, cur) = (&a.split.field, &a.split.current);
    let _ = writeln!(w, "Ground split                     E-field   Return current (HF)");
    let row = |w: &mut String, indent: &str, width: usize, c: GroundCat| {
        let _ = writeln!(
            w,
            "{indent}{:width$}{:6.1} %   {:6.1} %",
            c.label(),
            fd.get(c),
            cur.get(c),
            width = width
        );
    };
    if p.plane1 {
        row(w, "  ", 31, GroundCat::Plane1);
    }
    if p.plane2 {
        row(w, "  ", 31, GroundCat::Plane2);
    }
    let _ = writeln!(w, "  {:31}{:6.1} %   {:6.1} %", "Coplanar ground (total)", fd.coplanar, cur.coplanar);
    for c in GroundCat::COPLANAR {
        if c == GroundCat::CopOuter && fd.get(c) < 0.05 && cur.get(c) < 0.05 {
            continue;
        }
        row(w, "    ", 29, c);
    }
    if p.fence1 {
        row(w, "  ", 31, GroundCat::Fence1);
    }
    if p.fence2 {
        row(w, "  ", 31, GroundCat::Fence2);
    }
    if !p.closed() {
        row(w, "  ", 31, GroundCat::Boundary);
    }
    if let Some(x) = a.split.x90_planes {
        let _ = writeln!(w, "  90 % of plane current within |x| <= {x:.3} mm");
    }
    let _ = writeln!(w, "  90 % of coplanar current within {:.3} mm of the gap edge\n", a.split.x90_cop);

    let (za, ea, exact) = analytic(p);
    let rough = if exact { "" } else { "  (rough: open side treated as filled half-space)" };
    let _ = writeln!(w, "Analytic (t=0)  : Z0 = {za:.2} Ohm, eps_eff = {ea:.3}{rough}");
    let acc = &sol.accuracy;
    let _ = writeln!(
        w,
        "Mesh            : {} x {} = {} nodes, min. cell {:.2} um",
        acc.nx,
        acc.ny,
        acc.nodes,
        acc.dmin_actual * 1e3
    );
    if let (Some(e), Some((za, zb))) = (acc.est_err, acc.levels) {
        let _ = writeln!(
            w,
            "Accuracy        : levels {za:.3} / {zb:.3} Ohm -> extrapolated {:.3} Ohm (fine-level error {:.2} %, extrapolated value better)",
            sol.z0,
            e * 100.0
        );
    }
    if acc.margin > 0.0 {
        let _ = writeln!(
            w,
            "Domain          : open structure, extension {:.3} mm (converged to < {} %)",
            acc.margin,
            p.tol / 4.0
        );
    }
    s
}
