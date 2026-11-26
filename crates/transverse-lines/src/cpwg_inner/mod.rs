//! CPWG / stripline-type coplanar waveguide in a PCB inner layer.
//!
//! Cross-section (not to scale), y = 0 at the bottom of the signal layer:
//!
//! ```text
//!     =============================================  ground plane 1 (optional), y = t + h1
//!           upper dielectric   h1, er1, tand1        | via fence 1 (optional)
//!     #######  s  [====== w ======]  s  ########      t   (signal + coplanar ground)
//!           lower dielectric   h2, er2, tand2        | via fence 2 (optional)
//!     =============================================  ground plane 2 (optional), y = -h2
//! ```
//!
//! All lengths in mm, frequency in Hz.

mod fem;
mod fem_loss;
mod field;
mod grid;
mod ground_split;
mod model;
mod params;
mod report;
mod schematic;
mod section;
mod synthesis;
mod wheeler;

pub use fem::{FemOptions, FemSolution, analyze_fem};
pub use fem_loss::{loss_model_fem, wheeler_dzdn_fem};
pub use field::field_data;
pub use grid::*;
pub use ground_split::{GroundCat, Profile, Shares, Split, ground_split};
pub use model::CpwgInner;
pub use params::{CpwgParams, GapMode, MeshMode, analytic};
pub use report::report;
pub use schematic::{SchematicFlags, schematic};
pub use section::{GROWTH, MAT_AIR, MAT_GAP, MAT_LOWER, MAT_UPPER};
pub use synthesis::{DEFAULT_RTOL, SynthVar, synthesize};
pub use wheeler::wheeler_dzdn;

use transverse_core::losses::LossModel;
use transverse_core::quasistatic::{self, Solution};
use transverse_core::{Ctl, TvResult};

/// Full analysis of one geometry (Z0, eps_eff, L', C', G'/omega, fields).
pub fn analyze(p: &CpwgParams, ctl: &Ctl) -> TvResult<Solution> {
    p.validate()?;
    quasistatic::solve(p, &p.fd_options(), ctl)
}

/// Analysis with losses and ground split (CPWG-specific; [`CpwgInner`] converts it to the generic
/// [`crate::model::Analysis`]).
pub struct CpwgAnalysis {
    pub sol: Solution,
    pub loss: LossModel,
    pub split: Split,
}

/// Wheeler conductor loss and ground split for an existing solution.
pub fn analysis_from_solution(p: &CpwgParams, sol: Solution, ctl: &Ctl) -> TvResult<CpwgAnalysis> {
    let dzdn = wheeler_dzdn(p, &sol, ctl)?;
    let loss = LossModel {
        z0: sol.z0,
        eeff: sol.eeff,
        g_over_omega: sol.g_over_omega,
        dzdn,
        sigma: p.sigma,
        rq_mm: p.rq,
    };
    let split = ground_split(p, &sol);
    Ok(CpwgAnalysis { sol, loss, split })
}

/// Field solution + Wheeler conductor loss + dielectric loss model + ground split.
pub fn full_analysis(p: &CpwgParams, ctl: &Ctl) -> TvResult<CpwgAnalysis> {
    analysis_from_solution(p, analyze(p, ctl)?, ctl)
}
