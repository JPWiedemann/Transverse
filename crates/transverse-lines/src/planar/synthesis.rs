//! Synthesis: find a dimension that gives a target Z0.

use serde::{Deserialize, Serialize};
use transverse_core::quasistatic::{self, Solution};
use transverse_core::synthesis::{self as root, SynthSpec};
use transverse_core::synthesis2::{Eval, Spec2, solve2};
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
    /// Signal width at fixed total 2s + w (one trace).
    #[serde(rename = "w_total")]
    WTotal,
    /// Gap between the traces of a pair.
    #[serde(rename = "d")]
    D,
}

/// Default relative stopping tolerance of the varied dimension (0.01 %).
pub const DEFAULT_RTOL: f64 = 1e-4;

/// Returns the geometry that gives `target` Ohm (grid solver, one trace) and its solution.
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

/// As [`synthesize`] with the finite-element solver; of a pair the target is Zdiff. Its Z0 carries some mesh noise, so the solves ask for a
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
        FemSolution::target_impedance,
    )?;
    out.tol = p.tol;
    Ok((out, sol))
}

/// Strongest coupling [`synthesize_pair_fem`] searches for. Far below it a printed pair already needs traces of
/// a few micrometres, and the search for more would only mesh ever thinner ones.
const MAX_COUPLING: f64 = 0.8;
/// Weakest coupling it searches for, in units of its impedance tolerance: below, any wider gap meets the targets
/// as well, and the gap found would mean nothing. With 10 (a coupling of about 0.003 at the default accuracy) the
/// gap is repeatable to well under a percent.
const MIN_COUPLING_TOLERANCES: f64 = 10.0;

