//! Root finding for synthesis: bracket search + Brent's method with a relative tolerance.

use std::collections::HashMap;

use crate::error::{TvError, TvResult};

/// Brent's method (same algorithm and iteration sequence as scipy's `brentq`).
/// Convergence: |x - x0| <= xtol + rtol * |x0|.
pub fn brentq(
    mut f: impl FnMut(f64) -> TvResult<f64>,
    a: f64,
    b: f64,
    xtol: f64,
    rtol: f64,
) -> TvResult<f64> {
    let (mut xpre, mut xcur) = (a, b);
    let (mut xblk, mut fblk) = (0.0, 0.0);
    let (mut spre, mut scur) = (0.0, 0.0);
    let mut fpre = f(xpre)?;
    let mut fcur = f(xcur)?;
    if fpre == 0.0 {
        return Ok(xpre);
    }
    if fcur == 0.0 {
        return Ok(xcur);
    }
    if fpre.is_sign_negative() == fcur.is_sign_negative() {
        return Err(TvError::Synthesis("f(a) and f(b) must have different signs".into()));
    }
    for _ in 0..100 {
        if fpre != 0.0 && fcur != 0.0 && fpre.is_sign_negative() != fcur.is_sign_negative() {
            xblk = xpre;
            fblk = fpre;
            spre = xcur - xpre;
            scur = spre;
        }
        if fblk.abs() < fcur.abs() {
            xpre = xcur;
            xcur = xblk;
            xblk = xpre;
            fpre = fcur;
            fcur = fblk;
            fblk = fpre;
        }
        let delta = (xtol + rtol * xcur.abs()) / 2.0;
        let sbis = (xblk - xcur) / 2.0;
        if fcur == 0.0 || sbis.abs() < delta {
            return Ok(xcur);
        }
        if spre.abs() > delta && fcur.abs() < fpre.abs() {
            let stry = if xpre == xblk {
                -fcur * (xcur - xpre) / (fcur - fpre) // secant
            } else {
                let dpre = (fpre - fcur) / (xpre - xcur);
                let dblk = (fblk - fcur) / (xblk - xcur);
                -fcur * (fblk * dblk - fpre * dpre) / (dblk * dpre * (fblk - fpre)) // inverse quadratic
            };
            if 2.0 * stry.abs() < spre.abs().min(3.0 * sbis.abs() - delta) {
                spre = scur;
                scur = stry;
            } else {
                spre = sbis;
                scur = sbis;
            }
        } else {
            spre = sbis;
            scur = sbis;
        }
        xpre = xcur;
        fpre = fcur;
        xcur += if scur.abs() > delta {
            scur
        } else if sbis > 0.0 {
            delta
        } else {
            -delta
        };
        fcur = f(xcur)?;
    }
    Err(TvError::Synthesis("root finding did not converge".into()))
}

/// Search settings. All limits are relative to the geometry (set by the caller).
#[derive(Clone, Copy, Debug)]
pub struct SynthSpec {
    pub target: f64,
    /// Starting value of the varied dimension.
    pub x0: f64,
    pub lo_lim: f64,
    pub hi_lim: f64,
    /// True if the quantity grows with the varied dimension.
    pub increasing: bool,
    /// Relative stopping tolerance of the varied dimension.
    pub rtol: f64,
    /// Length scale for the absolute floor of the tolerance (xtol = 1e-12 * scale).
    pub scale: f64,
}

/// Find `x` with `eval(x).0 == target`. `eval` returns the quantity and an artifact (e.g. the
/// full solution) of that evaluation; the artifact of the returned `x` comes back with it.
/// Only the best artifact is kept in memory.
pub fn synthesize<T>(
    spec: &SynthSpec,
    mut eval: impl FnMut(f64) -> TvResult<(f64, T)>,
    refine_artifact: impl FnOnce(f64) -> TvResult<T>,
) -> TvResult<(f64, T)> {
    let mut cache: HashMap<u64, f64> = HashMap::new();
    let mut best: Option<(u64, f64, T)> = None; // (x bits, |z - target|, artifact)
    let target = spec.target;
    let mut f = |x: f64| -> TvResult<f64> {
        if let Some(&z) = cache.get(&x.to_bits()) {
            return Ok(z - target);
        }
        let (z, art) = eval(x)?;
        cache.insert(x.to_bits(), z);
        if best.as_ref().is_none_or(|b| (z - target).abs() < b.1) {
            best = Some((x.to_bits(), (z - target).abs(), art));
        }
        Ok(z - target)
    };

    let x0 = spec.x0.max(spec.lo_lim * 2.0).min(spec.hi_lim / 2.0);
    let f0 = f(x0)?;
    let finish = |best: Option<(u64, f64, T)>, x: f64| -> TvResult<(f64, T)> {
        match best {
            Some((bits, _, art)) if bits == x.to_bits() => Ok((x, art)),
            _ => Ok((x, refine_artifact(x)?)),
        }
    };
    if f0.abs() < 1e-6 * target {
        return finish(best, x0);
    }
    let go_up = (f0 < 0.0) == spec.increasing;
    let (mut a, mut fa) = (x0, f0);
    let mut bracket = None;
    for _ in 0..40 {
        let b = if go_up { a * 1.25 } else { a / 1.25 }.max(spec.lo_lim).min(spec.hi_lim);
        let fb = f(b)?;
        if fa * fb <= 0.0 {
            bracket = Some((a, b));
            break;
        }
        if b == spec.lo_lim || b == spec.hi_lim {
            return Err(TvError::Synthesis(format!(
                "target {target} not reachable (range {:.0}..{:.0} um, value there = {:.2})",
                spec.lo_lim * 1e3,
                spec.hi_lim * 1e3,
                fb + target
            )));
        }
        (a, fa) = (b, fb);
    }
    let (a, b) = bracket.ok_or_else(|| TvError::Synthesis("no bracket found".into()))?;
    let (lo, hi) = if a < b { (a, b) } else { (b, a) };
    let x = brentq(&mut f, lo, hi, 1e-12 * spec.scale, spec.rtol)?;
    finish(best, x)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brentq_finds_sqrt2() {
        let x = brentq(|x| Ok(x * x - 2.0), 0.0, 2.0, 1e-14, 1e-12).unwrap();
        assert!((x - 2f64.sqrt()).abs() < 1e-11);
    }

    #[test]
    fn synthesize_decreasing_function() {
        let spec = SynthSpec {
            target: 5.0,
            x0: 1.0,
            lo_lim: 0.01,
            hi_lim: 100.0,
            increasing: false,
            rtol: 1e-8,
            scale: 1.0,
        };
        let (x, art) = synthesize(&spec, |x| Ok((10.0 / x, x)), Ok).unwrap();
        assert!((x - 2.0).abs() < 1e-6 && (art - x).abs() < 1e-12);
    }

    #[test]
    fn unreachable_target_is_an_error() {
        let spec = SynthSpec {
            target: 1e6,
            x0: 1.0,
            lo_lim: 0.01,
            hi_lim: 100.0,
            increasing: false,
            rtol: 1e-6,
            scale: 1.0,
        };
        assert!(synthesize(&spec, |x| Ok((10.0 / x, ())), |_| Ok(())).is_err());
    }
}
