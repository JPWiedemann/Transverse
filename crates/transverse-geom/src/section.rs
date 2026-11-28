//! The cross-section: a domain with shapes painted in order.

use serde::{Deserialize, Serialize};
use transverse_core::{TvError, TvResult};

use crate::shape::Shape;

pub type MaterialId = u16;
/// Conductor number: [`GROUND`] is at 0 V, the others are driven (phase 2: only [`SIGNAL`], at 1 V).
pub type ConductorId = u8;
/// Meaning of a conductor piece for the line type (e.g. "plane 1", "coplanar ground"); opaque to the mesher.
pub type Tag = u8;

pub const GROUND: ConductorId = 0;
pub const SIGNAL: ConductorId = 1;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Material {
    pub er: f64,
    pub tand: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Paint {
    Dielectric(MaterialId),
    Conductor { id: ConductorId, tag: Tag },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Item {
    pub shape: Shape,
    pub paint: Paint,
}

/// Domain `[x0, x1, y0, y1]` [mm], filled with `background`; `items` are painted in order, where they
/// overlap the later one wins. The outer boundary is a grounded box (Dirichlet 0 V) if `box_grounded`,
/// carrying `box_tag`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CrossSection {
    pub domain: [f64; 4],
    pub background: MaterialId,
    pub materials: Vec<Material>,
    pub items: Vec<Item>,
    pub box_grounded: bool,
    pub box_tag: Tag,
}

impl CrossSection {
    /// Length of the domain diagonal: the scale all relative tolerances refer to.
    pub fn diag(&self) -> f64 {
        let d = self.domain;
        (d[1] - d[0]).hypot(d[3] - d[2])
    }

    /// Whether `p` is inside the domain (boundary included, with a relative tolerance).
    pub fn in_domain(&self, p: [f64; 2], rel_tol: f64) -> bool {
        let (d, t) = (self.domain, rel_tol * self.diag());
        p[0] >= d[0] - t && p[0] <= d[1] + t && p[1] >= d[2] - t && p[1] <= d[3] + t
    }

    pub fn validate(&self) -> TvResult<()> {
        let bad = |m: String| Err(TvError::InvalidParam(format!("cross-section: {m}")));
        let d = self.domain;
        if !(d.iter().all(|v| v.is_finite()) && d[1] > d[0] && d[3] > d[2]) {
            return bad("the domain must be a finite rectangle with positive size".into());
        }
        if self.materials.is_empty() || usize::from(self.background) >= self.materials.len() {
            return bad("the background material does not exist".into());
        }
        for (k, m) in self.materials.iter().enumerate() {
            if !(m.er.is_finite() && m.er >= 1.0 && m.tand.is_finite() && m.tand >= 0.0) {
                return bad(format!("material {k}: er must be >= 1 and tan d >= 0"));
            }
        }
        for (k, it) in self.items.iter().enumerate() {
            if !it.shape.all_finite() || it.shape.is_degenerate() {
                return bad(format!("item {k}: degenerate or non-finite shape"));
            }
            if let Paint::Dielectric(m) = it.paint {
                if usize::from(m) >= self.materials.len() {
                    return bad(format!("item {k}: material {m} does not exist"));
                }
                if matches!(it.shape, Shape::Segment { .. }) {
                    return bad(format!("item {k}: a segment can only be a conductor"));
                }
            }
            let b = it.shape.bounds();
            let t = 1e-9 * self.diag();
            if b[0] < d[0] - t || b[1] > d[1] + t || b[2] < d[2] - t || b[3] > d[3] + t {
                return bad(format!("item {k} reaches outside the domain"));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn section() -> CrossSection {
        CrossSection {
            domain: [0.0, 10.0, 0.0, 4.0],
            background: 0,
            materials: vec![Material { er: 1.0, tand: 0.0 }, Material { er: 3.5, tand: 0.02 }],
            items: vec![
                Item { shape: Shape::rect(0.0, 0.0, 10.0, 2.0), paint: Paint::Dielectric(1) },
                Item {
                    shape: Shape::segment([3.0, 2.0], [7.0, 2.0]),
                    paint: Paint::Conductor { id: SIGNAL, tag: 1 },
                },
            ],
            box_grounded: true,
            box_tag: 9,
        }
    }

    #[test]
    fn a_valid_section_passes_and_round_trips_through_json() {
        let s = section();
        s.validate().unwrap();
        let back: CrossSection = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back, s);
        assert!((s.diag() - 10.0f64.hypot(4.0)).abs() < 1e-12);
        assert!(s.in_domain([10.0, 4.0], 0.0) && !s.in_domain([10.1, 4.0], 1e-3));
    }

    #[test]
    fn invalid_sections_are_rejected_with_a_reason() {
        let mut s = section();
        s.domain = [0.0, 0.0, 0.0, 4.0];
        assert!(s.validate().is_err());
        let mut s = section();
        s.background = 5;
        assert!(s.validate().unwrap_err().to_string().contains("background"));
        let mut s = section();
        s.materials[1].er = 0.5;
        assert!(s.validate().is_err());
        let mut s = section();
        s.items[1].shape = Shape::segment([3.0, 2.0], [3.0, 2.0]);
        assert!(s.validate().unwrap_err().to_string().contains("item 1"));
        let mut s = section();
        s.items[0].shape = Shape::rect(0.0, 0.0, 11.0, 2.0);
        assert!(s.validate().unwrap_err().to_string().contains("outside"));
        let mut s = section();
        s.items[1].paint = Paint::Dielectric(1);
        assert!(s.validate().unwrap_err().to_string().contains("segment"));
    }
}