/// Width and pair gap of a differential pair for a differential and a common impedance at once (finite elements;
/// `s` and everything else stay). Zcomm is at least Zdiff / 4, which it reaches without any coupling; how far
/// above it lies is set by the coupling `(4 Zcomm - Zdiff) / (4 Zcomm + Zdiff)`.
pub fn synthesize_pair_fem(
    p: &PlanarParams,
    zdiff: f64,
    zcomm: f64,
    rtol: f64,
    ctl: &Ctl,
) -> TvResult<(PlanarParams, FemSolution)> {
    p.validate()?;
    if !p.pair() {
        return Err(TvError::Synthesis("Zdiff and Zcomm together are for a differential pair".into()));
    }
    if !(zdiff > 0.0 && zcomm > 0.0 && zdiff.is_finite() && zcomm.is_finite()) {
        return Err(TvError::Synthesis("the target impedances must be positive".into()));
    }
    let coupling = |zd: f64, zc: f64| (4.0 * zc - zd) / (4.0 * zc + zd);
    let wanted = coupling(zdiff, zcomm);
    if wanted <= 0.0 {
        return Err(TvError::Synthesis(format!(
            "Zcomm must be above Zdiff / 4 = {:.2} Ohm, the value of two traces without any coupling",
            zdiff / 4.0
        )));
    }
    let mut tight = PlanarParams { tol: p.tol / 4.0, ..p.clone() };
    // the relative error of both impedances at which the search is done
    let done = rtol.max(tight.tol / 100.0 / 4.0);
    let zcomm_of = |k: f64| zdiff / 4.0 * (1.0 + k) / (1.0 - k);
    let weakest = MIN_COUPLING_TOLERANCES * done;
    if wanted < weakest {
        return Err(TvError::Synthesis(format!(
            "these targets ask for a coupling of {wanted:.5}, too weak to fix the gap (any wide gap gives it). Ask for Zcomm of at least {:.2} Ohm, or choose the gap and synthesize w alone",
            zcomm_of(weakest)
        )));
    }
    if wanted > MAX_COUPLING {
        return Err(TvError::Synthesis(format!(
            "these targets ask for a coupling of {wanted:.2}, more than a pair of this kind can have. With this Zdiff, Zcomm up to {:.0} Ohm is searched",
            zcomm_of(MAX_COUPLING)
        )));
    }
    let solve = |q: &PlanarParams| analyze_fem(q, &q.fem_options(), ctl);
    if tight.margin <= 0.0 && !tight.closed() {
        ctl.msg("Synthesis: determining domain size ...");
        tight.margin = solve(&tight)?.margin;
    }
    // all limits relative to the geometry
    let scale = p.trace_span() + 2.0 * p.gap_or_zero();
    let hmax = p.h1.max(p.h2);
    let spec = Spec2 {
        x0: [p.w, p.d],
        lo_lim: [(2.0 * p.undercut() + 1e-3 * scale).max(1e-3 * scale), 1e-3 * scale],
        hi_lim: [20.0 * hmax + 10.0 * p.w, 20.0 * hmax + 10.0 * p.d],
        tol: done,
    };
    let mk = |[w, d]: [f64; 2]| PlanarParams { w, d, ..tight.clone() };
    let mut step = 0;
    let ([w, d], sol) = solve2(&spec, |x| {
        step += 1;
        let sol = solve(&mk(x))?;
        let (zd, zc) = (sol.target_impedance(), sol.common_impedance().expect("a pair"));
        ctl.msg(&format!(
            "Synthesis step {step}: w = {:.2} um, d = {:.2} um -> Zdiff = {zd:.3}, Zcomm = {zc:.3} Ohm",
            x[0] * 1e3,
            x[1] * 1e3
        ));
        Ok(Eval {
            // the coupling falls about exponentially with the gap, while Zcomm itself hardly moves once it is weak
            residual: [(zd / zdiff).ln(), (coupling(zd, zc).max(1e-9) / wanted).ln()],
            error: (zd / zdiff - 1.0).abs().max((zc / zcomm - 1.0).abs()),
            artifact: sol,
        })
    })?;
    let out = PlanarParams { tol: p.tol, ..mk([w, d]) };
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
    if matches!(var, SynthVar::S | SynthVar::WTotal) && !p.coplanar {
        return Err(TvError::Synthesis("the gap s can only be varied with a coplanar ground".into()));
    }
    if var == SynthVar::D && !p.pair() {
        return Err(TvError::Synthesis("the pair gap can only be varied for a differential pair".into()));
    }
    if var == SynthVar::WTotal && p.pair() {
        return Err(TvError::Synthesis("a fixed total width 2s + w is for a single trace".into()));
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
        SynthVar::D => PlanarParams { d: x, ..p.clone() },
    };

    let scale = p.trace_span() + 2.0 * p.gap_or_zero(); // all limits relative to the geometry
    let hmax = p.h1.max(p.h2);
    let (lo_lim, hi_lim) = match var {
        SynthVar::WTotal => (1e-3 * total, total * (1.0 - 2e-3)),
        SynthVar::W => (1e-3 * scale, 20.0 * hmax + 10.0 * p.w),
        SynthVar::S => (1e-3 * scale, 20.0 * hmax + 10.0 * p.s),
        SynthVar::D => (1e-3 * scale, 20.0 * hmax + 10.0 * p.d),
    };
    let gap = matches!(var, SynthVar::S | SynthVar::D);
    // the narrow face of an etched trace must stay wider than nothing
    let lo_lim = if gap { lo_lim } else { lo_lim.max(2.0 * p.undercut() + 1e-3 * scale) };
    let spec = SynthSpec {
        target,
        x0: match var {
            SynthVar::S => p.s,
            SynthVar::D => p.d,
            _ => p.w,
        },
        lo_lim,
        hi_lim,
        increasing: gap, // Z0 (and Zdiff) grow with s and d, fall with w
        rtol,
        scale,
    };
    let name = match var {
        SynthVar::W | SynthVar::WTotal => "w",
        SynthVar::S => "s",
        SynthVar::D => "d",
    };
    let what = if p.pair() { "Zdiff" } else { "Z0" };
    let mut step = 0;
    let (x, sol) = root::synthesize(
        &spec,
        |x| {
            step += 1;
            let sol = solve(&mk(x))?;
            let z0 = z0_of(&sol);
            ctl.msg(&format!("Synthesis step {step}: {name} = {:.2} um -> {what} = {z0:.3} Ohm", x * 1e3));
            Ok((z0, sol))
        },
        |x| solve(&mk(x)),
    )?;
    Ok((mk(x), sol))
}
