//! Conductor loss by Wheeler's incremental-inductance rule.
//!
//! All metal walls are moved by +dn / -dn in the air solution; dZ0/dn gives the loss.

use transverse_core::quasistatic::Solution;
use transverse_core::{Ctl, TvResult};

use super::{CpwgParams, GapMode, MeshMode, analyze};

/// dZ0_air/dn [Ohm/m] with the same mesh settings and domain as `sol`.
pub fn wheeler_dzdn(p: &CpwgParams, sol: &Solution, ctl: &Ctl) -> TvResult<f64> {
    // wall shift relative to the geometry
    let mut dn = (p.s / 20.0).min(p.w / 20.0);
    if p.t > 0.0 {
        dn = dn.min(p.t / 8.0);
    }
    let (dmin, dmax, richardson) = sol.accuracy.mesh_used;
    let pa = CpwgParams {
        er1: 1.0,
        er2: 1.0,
        gap_mode: GapMode::Custom,
        er_gap: 1.0,
        mesh_mode: MeshMode::Manual,
        dmin,
        dmax,
        richardson,
        margin: if sol.accuracy.margin > 0.0 { sol.accuracy.margin } else { 1.0 },
        ..p.clone()
    };
    let fence = p.fence1 || p.fence2;
    let z = |d: f64| -> TvResult<f64> {
        ctl.msg(&format!("Conductor loss (Wheeler): wall shift {:+.2} um ...", d * 1e3));
        let q = CpwgParams {
            w: p.w - 2.0 * d,
            s: p.s + d,
            t: if p.t > 0.0 { (p.t - 2.0 * d).max(0.0) } else { 0.0 },
            h1: if p.plane1 { p.h1 + d } else { p.h1 },
            h2: if p.plane2 { p.h2 + d } else { p.h2 },
            g: if fence { p.g + d } else { p.g },
            ..pa.clone()
        };
        Ok(analyze(&q, ctl)?.z0)
    };
    Ok((z(dn)? - z(-dn)?) / (2.0 * dn * 1e-3))
}
