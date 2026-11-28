//! Text report of an analysis (same content as the Python tool).

use std::fmt::Write;

use super::{
    EtchMode, GroundCat, PlanarAnalysis, PlanarFemAnalysis, PlanarParams, Shares, Split, WideSide, analytic,
};
use transverse_core::losses::LossModel;
use transverse_core::units::format_g;
use transverse_core::{C_LIGHT, NP2DB, guided_wavelength_mm, phase_deg};

/// The lumped values of a solution (any solver).
struct Lumped {
    z0: f64,
    eeff: f64,
    l: f64,
    c: f64,
}

pub fn report(title: &str, p: &PlanarParams, a: &PlanarAnalysis) -> String {
    let sol = &a.sol;
    let mut s = body(title, p, &Lumped { z0: sol.z0, eeff: sol.eeff, l: sol.l, c: sol.c }, &a.loss, &a.split);
    let acc = &sol.accuracy;
    let _ = writeln!(
        s,
        "Mesh            : {} x {} = {} nodes, min. cell {:.2} um",
        acc.nx,
        acc.ny,
        acc.nodes,
        acc.dmin_actual * 1e3
    );
    if let (Some(e), Some((za, zb))) = (acc.est_err, acc.levels) {
        let _ = writeln!(
            s,
            "Accuracy        : levels {za:.3} / {zb:.3} Ohm -> extrapolated {:.3} Ohm (fine-level error {:.2} %, extrapolated value better)",
            sol.z0,
            e * 100.0
        );
    }
    domain_line(&mut s, p, acc.margin);
    s
}

/// Report of a finite-element analysis.
pub fn report_fem(title: &str, p: &PlanarParams, a: &PlanarFemAnalysis) -> String {
    let sol = &a.sol;
    let mut s = body(title, p, &Lumped { z0: sol.z0, eeff: sol.eeff, l: sol.l, c: sol.c }, &a.loss, &a.split);
    let acc = &sol.accuracy;
    let (nodes, elements) = (sol.mesh.pts.len(), sol.mesh.tris.len());
    let _ = writeln!(
        s,
        "Mesh            : finite elements (quadratic), {nodes} nodes, {elements} triangles, min. edge {:.2} um",
        super::fem_summary::min_edge(&sol.mesh) * 1e3
    );
    if acc.nodes.len() >= 2 {
        let k = acc.z0.len();
        let how = match (sol.adaptive, acc.order) {
            (true, _) => format!("adaptive refinement, {k} meshes"),
            (false, Some(_)) => format!("{k} mesh levels, extrapolated"),
            (false, None) => format!("{k} mesh levels"),
        };
        let _ = writeln!(
            s,
            "Accuracy        : {how}; Z0 of the last two {:.3} / {:.3} Ohm (estimated error {:.2} %)",
            acc.z0[k - 2],
            acc.z0[k - 1],
            acc.est_err * 100.0
        );
    }
    domain_line(&mut s, p, sol.margin);
    s
}

fn domain_line(s: &mut String, p: &PlanarParams, margin: f64) {
    if margin > 0.0 {
        let _ = writeln!(
            s,
            "Domain          : open structure, extension {margin:.3} mm (converged to < {} %)",
            p.tol / 4.0
        );
    }
}

/// The structure and its dimensions.
fn geometry_lines(w: &mut String, e: &PlanarParams) {
    let mut sizes = vec![format!("w = {:.2} um", e.w * 1e3)];
    if e.coplanar {
        sizes.push(format!("s = {:.2} um", e.s * 1e3));
    }
    sizes.push(format!("t = {:.2} um", e.t * 1e3));
    if e.fence1 || e.fence2 || e.side_walls {
        sizes.push(format!("g = {:.3} mm", e.g));
    }
    let _ = writeln!(w, "Geometry : {}", sizes.join(", "));
    if e.undercut() > 0.0 {
        let given = match e.etch {
            EtchMode::Factor => format!("etch factor {}", format_g(e.etch_factor)),
            _ => format!("width difference {:.2} um", e.etch_delta * 1e3),
        };
        let face = if e.etch_wide == WideSide::Bottom { "bottom" } else { "top" };
        let _ = writeln!(w, "Etching  : {given}, wide side at the {face}; w, s and g are measured there");
        let _ = writeln!(
            w,
            "           narrow width {:.2} um, undercut {:.2} um per side, flank angle {:.1} deg",
            e.narrow_width() * 1e3,
            e.undercut() * 1e3,
            e.flank_angle_deg()
        );
    }
    let yn = |on: bool, name: &str| if on { name.to_string() } else { format!("no {name}") };
    let mut parts = Vec::new();
    if !e.upper {
        parts.push("air above".to_string());
    }
    if e.upper {
        parts.push(yn(e.plane1, "plane 1"));
    }
    parts.push(yn(e.plane2, "plane 2"));
    if e.coplanar {
        if e.upper {
            parts.push(yn(e.fence1, "fence 1"));
        }
        parts.push(yn(e.fence2, "fence 2"));
    } else if e.side_walls {
        parts.push("via fences".to_string());
    }
    let total =
        if e.coplanar { format!("2s + w = {:.2} um   ", (2.0 * e.s + e.w) * 1e3) } else { String::new() };
    let _ = writeln!(w, "           {total}{}  [{}]\n", e.structure_name(), parts.join(", "));
}

