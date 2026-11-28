//! Lines with several signal conductors: capacitance matrices, the characteristic impedance matrix and the
//! quasi-TEM modes.
//!
//! With the capacitance matrices per metre `C` (with the dielectrics) and `C0` (in air), `L = C0^-1 / c^2`.
//! The modes are the voltage vectors `x` with `C x = eps_eff C0 x` (the eigenvectors of `L C`, from a symmetric
//! problem, so `eps_eff` is real and positive). With `X' C0 X = 1` the impedance matrix `Zc = (L C)^(-1/2) L`,
//! which relates the voltages and currents of a wave in one direction (`v = Zc i`), is
//! `X diag(eps_eff^(-1/2)) X' / c`: symmetric and positive definite.
//!
//! For one signal everything is the familiar `Z0 = 1 / (c sqrt(C C0))` and `eps_eff = C / C0`.

use crate::C_LIGHT;
use crate::error::{TvError, TvResult};
use crate::fem::Lumped;
use crate::losses::LossModel;
use crate::matrix::{SymMatrix, dot, solve_lower, solve_upper};

/// Maxwell capacitance matrices per metre of one solution: entry `(i, j)` belongs to the signals `i + 1` and
/// `j + 1`. Off-diagonal entries of `c` and `c0` are negative.
#[derive(Clone, Debug, PartialEq)]
pub struct CapMatrices {
    /// With the dielectrics [F/m].
    pub c: SymMatrix,
    /// In air [F/m].
    pub c0: SymMatrix,
    /// G / omega [F/m].
    pub g_over_omega: SymMatrix,
}

impl CapMatrices {
    pub fn signals(&self) -> usize {
        self.c.n()
    }

    /// The values of a line with one signal.
    pub fn single(&self) -> TvResult<Lumped> {
        if self.signals() != 1 {
            return Err(TvError::Solver(format!("expected one signal conductor, found {}", self.signals())));
        }
        Ok(Lumped { c: self.c.get(0, 0), c0: self.c0.get(0, 0), g_over_omega: self.g_over_omega.get(0, 0) })
    }

    /// Inductance matrix per metre [H/m].
    pub fn inductance(&self) -> TvResult<SymMatrix> {
        let inv = self.c0.inverse_spd()?;
        Ok(SymMatrix::from_fn(self.signals(), |i, j| inv.get(i, j) / (C_LIGHT * C_LIGHT)))
    }

    /// The matrices of a structure that is unchanged when the signals are permuted (signal `i` takes the place of
    /// `perm[i]`), made exactly so: each is averaged with its permutation, which removes the asymmetry of the
    /// mesh. `perm` must be its own inverse.
    pub fn symmetrised(&self, perm: &[usize]) -> TvResult<Self> {
        let n = self.signals();
        if perm.len() != n || !(0..n).all(|i| perm[i] < n && perm[perm[i]] == i) {
            return Err(TvError::InvalidParam("modes: not a permutation that swaps signals".into()));
        }
        let mean =
            |m: &SymMatrix| SymMatrix::from_fn(n, |i, j| 0.5 * (m.get(i, j) + m.get(perm[i], perm[j])));
        Ok(Self { c: mean(&self.c), c0: mean(&self.c0), g_over_omega: mean(&self.g_over_omega) })
    }
}

/// One quasi-TEM mode: the voltages and currents of a wave that keeps its shape along the line.
#[derive(Clone, Debug, PartialEq)]
pub struct Mode {
    pub eps_eff: f64,
    /// Voltages of the signals, scaled so that the largest magnitude is 1. The first entry within 1 % of that is
    /// positive, so that the nearly equal magnitudes of nearly equal lines do not decide the sign by chance.
    pub voltages: Vec<f64>,
    /// The currents of a forward wave with these voltages [A for V]: `Zc^-1 v`.
    pub currents: Vec<f64>,
}

