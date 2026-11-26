//! Numerics for Transverse. No UI, no files, no threads: portable to wasm32.

pub mod control;
pub mod error;
pub mod fd2d;
pub mod losses;
pub mod postproc;
pub mod quasistatic;
pub mod special;
pub mod synthesis;
pub mod units;

pub use control::Ctl;
pub use error::{TvError, TvResult};

pub const EPS0: f64 = 8.854_187_812_8e-12;
pub const MU0: f64 = 4e-7 * std::f64::consts::PI;
pub const C_LIGHT: f64 = 299_792_458.0;
pub const ETA0: f64 = MU0 * C_LIGHT;
/// Conversion of nepers to decibels.
pub const NP2DB: f64 = 8.685_889_638_065_037; // 20 / ln(10)

/// Guided wavelength [mm] of a quasi-TEM line: `c / (f sqrt(eps_eff))`.
pub fn guided_wavelength_mm(f_hz: f64, eps_eff: f64) -> f64 {
    C_LIGHT / (f_hz * eps_eff.sqrt()) * 1e3
}

/// Physical length [mm] that has an electrical length of `phase_deg` degrees at `f_hz`.
pub fn length_for_phase_mm(f_hz: f64, eps_eff: f64, phase_deg: f64) -> f64 {
    phase_deg / 360.0 * guided_wavelength_mm(f_hz, eps_eff)
}

/// Electrical length [deg] of a line of `length_mm` for a wavelength of `wavelength_mm`.
pub fn phase_deg(length_mm: f64, wavelength_mm: f64) -> f64 {
    360.0 * length_mm / wavelength_mm
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wavelength_and_phase_are_consistent() {
        // free space at 1 GHz: 299.79 mm; eps_eff = 4 halves it
        assert!((guided_wavelength_mm(1e9, 1.0) - 299.792_458).abs() < 1e-9);
        let l = guided_wavelength_mm(1e9, 4.0);
        assert!((l - 149.896_229).abs() < 1e-6);
        // a quarter wavelength is 90 degrees, and phase grows linearly with length
        assert!((phase_deg(l / 4.0, l) - 90.0).abs() < 1e-12);
        assert!((phase_deg(2.0 * l, l) - 720.0).abs() < 1e-12);
        // and back: the length for a phase has that phase
        assert!((length_for_phase_mm(1e9, 4.0, 90.0) - l / 4.0).abs() < 1e-12);
        assert!(
            (phase_deg(length_for_phase_mm(2.4e9, 3.58, 37.0), guided_wavelength_mm(2.4e9, 3.58)) - 37.0)
                .abs()
                < 1e-12
        );
    }
}
