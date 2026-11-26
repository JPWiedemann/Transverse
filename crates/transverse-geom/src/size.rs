//! How fine a mesh of a cross-section should be.

use serde::{Deserialize, Serialize};
use transverse_core::{TvError, TvResult};

/// Coarser elements outside the region that matters (the open space around a board).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Far {
    /// Region `[x0, x1, y0, y1]` where `dmax` applies.
    pub core: [f64; 4],
    /// Largest element size far away from the core.
    pub dfar: f64,
}

/// Target element size [mm]: `dmin` at conductor edges, growing by `growth` per unit distance
/// (`h = dmin + (growth - 1) * distance`) up to `dmax` inside the core and, with `far`, up to `dfar` far
/// outside it (growing with the distance from the core). Everything derived from the geometry.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SizeSpec {
    pub dmin: f64,
    pub dmax: f64,
    /// 1.2 to 1.3 is a good grading; larger values give fewer but worse-shaped elements.
    pub growth: f64,
    pub far: Option<Far>,
}

impl SizeSpec {
    pub fn validate(&self) -> TvResult<()> {
        let far_ok = self.far.is_none_or(|f| {
            f.dfar >= self.dmax && f.dfar.is_finite() && f.core.iter().all(|v| v.is_finite())
        });
        if self.dmin.is_finite()
            && self.dmin > 0.0
            && self.dmax >= self.dmin
            && self.growth > 1.0
            && self.growth <= 2.0
            && far_ok
        {
            Ok(())
        } else {
            Err(TvError::InvalidParam("size spec: 0 < dmin <= dmax and 1 < growth <= 2 are required".into()))
        }
    }

    /// The same grading with every size multiplied by `factor` (used by the accuracy levels).
    pub fn scaled(&self, factor: f64) -> Self {
        Self {
            dmin: self.dmin * factor,
            dmax: self.dmax * factor,
            growth: self.growth,
            far: self.far.map(|f| Far { core: f.core, dfar: f.dfar * factor }),
        }
    }

    /// Target size at distance `d` from the nearest conductor edge, for a point `dcore` away from the
    /// core region (0 inside it).
    pub fn at_distance(&self, d: f64, dcore: f64) -> f64 {
        let cap = match self.far {
            Some(f) => f.dfar.min(self.dmax + (self.growth - 1.0) * dcore),
            None => self.dmax,
        };
        (self.dmin + (self.growth - 1.0) * d).min(cap)
    }

    /// Distance from `p` to the core region (0 if there is none or `p` is inside).
    pub fn core_distance(&self, p: [f64; 2]) -> f64 {
        match self.far {
            Some(f) => {
                let c = f.core;
                (c[0] - p[0]).max(0.0).max(p[0] - c[1]).hypot((c[2] - p[1]).max(0.0).max(p[1] - c[3]))
            }
            None => 0.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grows_with_distance_up_to_dmax_and_scales_as_a_whole() {
        let s = SizeSpec { dmin: 0.01, dmax: 0.1, growth: 1.25, far: None };
        s.validate().unwrap();
        assert_eq!(s.at_distance(0.0, 0.0), 0.01);
        assert!((s.at_distance(0.2, 0.0) - 0.06).abs() < 1e-15);
        assert_eq!(s.at_distance(100.0, 0.0), 0.1);
        let h = s.scaled(0.5);
        assert_eq!((h.dmin, h.dmax, h.growth), (0.005, 0.05, 1.25));
        assert!(SizeSpec { dmin: 0.0, ..s }.validate().is_err());
        assert!(SizeSpec { growth: 1.0, ..s }.validate().is_err());
        assert!(SizeSpec { dmax: 0.001, ..s }.validate().is_err());
    }

    #[test]
    fn the_far_field_is_coarser_and_grows_with_the_distance_from_the_core() {
        let s = SizeSpec {
            dmin: 0.01,
            dmax: 0.1,
            growth: 1.25,
            far: Some(Far { core: [0.0, 1.0, 0.0, 1.0], dfar: 0.5 }),
        };
        s.validate().unwrap();
        assert_eq!(s.core_distance([0.5, 0.5]), 0.0);
        assert!((s.core_distance([2.0, 0.5]) - 1.0).abs() < 1e-15);
        assert!((s.core_distance([2.0, 2.0]) - 2.0f64.sqrt()).abs() < 1e-15);
        // far from conductors the cap applies: dmax in the core, growing outside, dfar at the end
        assert_eq!(s.at_distance(100.0, 0.0), 0.1);
        assert!((s.at_distance(100.0, 0.4) - 0.2).abs() < 1e-15);
        assert_eq!(s.at_distance(100.0, 10.0), 0.5);
        // near a conductor the grading from the edge wins everywhere
        assert_eq!(s.at_distance(0.0, 10.0), 0.01);
        assert_eq!(s.scaled(0.5).far.unwrap().dfar, 0.25);
        assert!(SizeSpec { far: Some(Far { core: [0.0; 4], dfar: 0.05 }), ..s }.validate().is_err());
    }
}
