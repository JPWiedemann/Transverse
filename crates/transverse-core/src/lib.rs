//! Numerics for Transverse. No UI, no files, no threads: portable to wasm32.

pub mod control;
pub mod error;
pub mod fd2d;
pub mod quasistatic;
pub mod special;

pub use control::Ctl;
pub use error::{TvError, TvResult};

pub const EPS0: f64 = 8.854_187_812_8e-12;
pub const MU0: f64 = 4e-7 * std::f64::consts::PI;
pub const C_LIGHT: f64 = 299_792_458.0;
pub const ETA0: f64 = MU0 * C_LIGHT;
/// Conversion of nepers to decibels.
pub const NP2DB: f64 = 8.685_889_638_065_037; // 20 / ln(10)
