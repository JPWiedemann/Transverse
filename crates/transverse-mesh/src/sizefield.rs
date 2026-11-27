//! The size field: how large an element may be at a point.

use transverse_core::postproc::mesh::TriIndex;
use transverse_geom::{P2, SizeSpec};

use crate::planar::{Pslg, project};

/// Local size targets taken from an earlier mesh: one size per triangle of it. Used for adaptive refinement
/// (smaller targets where the error is large) and to mesh shifted copies of a structure alike.
#[derive(Clone, Copy)]
pub struct Hint<'a> {
    pub pts: &'a [P2],
    pub tris: &'a [[u32; 3]],
    /// Largest element edge wanted inside each triangle.
    pub size: &'a [f64],
}

struct HintField {
    pts: Vec<P2>,
    tris: Vec<[u32; 3]>,
    size: Vec<f64>,
    index: TriIndex,
}

/// Target element size, small at conductor edges and growing with the distance from them
/// (`SizeSpec::at_distance`). Distances are measured to the pieces of conductor boundary in the PSLG;
/// the grounded box does not count, so the far field is coarse.
pub struct SizeField {
    segs: Vec<(P2, P2)>,
    spec: SizeSpec,
    hint: Option<HintField>,
}

impl SizeField {
    pub fn new(pslg: &Pslg, spec: SizeSpec) -> Self {
        Self::with_hint(pslg, spec, None)
    }

    /// With local targets from `hint`: the size is the smaller of the size field and the target of the triangle
    /// of the earlier mesh that contains the point (no restriction where there is none).
    pub fn with_hint(pslg: &Pslg, spec: SizeSpec, hint: Option<Hint<'_>>) -> Self {
        let segs = pslg
            .edges
            .iter()
            .filter(|e| e.owner.is_some_and(|o| o.rank >= 0))
            .map(|e| (pslg.pts[e.a as usize], pslg.pts[e.b as usize]))
            .collect();
        let hint = hint.map(|h| HintField {
            index: TriIndex::new(h.pts, h.tris),
            pts: h.pts.to_vec(),
            tris: h.tris.to_vec(),
            size: h.size.to_vec(),
        });
        Self { segs, spec, hint }
    }

    /// Distance to the nearest conductor edge (infinite without conductors).
    pub fn distance(&self, p: P2) -> f64 {
        self.segs.iter().map(|(a, b)| project(p, *a, *b).1).fold(f64::INFINITY, f64::min)
    }

    pub fn at(&self, p: P2) -> f64 {
        let base = self.spec.at_distance(self.distance(p), self.spec.core_distance(p));
        match &self.hint {
            Some(h) => h.index.locate(&h.pts, &h.tris, p).map_or(base, |l| base.min(h.size[l.tri])),
            None => base,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planar::{Owner, PEdge};

    #[test]
    fn size_grows_away_from_conductor_edges_and_ignores_the_box() {
        let pslg = Pslg {
            pts: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 5.0], [1.0, 5.0]],
            edges: vec![
                PEdge { a: 0, b: 1, owner: Some(Owner { rank: 0, id: 1, tag: 1 }) },
                PEdge { a: 2, b: 3, owner: Some(Owner { rank: -1, id: 0, tag: 9 }) }, // box edge: not counted
            ],
        };
        let f =
            SizeField::new(&pslg, SizeSpec { dmin: 0.01, dmax: 0.5, growth: 1.25, power: 1.0, far: None });
        assert_eq!(f.at([0.5, 0.0]), 0.01);
        assert!((f.distance([0.5, 2.0]) - 2.0).abs() < 1e-15);
        assert!((f.at([0.5, 1.0]) - 0.26).abs() < 1e-12);
        assert_eq!(f.at([0.5, 4.9]), 0.5); // near the box edge but far from any conductor
        // beyond the end of the segment the distance is to the end point
        assert!((f.distance([2.0, 0.0]) - 1.0).abs() < 1e-15);
    }
}