fn body(title: &str, p: &PlanarParams, sol: &Lumped, loss: &LossModel, split: &Split) -> String {
    let lo = loss.at(p.f);
    let to_db_mm = NP2DB / 1000.0;
    let (ac, ad) = (lo.alpha_c * to_db_mm, lo.alpha_d * to_db_mm);
    let tot = ac + ad;
    let vp = C_LIGHT / sol.eeff.sqrt();
    let e = p.effective();
    let mut s = String::new();
    let w = &mut s;
    let _ = writeln!(w, "=== {title} ===");
    geometry_lines(w, &e);
    let _ = writeln!(w, "Z0              = {:8.3} Ohm", sol.z0);
    let _ = writeln!(w, "eps_eff         = {:8.4}", sol.eeff);
    let _ = writeln!(w, "v_p             = {:8.4} c", vp / C_LIGHT);
    let _ =
        writeln!(w, "Delay           = {:8.3} ps/mm   ({:.2} ps/in)", 1e12 / vp * 1e-3, 1e12 / vp * 0.0254);
    let _ = writeln!(w, "L'              = {:8.4} nH/mm", sol.l * 1e6);
    let _ = writeln!(w, "C'              = {:8.4} pF/mm\n", sol.c * 1e9);
    let _ = writeln!(w, "@ f = {} GHz", p.f / 1e9);
    let lambda = guided_wavelength_mm(p.f, sol.eeff);
    let _ = writeln!(
        w,
        "Wavelength      = {lambda:8.3} mm  (free space {:.3} mm)",
        guided_wavelength_mm(p.f, 1.0)
    );
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
        "Length {} mm : {:.3} dB, delay {:.1} ps, phase {:.2} deg ({:.4} wavelengths)\n",
        format_g(p.length),
        tot * p.length,
        1e12 / vp * p.length * 1e-3,
        phase_deg(p.length, lambda),
        p.length / lambda
    );

    let (fd, cur) = (&split.field, &split.current);
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
    if e.plane1 {
        row(w, "  ", 31, GroundCat::Plane1);
    }
    if e.plane2 {
        row(w, "  ", 31, GroundCat::Plane2);
    }
    if e.coplanar {
        let _ =
            writeln!(w, "  {:31}{:6.1} %   {:6.1} %", "Coplanar ground (total)", fd.coplanar, cur.coplanar);
        for c in GroundCat::COPLANAR {
            if c == GroundCat::CopOuter && fd.get(c) < 0.05 && cur.get(c) < 0.05 {
                continue;
            }
            row(w, "    ", 29, c);
        }
    }
    if e.fence1 {
        row(w, "  ", 31, GroundCat::Fence1);
    }
    if e.fence2 {
        row(w, "  ", 31, GroundCat::Fence2);
    }
    if e.side_walls {
        let sum = |s: &Shares| s.get(GroundCat::Fence1) + s.get(GroundCat::Fence2);
        let _ = writeln!(w, "  {:31}{:6.1} %   {:6.1} %", "Via fences", sum(fd), sum(cur));
    }
    if !p.closed() {
        row(w, "  ", 31, GroundCat::Boundary);
    }
    if let Some(x) = split.x90_planes {
        let _ = writeln!(w, "  90 % of plane current within |x| <= {x:.3} mm");
    }
    if e.coplanar {
        let _ = writeln!(w, "  90 % of coplanar current within {:.3} mm of the gap edge", split.x90_cop);
    }
    let _ = writeln!(w);

    if let Some(a) = analytic(p) {
        let note = match (a.method, a.exact) {
            ("conformal mapping", true) => String::new(),
            ("conformal mapping", false) if e.coplanar => {
                "  (rough: open side treated as filled half-space)".into()
            }
            (method, false) => format!("  (estimate: {method})"),
            (_, true) => String::new(),
        };
        let _ = writeln!(w, "Analytic (t=0)  : Z0 = {:.2} Ohm, eps_eff = {:.3}{note}", a.z0, a.eeff);
    }
    s
}
