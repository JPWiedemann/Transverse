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

mod grid;
mod ground_split;
mod params;
mod synthesis;
mod wheeler;

pub use grid::*;
pub use ground_split::{GroundCat, Profile, Shares, Split, ground_split};
pub use params::{CpwgParams, GapMode, MeshMode, analytic};
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

/// Analysis with losses and ground split.
pub struct Analysis {
    pub sol: Solution,
    pub loss: LossModel,
    pub split: Split,
}

/// Field solution + Wheeler conductor loss + dielectric loss model + ground split.
pub fn full_analysis(p: &CpwgParams, ctl: &Ctl) -> TvResult<Analysis> {
    let sol = analyze(p, ctl)?;
    let dzdn = wheeler_dzdn(p, &sol, ctl)?;
    let loss = LossModel { z0: sol.z0, g_over_omega: sol.g_over_omega, dzdn, sigma: p.sigma, rq_mm: p.rq };
    let split = ground_split(p, &sol);
    Ok(Analysis { sol, loss, split })
}
