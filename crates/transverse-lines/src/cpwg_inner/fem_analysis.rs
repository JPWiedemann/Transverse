//! Full FEM analysis: Z0 and friends, losses and the ground split (the counterpart of [`super::full_analysis`]).

use transverse_core::losses::LossModel;
use transverse_core::{Ctl, TvResult};

use super::fem::{FemSolution, analyze_fem};
use super::fem_loss::loss_model_fem;
use super::fem_split::ground_split_fem;
use super::{CpwgParams, Split};

pub struct CpwgFemAnalysis {
    pub sol: FemSolution,
    pub loss: LossModel,
    pub split: Split,
}

/// FEM solution, Wheeler conductor loss with the dielectric loss model, and the ground split.
pub fn full_analysis_fem(p: &CpwgParams, ctl: &Ctl) -> TvResult<CpwgFemAnalysis> {
    let sol = analyze_fem(p, &p.fem_options(), ctl)?;
    let loss = loss_model_fem(p, &sol, ctl)?;
    let split = ground_split_fem(p, &sol)?;
    Ok(CpwgFemAnalysis { sol, loss, split })
}
