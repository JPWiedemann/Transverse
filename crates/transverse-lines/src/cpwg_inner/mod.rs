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
mod params;

pub use grid::*;
pub use params::{CpwgParams, GapMode, MeshMode, analytic};

use transverse_core::quasistatic::{self, Solution};
use transverse_core::{Ctl, TvResult};

/// Full analysis of one geometry (Z0, eps_eff, L', C', G'/omega, fields).
pub fn analyze(p: &CpwgParams, ctl: &Ctl) -> TvResult<Solution> {
    p.validate()?;
    quasistatic::solve(p, &p.fd_options(), ctl)
}