impl Mode {
    /// Impedance of the mode, `v'v / v'i` [Ohm]: where all driven signals have the same ratio of voltage to
    /// current (one signal; odd and even mode of equal lines) it is that ratio.
    pub fn impedance(&self) -> f64 {
        dot(&self.voltages, &self.voltages) / dot(&self.voltages, &self.currents)
    }

    /// The mode for the voltage vector `x`, which must be a solution of `C x = eps_eff C0 x`.
    fn new(caps: &CapMatrices, x: &[f64]) -> Self {
        let peak = x.iter().fold(0.0f64, |s, v| s.max(v.abs()));
        let sign = x.iter().find(|v| v.abs() > 0.99 * peak).map_or(1.0, |v| v.signum());
        let voltages: Vec<f64> = x.iter().map(|v| sign * v / peak).collect();
        let eps_eff = caps.c.form(&voltages, &voltages) / caps.c0.form(&voltages, &voltages);
        // i = Zc^-1 v = c sqrt(eps_eff) C0 v for a mode
        let currents = caps.c0.mul_vec(&voltages).iter().map(|q| C_LIGHT * eps_eff.sqrt() * q).collect();
        Self { eps_eff, voltages, currents }
    }
}

/// Modes closer than this in `eps_eff` (relative) count as degenerate: any mix of them is a mode.
const DEGENERATE: f64 = 1e-7;

/// The solutions of `C x = lambda C0 x` with `x' C0 x = 1`.
fn generalised_eigen(caps: &CapMatrices) -> TvResult<(Vec<f64>, Vec<Vec<f64>>)> {
    let n = caps.signals();
    let l = caps.c0.cholesky()?;
    // a = l^-1 C l^-T, one column after the other
    let half: Vec<Vec<f64>> =
        (0..n).map(|j| solve_lower(&l, &(0..n).map(|i| caps.c.get(i, j)).collect::<Vec<_>>())).collect();
    let full: Vec<Vec<f64>> =
        (0..n).map(|i| solve_lower(&l, &(0..n).map(|j| half[j][i]).collect::<Vec<_>>())).collect();
    let a = SymMatrix::from_fn(n, |i, j| 0.5 * (full[i][j] + full[j][i]));
    let (values, vectors) = a.eigen();
    if !values.iter().all(|v| *v > 0.0) {
        return Err(TvError::Solver("modes: the capacitance matrix is not positive definite".into()));
    }
    Ok((values, vectors.iter().map(|y| solve_upper(&l, y)).collect()))
}

/// The characteristic impedance matrix [Ohm].
pub fn impedance_matrix(caps: &CapMatrices) -> TvResult<SymMatrix> {
    let n = caps.signals();
    if n == 1 {
        let z0 = caps.single()?.z0();
        return if z0.is_finite() && z0 > 0.0 {
            Ok(SymMatrix::from_fn(1, |_, _| z0))
        } else {
            Err(TvError::Solver("modes: the capacitances are not positive".into()))
        };
    }
    let (values, x) = generalised_eigen(caps)?;
    Ok(SymMatrix::from_fn(n, |i, j| {
        (0..n).map(|k| x[k][i] * x[k][j] / values[k].sqrt()).sum::<f64>() / C_LIGHT
    }))
}

/// The modes, by rising `eps_eff`. Where modes are degenerate (a homogeneous dielectric: all of them) those are
/// chosen whose voltage vectors are orthogonal, which for two equal lines are the even and the odd mode. If the
/// lines are equal use [`symmetric_pair_modes`]: nearly degenerate modes of a mesh that is not quite symmetric
/// are mixed.
pub fn modes(caps: &CapMatrices) -> TvResult<Vec<Mode>> {
    let (values, x) = generalised_eigen(caps)?;
    let mut order: Vec<usize> = (0..values.len()).collect();
    order.sort_by(|&a, &b| values[a].total_cmp(&values[b]));
    let mut out = Vec::with_capacity(values.len());
    let mut start = 0;
    while start < order.len() {
        let first = values[order[start]];
        let end =
            start + order[start..].iter().take_while(|&&k| values[k] - first <= DEGENERATE * first).count();
        let group: Vec<&Vec<f64>> = order[start..end].iter().map(|&k| &x[k]).collect();
        // rotate within the group until the vectors are orthogonal; they stay C0-orthonormal
        let gram = SymMatrix::from_fn(group.len(), |i, j| dot(group[i], group[j]));
        for q in gram.eigen().1 {
            let mixed: Vec<f64> =
                (0..values.len()).map(|i| group.iter().zip(&q).map(|(v, w)| w * v[i]).sum()).collect();
            out.push(Mode::new(caps, &mixed));
        }
        start = end;
    }
    Ok(out)
}

