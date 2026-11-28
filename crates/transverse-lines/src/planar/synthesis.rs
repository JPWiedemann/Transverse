//! Synthesis: find a dimension that gives a target Z0.

use serde::{Deserialize, Serialize};
use transverse_core::quasistatic::{self, Solution};
use transverse_core::synthesis::{self as root, SynthSpec};
use transverse_core::{Ctl, TvError, TvResult};

use super::{FemSolution, PlanarParams, analyze, analyze_fem};

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

/// Returns the geometry that gives `target` Ohm and its solution.
pub fn synthesize(
    p: &PlanarParams,
    target: f64,
    var: SynthVar,
    rtol: f64,
    ctl: &Ctl,
) -> TvResult<(PlanarParams, Solution)> {
    synthesize_with(
        p,
        target,
        var,
        rtol,
        ctl,
        |p| quasistatic::auto_margin(p, &p.fd_options(), ctl),
        |q| analyze(q, ctl),
        |s| s.z0,
    )
}

/// As [`synthesize`] with the finite-element solver. Its Z0 carries some mesh noise, so the solves ask for a
/// tighter mesh tolerance than the user's and the search stops at the larger of `rtol` and what that noise allows.
pub fn synthesize_fem(
    p: &PlanarParams,
    target: f64,
    var: SynthVar,
    rtol: f64,
    ctl: &Ctl,
) -> TvResult<(PlanarParams, FemSolution)> {
    let tight = PlanarParams { tol: p.tol / 4.0, ..p.clone() };
    let solve = |q: &PlanarParams| analyze_fem(q, &q.fem_options(), ctl);
    let (mut out, sol) = synthesize_with(
        &tight,
        target,
        var,
        rtol.max(tight.tol / 100.0 / 4.0),
        ctl,
        |q| Ok(solve(q)?.margin),
        solve,
        |s| s.z0,
    )?;
    out.tol = p.tol;
    Ok((out, sol))
}

#[allow(clippy::too_many_arguments)]
fn synthesize_with<S>(
    p: &PlanarParams,
    target: f64,
    var: SynthVar,
    rtol: f64,
    ctl: &Ctl,
    margin_of: impl FnOnce(&PlanarParams) -> TvResult<f64>,
    solve: impl Fn(&PlanarParams) -> TvResult<S>,
    z0_of: impl Fn(&S) -> f64,
) -> TvResult<(PlanarParams, S)> {
    p.validate()?;
    if var != SynthVar::W && !p.coplanar {
        return Err(TvError::Synthesis("the gap can only be varied with a coplanar ground".into()));
    }
    let mut p = p.clone();
    if p.margin <= 0.0 && !p.closed() {
        ctl.msg("Synthesis: determining domain size ...");
        p.margin = margin_of(&p)?;
    }
    let total = 2.0 * p.s + p.w;
    let mk = |x: f64| match var {
        SynthVar::W => PlanarParams { w: x, ..p.clone() },
        SynthVar::S => PlanarParams { s: x, ..p.clone() },
        SynthVar::WTotal => PlanarParams { w: x, s: (total - x) / 2.0, ..p.clone() },
    };

    let scale = p.w + 2.0 * p.gap_or_zero(); // all limits relative to the geometry
    let hmax = p.h1.max(p.h2);
    let (lo_lim, hi_lim) = match var {
        SynthVar::WTotal => (1e-3 * total, total * (1.0 - 2e-3)),
        SynthVar::W => (1e-3 * scale, 20.0 * hmax + 10.0 * p.w),
        SynthVar::S => (1e-3 * scale, 20.0 * hmax + 10.0 * p.s),
    };
    // the narrow face of an etched trace must stay wider than nothing
    let lo_lim = if var == SynthVar::S { lo_lim } else { lo_lim.max(2.0 * p.undercut() + 1e-3 * scale) };
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
            let sol = solve(&mk(x))?;
            let z0 = z0_of(&sol);
            ctl.msg(&format!("Synthesis step {step}: {name} = {:.2} um -> Z0 = {z0:.3} Ohm", x * 1e3));
            Ok((z0, sol))
        },
        |x| solve(&mk(x)),
    )?;
    Ok((mk(x), sol))
}
