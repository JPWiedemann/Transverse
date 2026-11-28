//! Full FEM analysis: Z0 and friends, losses and the ground split (the counterpart of [`super::full_analysis`]).

use transverse_core::losses::LossModel;
use transverse_core::{Ctl, TvResult};

use super::fem::{FemSolution, analyze_fem};
use super::fem_loss::loss_models_fem;
use super::fem_split::ground_split_fem;
use super::{PlanarParams, Split};

/// Losses and ground split of one mode.
pub struct ModeAnalysis {
    pub loss: LossModel,
    pub split: Split,
}

pub struct PlanarFemAnalysis {
    /// In the first mode: the line itself, or the odd (differential) mode of a pair.
    pub sol: FemSolution,
    /// Of the first mode.
    pub loss: LossModel,
    pub split: Split,
    /// The even (common) mode of a pair.
    pub even: Option<ModeAnalysis>,
}

/// Losses and ground splits for an existing solution.
pub fn analysis_from_fem(p: &PlanarParams, sol: FemSolution, ctl: &Ctl) -> TvResult<PlanarFemAnalysis> {
    let mut losses = loss_models_fem(p, &sol, ctl)?.into_iter();
    let split = ground_split_fem(p, &sol)?;
    let loss = losses.next().expect("every solution has a mode");
    let even = match losses.next() {
        Some(loss) => Some(ModeAnalysis { loss, split: ground_split_fem(p, &sol.in_mode(1))? }),
        None => None,
    };
    Ok(PlanarFemAnalysis { sol, loss, split, even })
}

/// FEM solution, Wheeler conductor loss with the dielectric loss model, and the ground split.
pub fn full_analysis_fem(p: &PlanarParams, ctl: &Ctl) -> TvResult<PlanarFemAnalysis> {
    analysis_from_fem(p, analyze_fem(p, &p.fem_options(), ctl)?, ctl)
}
