//! Conductor and dielectric loss of the FEM backend.
//!
//! Conductor loss by Wheeler's incremental-inductance rule, exactly as for the grid ([`super::wheeler`]):
//! all metal walls move by +dn / -dn in the air solution and dZ0/dn gives the loss. The shifted geometries are
//! not meshed anew: the final mesh of the main solution is moved with the walls ([`super::fem_morph`]). Both
//! solves then have the same elements, so their discretisation errors cancel in the difference.

use transverse_core::losses::LossModel;
use transverse_core::{Ctl, TvResult};

use super::fem::{FemSolution, solve_mesh};
use super::fem_morph::move_mesh;
use super::wheeler::wheeler_step;
use super::{GapMode, MeshMode, PlanarParams};

/// dZ0_air/dn [Ohm/m] with the mesh settings and domain of `sol`.
pub fn wheeler_dzdn_fem(p: &PlanarParams, sol: &FemSolution, ctl: &Ctl) -> TvResult<f64> {
    let dn = wheeler_step(p);
    let pa = PlanarParams {
        er1: 1.0,
        er2: 1.0,
        gap_mode: GapMode::Custom,
        er_gap: 1.0,
        mesh_mode: MeshMode::Auto,
        margin: if sol.margin > 0.0 { sol.margin } else { 1.0 },
        ..p.clone()
    };
    let z = |d: f64| -> TvResult<f64> {
        ctl.msg(&format!("Conductor loss (Wheeler): wall shift {:+.2} um ...", d * 1e3));
        let q = pa.walls_receded(d);
        let mesh = move_mesh(&sol.mesh, p, &q, sol.margin, q.margin);
        let run = solve_mesh(&q.cross_section(q.margin), mesh, sol.order, ctl)?;
        Ok(run.lumped.z0())
    };
    Ok((z(dn)? - z(-dn)?) / (2.0 * dn * 1e-3))
}

/// The loss model of a FEM solution: dielectric loss from G/omega, conductor loss from Wheeler.
pub fn loss_model_fem(p: &PlanarParams, sol: &FemSolution, ctl: &Ctl) -> TvResult<LossModel> {
    Ok(LossModel {
        z0: sol.z0,
        eeff: sol.eeff,
        g_over_omega: sol.g_over_omega,
        dzdn: wheeler_dzdn_fem(p, sol, ctl)?,
        sigma: p.sigma,
        rq_mm: p.rq,
    })
}
