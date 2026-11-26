//! Conductor and dielectric loss of the FEM backend.
//!
//! Conductor loss by Wheeler's incremental-inductance rule, exactly as for the grid ([`super::wheeler`]):
//! all metal walls move by +dn / -dn in the air solution and dZ0/dn gives the loss. Both shifted
//! geometries are meshed with the size field of the finest level of the main solution (a single level, no
//! extrapolation), so their discretisation errors are alike and largely cancel in the difference.

use transverse_core::losses::LossModel;
use transverse_core::{Ctl, TvResult};

use super::fem::{FemSolution, solve_level};
use super::{CpwgParams, GapMode, MeshMode};

/// dZ0_air/dn [Ohm/m] with the mesh settings and domain of `sol`.
pub fn wheeler_dzdn_fem(p: &CpwgParams, sol: &FemSolution, max_nodes: usize, ctl: &Ctl) -> TvResult<f64> {
    // wall shift relative to the geometry (the same rule as for the grid)
    let mut dn = (p.s / 20.0).min(p.w / 20.0);
    if p.t > 0.0 {
        dn = dn.min(p.t / 8.0);
    }
    let pa = CpwgParams {
        er1: 1.0,
        er2: 1.0,
        gap_mode: GapMode::Custom,
        er_gap: 1.0,
        mesh_mode: MeshMode::Auto,
        margin: if sol.margin > 0.0 { sol.margin } else { 1.0 },
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
        // the same absolute sizes as the finest level of the main solution (the shifted geometry would have its
        // own base sizes, a few percent different, and its own mesh noise)
        let spec = q.size_spec_like(q.margin, &sol.spec);
        let run = solve_level(&q, q.margin, &spec, max_nodes, sol.order, ctl)?;
        Ok(run.lumped.z0())
    };
    Ok((z(dn)? - z(-dn)?) / (2.0 * dn * 1e-3))
}

/// The loss model of a FEM solution: dielectric loss from G/omega, conductor loss from Wheeler.
pub fn loss_model_fem(p: &CpwgParams, sol: &FemSolution, max_nodes: usize, ctl: &Ctl) -> TvResult<LossModel> {
    Ok(LossModel {
        z0: sol.z0,
        eeff: sol.eeff,
        g_over_omega: sol.g_over_omega,
        dzdn: wheeler_dzdn_fem(p, sol, max_nodes, ctl)?,
        sigma: p.sigma,
        rq_mm: p.rq,
    })
}
