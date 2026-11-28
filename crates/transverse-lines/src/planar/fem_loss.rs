//! Conductor and dielectric loss of the FEM backend.
//!
//! Conductor loss by Wheeler's incremental-inductance rule, exactly as for the grid ([`super::wheeler`]):
//! all metal walls move by +dn / -dn in the air solution and dZ0/dn gives the loss, for every mode from its
//! equivalent line. The shifted geometries are not meshed anew: the final mesh of the main solution is moved with
//! the walls ([`super::fem_morph`]). Both solves then have the same elements, so their discretisation errors
//! cancel in the difference.

use transverse_core::losses::LossModel;
use transverse_core::{Ctl, TvResult};

use super::fem::FemSolution;
use super::fem_modes::mode_values;
use super::fem_morph::move_mesh;
use super::wheeler::wheeler_step;
use super::{GapMode, MeshMode, PlanarParams};
use crate::section_solve::solve_mesh;

/// dZ0_air/dn [Ohm/m] of every mode of `sol`, with its mesh settings and domain.
fn wheeler_dzdn_modes(p: &PlanarParams, sol: &FemSolution, ctl: &Ctl) -> TvResult<Vec<f64>> {
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
    let z = |d: f64| -> TvResult<Vec<f64>> {
        ctl.msg(&format!("Conductor loss (Wheeler): wall shift {:+.2} um ...", d * 1e3));
        let q = pa.walls_receded(d);
        let mesh = move_mesh(&sol.mesh, p, &q, sol.margin, q.margin);
        let run = solve_mesh(&q.cross_section(q.margin), mesh, sol.order, ctl)?;
        Ok(mode_values(run.caps)?.1.iter().map(|m| m.z0).collect())
    };
    let (out, back) = (z(dn)?, z(-dn)?);
    Ok(out.iter().zip(&back).map(|(a, b)| (a - b) / (2.0 * dn * 1e-3)).collect())
}

/// dZ0_air/dn [Ohm/m] of the mode `sol` is in.
pub fn wheeler_dzdn_fem(p: &PlanarParams, sol: &FemSolution, ctl: &Ctl) -> TvResult<f64> {
    Ok(wheeler_dzdn_modes(p, sol, ctl)?[sol.mode])
}

/// The loss model of every mode of a FEM solution: dielectric loss from G/omega, conductor loss from Wheeler.
pub fn loss_models_fem(p: &PlanarParams, sol: &FemSolution, ctl: &Ctl) -> TvResult<Vec<LossModel>> {
    let dzdn = wheeler_dzdn_modes(p, sol, ctl)?;
    Ok(sol
        .modes
        .iter()
        .zip(dzdn)
        .map(|(m, dzdn)| LossModel {
            z0: m.z0,
            eeff: m.eeff,
            g_over_omega: m.g_over_omega,
            dzdn,
            sigma: p.sigma,
            rq_mm: p.rq,
        })
        .collect())
}

/// The loss model of the mode `sol` is in.
pub fn loss_model_fem(p: &PlanarParams, sol: &FemSolution, ctl: &Ctl) -> TvResult<LossModel> {
    Ok(loss_models_fem(p, sol, ctl)?[sol.mode])
}
