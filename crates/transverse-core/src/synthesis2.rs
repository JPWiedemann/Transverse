//! Root finding in two unknowns for synthesis with two targets: a damped Broyden (quasi-Newton) iteration.
//!
//! Every evaluation is a field solve, so the derivatives are taken by finite differences only at the start (and
//! again if the iteration stalls) and are otherwise updated from the steps taken. The unknowns are positive
//! dimensions and are handled as their logarithms, so steps and limits are relative.

use crate::error::{TvError, TvResult};

/// Search settings. All limits are relative to the geometry (set by the caller).
#[derive(Clone, Copy, Debug)]
pub struct Spec2 {
    /// Starting values.
    pub x0: [f64; 2],
    pub lo_lim: [f64; 2],
    pub hi_lim: [f64; 2],
    /// Done when [`Eval::error`] is at most this.
    pub tol: f64,
}

/// What one evaluation reports.
pub struct Eval<T> {
    /// Zero at the solution; what the iteration works on. Best close to linear in the logarithms of the unknowns.
    pub residual: [f64; 2],
    /// How far the targets are missed (e.g. the largest relative error): decides when to stop.
    pub error: f64,
    /// E.g. the full solution; of the result it comes back.
    pub artifact: T,
}

/// Largest number of evaluations.
const MAX_EVALS: usize = 40;
/// Steps of the finite differences, in the logarithms.
const FD_STEP: [f64; 2] = [0.08, 0.15];
/// Largest step of one unknown, as a factor.
const MAX_FACTOR: f64 = 1.6;
/// Steps shorter than this (in the logarithms) say more about the noise of the solves than about the slopes.
const MIN_UPDATE: f64 = 5e-3;

type Jacobian = [[f64; 2]; 2];

fn norm(r: [f64; 2]) -> f64 {
    r[0].hypot(r[1])
}

/// `x` with `J x = -r`.
fn newton_step(j: &Jacobian, r: [f64; 2]) -> Option<[f64; 2]> {
    let det = j[0][0] * j[1][1] - j[0][1] * j[1][0];
    let step = [(j[0][1] * r[1] - j[1][1] * r[0]) / det, (j[1][0] * r[0] - j[0][0] * r[1]) / det];
    step.iter().all(|v| v.is_finite()).then_some(step)
}

