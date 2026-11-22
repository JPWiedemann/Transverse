//! Colour maps (sampled from matplotlib, see `python/export_colormaps.py`).

use crate::colormap_data::{INFERNO, VIRIDIS};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cmap {
    Inferno,
    Viridis,
    /// White (0) to black (1), like matplotlib's `Greys`.
    Greys,
}

impl Cmap {
    /// Colour at `t` in 0..=1 (clamped, NaN gives the low end).
    pub fn sample(self, t: f64) -> [u8; 3] {
        let t = if t.is_nan() { 0.0 } else { t.clamp(0.0, 1.0) };
        let table = match self {
            Cmap::Inferno => &INFERNO,
            Cmap::Viridis => &VIRIDIS,
            Cmap::Greys => {
                let v = (255.0 * (1.0 - t)).round() as u8;
                return [v, v, v];
            }
        };
        let pos = t * 255.0;
        let i = (pos.floor() as usize).min(254);
        let frac = pos - i as f64;
        let (a, b) = (table[i], table[i + 1]);
        let mix = |k: usize| (a[k] as f64 + (b[k] as f64 - a[k] as f64) * frac).round() as u8;
        [mix(0), mix(1), mix(2)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoints_match_matplotlib() {
        assert_eq!(Cmap::Inferno.sample(0.0), [0, 0, 4]);
        assert_eq!(Cmap::Inferno.sample(1.0), [252, 255, 164]);
        assert_eq!(Cmap::Viridis.sample(0.0), [68, 1, 84]);
        assert_eq!(Cmap::Viridis.sample(1.0), [253, 231, 37]);
        assert_eq!(Cmap::Greys.sample(0.0), [255, 255, 255]);
        assert_eq!(Cmap::Greys.sample(1.0), [0, 0, 0]);
    }

    #[test]
    fn out_of_range_and_nan_are_clamped() {
        assert_eq!(Cmap::Viridis.sample(-3.0), Cmap::Viridis.sample(0.0));
        assert_eq!(Cmap::Viridis.sample(7.0), Cmap::Viridis.sample(1.0));
        assert_eq!(Cmap::Viridis.sample(f64::NAN), Cmap::Viridis.sample(0.0));
    }

    #[test]
    fn interpolates_between_table_entries() {
        let mid = Cmap::Inferno.sample(0.5 / 255.0);
        let (a, b) = (INFERNO[0], INFERNO[1]);
        for k in 0..3 {
            assert!(mid[k] >= a[k].min(b[k]) && mid[k] <= a[k].max(b[k]));
        }
    }
}
