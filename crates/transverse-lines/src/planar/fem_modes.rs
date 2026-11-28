//! The modes of a planar line, each as an equivalent line per trace.
//!
//! One trace has one mode. Two equal traces have the odd mode (driven against each other: the differential
//! signal) and the even mode (driven together: the common signal). Their voltage vectors are known from the
//! symmetry, so the capacitances per trace of a mode are `x' C x / x' x`: `C11 - C12` for the odd and
//! `C11 + C12` for the even mode. `Zdiff = 2 Zodd`, `Zcomm = Zeven / 2`.

use serde::{Deserialize, Serialize};
use transverse_core::fem::Lumped;
use transverse_core::modes::{CapMatrices, symmetric_pair_modes};
use transverse_core::{TvError, TvResult};

use super::FemSolution;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModeKind {
    /// The mode of a line with one trace.
    Single,
    Odd,
    Even,
}

impl ModeKind {
    pub fn key(self) -> &'static str {
        match self {
            Self::Single => "single",
            Self::Odd => "odd",
            Self::Even => "even",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Single => "Single-ended",
            Self::Odd => "Odd mode (differential)",
            Self::Even => "Even mode (common)",
        }
    }
}

/// One mode as an equivalent line: the values of one trace while all are driven with `voltages`.
#[derive(Clone, Debug)]
pub struct ModeValues {
    pub kind: ModeKind,
    /// Voltage of every signal [V].
    pub voltages: Vec<f64>,
    /// Capacitance with dielectrics [F/m].
    pub c: f64,
    /// Capacitance in air [F/m].
    pub c0: f64,
    pub g_over_omega: f64,
    pub z0: f64,
    pub eeff: f64,
    /// Inductance per metre [H/m].
    pub l: f64,
}

impl ModeValues {
    fn new(kind: ModeKind, voltages: Vec<f64>, v: Lumped) -> Self {
        Self {
            kind,
            voltages,
            c: v.c,
            c0: v.c0,
            g_over_omega: v.g_over_omega,
            z0: v.z0(),
            eeff: v.eeff(),
            l: v.l(),
        }
    }
}

/// The modes of the capacitance matrices, the one a line is designed for first (single, or odd then even), and the
/// matrices themselves, made symmetric for a pair (its two traces are equal; the mesh is not quite).
pub(super) fn mode_values(caps: CapMatrices) -> TvResult<(CapMatrices, Vec<ModeValues>)> {
    match caps.signals() {
        1 => {
            let single = ModeValues::new(ModeKind::Single, vec![1.0], caps.single()?);
            Ok((caps, vec![single]))
        }
        2 => {
            let [even, odd] = symmetric_pair_modes(&caps)?;
            let caps = caps.symmetrised(&[1, 0])?;
            let modes = [(ModeKind::Odd, odd), (ModeKind::Even, even)]
                .into_iter()
                .map(|(kind, m)| {
                    let x = m.voltages;
                    let norm: f64 = x.iter().map(|v| v * v).sum();
                    let per_trace = Lumped {
                        c: caps.c.form(&x, &x) / norm,
                        c0: caps.c0.form(&x, &x) / norm,
                        g_over_omega: caps.g_over_omega.form(&x, &x) / norm,
                    };
                    ModeValues::new(kind, x, per_trace)
                })
                .collect();
            Ok((caps, modes))
        }
        n => Err(TvError::Solver(format!("the planar line has one or two signals, found {n}"))),
    }
}

/// The potential with the signals at `voltages`, from the potentials with one signal at 1 V each.
pub(super) fn superpose(per_signal: &[Vec<f64>], voltages: &[f64]) -> Vec<f64> {
    let mut out = vec![0.0; per_signal[0].len()];
    for (v, &a) in per_signal.iter().zip(voltages) {
        for (o, x) in out.iter_mut().zip(v) {
            *o += a * x;
        }
    }
    out
}

impl FemSolution {
    /// Switches the values and potentials of this solution to mode `k` of [`FemSolution::modes`].
    pub(super) fn select_mode(&mut self, k: usize) {
        let m = &self.modes[k];
        (self.c, self.c0, self.g_over_omega) = (m.c, m.c0, m.g_over_omega);
        (self.z0, self.eeff, self.l) = (m.z0, m.eeff, m.l);
        self.v = superpose(&self.signal_v, &m.voltages);
        self.v0 = superpose(&self.signal_v0, &m.voltages);
        self.mode = k;
    }

    /// The solution with the values and potentials of mode `k`.
    pub fn in_mode(&self, k: usize) -> Self {
        let mut out = self.clone();
        out.select_mode(k);
        out
    }

    /// The potential (with dielectrics) with the signals at `voltages`, at all degrees of freedom.
    pub fn potential(&self, voltages: &[f64]) -> Vec<f64> {
        superpose(&self.signal_v, voltages)
    }

    /// The impedance a line is designed for [Ohm]: Z0 of a single trace, Zdiff of a pair.
    pub fn target_impedance(&self) -> f64 {
        let first = &self.modes[0];
        if first.kind == ModeKind::Odd { 2.0 * first.z0 } else { first.z0 }
    }

    /// Zcomm of a pair [Ohm]: both traces in parallel against ground.
    pub fn common_impedance(&self) -> Option<f64> {
        self.modes.iter().find(|m| m.kind == ModeKind::Even).map(|m| m.z0 / 2.0)
    }

    /// Coupling coefficient of a pair: `(Zeven - Zodd) / (Zeven + Zodd)`.
    pub fn coupling(&self) -> Option<f64> {
        let z = |kind| self.modes.iter().find(|m| m.kind == kind).map(|m| m.z0);
        let (even, odd) = (z(ModeKind::Even)?, z(ModeKind::Odd)?);
        Some((even - odd) / (even + odd))
    }
}
