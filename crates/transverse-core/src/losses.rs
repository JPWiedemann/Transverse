//! Frequency-dependent loss from a quasi-static solution.

use serde::{Deserialize, Serialize};

use crate::{ETA0, MU0};

/// Skin depth [m].
pub fn skin_depth(f: f64, sigma: f64) -> f64 {
    1.0 / (std::f64::consts::PI * f * MU0 * sigma).sqrt()
}

/// Surface resistance [Ohm/sq] with Hammerstad roughness factor; returns (Rs, factor).
/// `rq_mm` is the RMS roughness in mm.
pub fn surface_resistance(f: f64, sigma: f64, rq_mm: f64) -> (f64, f64) {
    let rs = (std::f64::consts::PI * f * MU0 / sigma).sqrt();
    let rq = rq_mm * 1e-3;
    let k = if rq > 0.0 {
        1.0 + 2.0 / std::f64::consts::PI * (1.4 * (rq / skin_depth(f, sigma)).powi(2)).atan()
    } else {
        1.0
    };
    (rs * k, k)
}

/// Everything needed to evaluate losses at any frequency.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct LossModel {
    pub z0: f64,
    /// G / omega [F/m].
    pub g_over_omega: f64,
    /// Wheeler: dZ0_air/dn [Ohm/m].
    pub dzdn: f64,
    pub sigma: f64,
    /// RMS roughness [mm].
    pub rq_mm: f64,
}

/// Loss values at one frequency (per metre).
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct LossPoint {
    /// Conductor attenuation [Np/m].
    pub alpha_c: f64,
    /// Dielectric attenuation [Np/m].
    pub alpha_d: f64,
    /// Series resistance [Ohm/m].
    pub r: f64,
    /// Shunt conductance [S/m].
    pub g: f64,
    /// Surface resistance including roughness [Ohm/sq].
    pub rs: f64,
    pub krough: f64,
    /// Skin depth [m].
    pub delta: f64,
}

impl LossModel {
    pub fn at(&self, f: f64) -> LossPoint {
        let w = 2.0 * std::f64::consts::PI * f;
        let (rs, krough) = surface_resistance(f, self.sigma, self.rq_mm);
        let alpha_c = rs / (2.0 * ETA0 * self.z0) * self.dzdn;
        let g = w * self.g_over_omega;
        let alpha_d = g * self.z0 / 2.0;
        LossPoint {
            alpha_c,
            alpha_d,
            r: 2.0 * self.z0 * alpha_c,
            g,
            rs,
            krough,
            delta: skin_depth(f, self.sigma),
        }
    }
}