/// The even `[1, 1]` and the odd `[1, -1]` mode (in this order) of two equal lines. The matrices are symmetrised
/// first, so the voltage vectors are exact however weak the coupling is.
pub fn symmetric_pair_modes(caps: &CapMatrices) -> TvResult<[Mode; 2]> {
    if caps.signals() != 2 {
        return Err(TvError::InvalidParam("modes: a pair has two signals".into()));
    }
    let s = caps.symmetrised(&[1, 0])?;
    Ok([Mode::new(&s, &[1.0, 1.0]), Mode::new(&s, &[1.0, -1.0])])
}

/// Impedances of a pair, from its impedance matrix.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PairImpedances {
    /// Between the two signals, driven against each other: `d' Zc d` with `d = [1, -1]`.
    pub differential: f64,
    /// Both signals in parallel against ground: `[1, 1] Zc [1, 1]' / 4`.
    pub common: f64,
    /// `(Z11 - Z22) / 2`: conversion between the two; zero for equal lines.
    pub conversion: f64,
    /// Coupling coefficient `Z12 / sqrt(Z11 Z22)`; for equal lines `(Ze - Zo) / (Ze + Zo)`.
    pub coupling: f64,
}

impl PairImpedances {
    pub fn new(zc: &SymMatrix) -> TvResult<Self> {
        if zc.n() != 2 {
            return Err(TvError::InvalidParam("modes: a pair has two signals".into()));
        }
        let (z11, z12, z22) = (zc.get(0, 0), zc.get(0, 1), zc.get(1, 1));
        Ok(Self {
            differential: z11 + z22 - 2.0 * z12,
            common: (z11 + z22 + 2.0 * z12) / 4.0,
            conversion: (z11 - z22) / 2.0,
            coupling: z12 / (z11 * z22).sqrt(),
        })
    }
}

/// Metal data of the conductor loss of several signals.
#[derive(Clone, Debug)]
pub struct WallDerivative {
    /// `dL/dn` [H/m per m]: change of the inductance matrix when all metal surfaces recede into the metal.
    pub dl_dn: SymMatrix,
    pub sigma: f64,
    /// RMS roughness [mm].
    pub rq_mm: f64,
}

/// The loss model of one mode. With the currents `i` and voltages `v` of the mode, the attenuations are
/// `alpha_c = i' R i / (2 i' Zc i)` with `R = Rs / mu0 * dL/dn`, and `alpha_d = v' G v / (2 v' Zc^-1 v)`. They are
/// written as the model of an equivalent single line with the impedance of the mode.
pub fn mode_loss_model(caps: &CapMatrices, mode: &Mode, walls: &WallDerivative) -> LossModel {
    let z0 = mode.impedance();
    let (v, i) = (&mode.voltages, &mode.currents);
    // v' Zc^-1 v = v' i, and i' Zc i = i' v: twice the power of the wave
    let power = dot(v, i);
    LossModel {
        z0,
        eeff: mode.eps_eff,
        g_over_omega: caps.g_over_omega.form(v, v) / (z0 * power),
        dzdn: z0 * C_LIGHT * walls.dl_dn.form(i, i) / power,
        sigma: walls.sigma,
        rq_mm: walls.rq_mm,
    }
}

#[cfg(test)]
#[path = "modes_tests.rs"]
mod tests;