/// Find `x` where the error of `eval(x)` is within the tolerance.
pub fn solve2<T>(
    spec: &Spec2,
    mut eval: impl FnMut([f64; 2]) -> TvResult<Eval<T>>,
) -> TvResult<([f64; 2], T)> {
    let lo = spec.lo_lim.map(f64::ln);
    let hi = spec.hi_lim.map(f64::ln);
    let mut u: [f64; 2] =
        std::array::from_fn(|i| spec.x0[i].max(2.0 * spec.lo_lim[i]).min(spec.hi_lim[i] / 2.0).ln());
    let mut count = 0;
    let mut at = |u: [f64; 2]| -> TvResult<Eval<T>> {
        count += 1;
        if count > MAX_EVALS {
            return Err(TvError::Synthesis(format!("the targets were not reached in {MAX_EVALS} solves")));
        }
        let e = eval(u.map(f64::exp))?;
        if e.residual.iter().all(|r| r.is_finite()) {
            Ok(e)
        } else {
            Err(TvError::Synthesis("the targets cannot be evaluated here".into()))
        }
    };

    let mut here = at(u)?;
    let mut jacobian: Option<Jacobian> = None;
    let mut fresh = false;
    let mut limit = MAX_FACTOR.ln();
    // steps in a row that ran into a limit of the dimensions
    let mut pinned = 0;
    loop {
        if here.error <= spec.tol {
            return Ok((u.map(f64::exp), here.artifact));
        }
        let mut j = match jacobian {
            Some(j) => j,
            None => {
                let mut j = [[0.0; 2]; 2];
                for k in 0..2 {
                    let h = if u[k] + FD_STEP[k] <= hi[k] { FD_STEP[k] } else { -FD_STEP[k] };
                    let mut v = u;
                    v[k] += h;
                    let there = at(v)?;
                    if there.error <= spec.tol {
                        return Ok((v.map(f64::exp), there.artifact));
                    }
                    for i in 0..2 {
                        j[i][k] = (there.residual[i] - here.residual[i]) / h;
                    }
                }
                fresh = true;
                j
            }
        };
        let step = newton_step(&j, here.residual).ok_or_else(|| {
            TvError::Synthesis("the two dimensions do not move the two targets independently here".into())
        })?;
        let shrink = (limit / step[0].abs().max(step[1].abs())).min(1.0);
        let next: [f64; 2] = std::array::from_fn(|k| (u[k] + shrink * step[k]).clamp(lo[k], hi[k]));
        let clipped = (0..2).any(|k| next[k] != u[k] + shrink * step[k]);
        pinned = if clipped { pinned + 1 } else { 0 };
        if pinned >= 3 {
            let x = u.map(f64::exp);
            return Err(TvError::Synthesis(format!(
                "the targets lead beyond the limits of the dimensions (reached {:.0} um and {:.0} um)",
                x[0] * 1e3,
                x[1] * 1e3
            )));
        }
        let there = at(next)?;
        if there.error <= spec.tol {
            return Ok((next.map(f64::exp), there.artifact));
        }
        // Broyden: the smallest change of the derivatives that explains this step
        let s = [next[0] - u[0], next[1] - u[1]];
        let length2 = s[0] * s[0] + s[1] * s[1];
        if length2.sqrt() > MIN_UPDATE {
            for i in 0..2 {
                let miss = there.residual[i] - here.residual[i] - j[i][0] * s[0] - j[i][1] * s[1];
                for k in 0..2 {
                    j[i][k] += miss * s[k] / length2;
                }
            }
        }
        if norm(there.residual) < norm(here.residual) {
            (u, here) = (next, there);
            jacobian = Some(j);
            fresh = false;
            limit = (2.0 * limit).min(MAX_FACTOR.ln());
        } else {
            // no gain: go less far, and with slopes taken here if these were only updated ones
            limit = 0.5 * length2.sqrt();
            jacobian = if fresh { Some(j) } else { None };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ETA0;
    use crate::special::k_ratio;

    fn spec(x0: [f64; 2]) -> Spec2 {
        Spec2 { x0, lo_lim: [1e-3, 1e-3], hi_lim: [20.0, 20.0], tol: 1e-8 }
    }

    /// Cohn's coupled striplines (exact, t = 0): find width and gap for the even and odd impedance of a known pair.
    #[test]
    fn width_and_gap_of_coupled_striplines_are_found_again() {
        let b = 0.5;
        let cohn = |w: f64, d: f64| {
            let own = (std::f64::consts::PI * w / (2.0 * b)).tanh();
            let other = (std::f64::consts::PI * (w + d) / (2.0 * b)).tanh();
            let z = |k: f64| ETA0 / (4.0 * 3.5f64.sqrt()) / k_ratio(k);
            (2.0 * z(own / other), z(own * other) / 2.0)
        };
        let coupling = |zd: f64, zc: f64| (4.0 * zc - zd) / (4.0 * zc + zd);
        for (w, d) in [(0.2, 0.05), (0.15, 0.2), (0.3, 0.4)] {
            let (zd, zc) = cohn(w, d);
            for start in [[0.1, 0.1], [0.4, 0.5]] {
                let mut evals = 0;
                let (x, ()) = solve2(&spec(start), |[w, d]| {
                    evals += 1;
                    let (a, c) = cohn(w, d);
                    Ok(Eval {
                        residual: [(a / zd).ln(), (coupling(a, c) / coupling(zd, zc)).ln()],
                        error: (a / zd - 1.0).abs().max((c / zc - 1.0).abs()),
                        artifact: (),
                    })
                })
                .unwrap();
                assert!((x[0] / w - 1.0).abs() < 1e-5 && (x[1] / d - 1.0).abs() < 1e-3, "{x:?} for {w} {d}");
                assert!(evals <= 25, "{evals} evaluations for {w} {d} from {start:?}");
            }
        }
    }

    #[test]
    fn targets_beyond_the_limits_are_errors() {
        let line = |target: [f64; 2]| {
            solve2(&spec([1.0, 1.0]), move |x| {
                let r = [(x[0] / target[0]).ln(), (x[1] / target[1]).ln()];
                Ok(Eval { residual: r, error: norm(r), artifact: () })
            })
        };
        assert!(line([3.0, 0.2]).is_ok());
        let err = line([3.0, 500.0]).err().unwrap().to_string();
        assert!(err.contains("limits"), "{err}");
        // two unknowns that act the same way cannot meet two targets
        let same = solve2(&spec([1.0, 1.0]), |x| {
            let r = [(x[0] * x[1]).ln(), (x[0] * x[1]).ln() - 1.0];
            Ok(Eval { residual: r, error: norm(r), artifact: () })
        });
        assert!(same.is_err());
    }
}
