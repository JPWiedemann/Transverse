//! Synthesis: find a dimension that gives a target Z0, or the length that gives a target phase.

use serde::{Deserialize, Serialize};
use transverse_core::quasistatic::{self, Solution};
use transverse_core::synthesis::{self as root, SynthSpec};
use transverse_core::{Ctl, TvError, TvResult, guided_wavelength_mm};

use super::{CpwgAnalysis, CpwgParams, analyze, full_analysis};

/// Which dimension is varied.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SynthVar {
    /// Signal width, gap fixed.
    #[serde(rename = "w")]
    W,
    /// Gap, signal width fixed.
    #[serde(rename = "s")]
    S,
    /// Signal width at fixed total 2s + w.
    #[serde(rename = "w_total")]
    WTotal,
}

/// Default relative stopping tolerance of the varied dimension (0.01 %).
pub const DEFAULT_RTOL: f64 = 1e-4;

/// The line length that has an electrical length of `phase_deg` degrees at the analysis frequency,
/// with the analysis of the (unchanged) cross-section. The solution does not depend on the length,
/// so this is one analysis and no iteration: `length = phase / 360 * lambda_guided`.
pub fn synthesize_length(p: &CpwgParams, phase_deg: f64, ctl: &Ctl) -> TvResult<(CpwgParams, CpwgAnalysis)> {
    if !(phase_deg.is_finite() && phase_deg > 0.0) {
        return Err(TvError::Synthesis("the target phase must be a positive number of degrees".into()));
    }
    let a = full_analysis(p, ctl)?;
    let length = phase_deg / 360.0 * guided_wavelength_mm(p.f, a.sol.eeff);
    Ok((CpwgParams { length, ..p.clone() }, a))
}

/// Returns the geometry that gives `target` Ohm and its solution.
pub fn synthesize(
    p: &CpwgParams,
    target: f64,
    var: SynthVar,
    rtol: f64,
    ctl: &Ctl,
) -> TvResult<(CpwgParams, Solution)> {
    p.validate()?;
    let mut p = p.clone();
    if p.margin <= 0.0 && !p.closed() {
        ctl.msg("Synthesis: determining domain size ...");
        p.margin = quasistatic::auto_margin(&p, &p.fd_options(), ctl)?;
    }
    let total = 2.0 * p.s + p.w;
    let mk = |x: f64| match var {
        SynthVar::W => CpwgParams { w: x, ..p.clone() },
        SynthVar::S => CpwgParams { s: x, ..p.clone() },
        SynthVar::WTotal => CpwgParams { w: x, s: (total - x) / 2.0, ..p.clone() },
    };

    let scale = p.w + 2.0 * p.s; // all limits relative to the geometry
    let hmax = p.h1.max(p.h2);
    let (lo_lim, hi_lim) = match var {
        SynthVar::WTotal => (1e-3 * total, total * (1.0 - 2e-3)),
        SynthVar::W => (1e-3 * scale, 20.0 * hmax + 10.0 * p.w),
        SynthVar::S => (1e-3 * scale, 20.0 * hmax + 10.0 * p.s),
    };
    let spec = SynthSpec {
        target,
        x0: if var == SynthVar::S { p.s } else { p.w },
        lo_lim,
        hi_lim,
        increasing: var == SynthVar::S, // Z0 grows with s, falls with w
        rtol,
        scale,
    };
    let name = match var {
        SynthVar::W | SynthVar::WTotal => "w",
        SynthVar::S => "s",
    };
    let mut step = 0;
    let (x, sol) = root::synthesize(
        &spec,
        |x| {
            step += 1;
            let sol = analyze(&mk(x), ctl)?;
            ctl.msg(&format!("Synthesis step {step}: {name} = {:.2} um -> Z0 = {:.3} Ohm", x * 1e3, sol.z0));
            Ok((sol.z0, sol))
        },
        |x| analyze(&mk(x), ctl),
    )?;
    Ok((mk(x), sol))
}
