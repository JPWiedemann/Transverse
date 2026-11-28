//! Moving a mesh with the walls of the structure: the mesh of one CPWG becomes a mesh of a CPWG with slightly
//! different dimensions, with the same elements.
//!
//! Each coordinate is mapped on its own, piecewise linearly between the positions of the walls (conductor edges
//! and faces, interfaces, domain edges). Etched flanks are slanted: within the copper layer the positions in x
//! are those at the height of the point, so that a point on a flank stays on the flank.

use transverse_mesh::Mesh;

use super::{PlanarParams, Stackup};

/// Wall positions of one structure, in ascending order; the same list of walls for every set of dimensions of
/// that structure.
struct Walls {
    stack: Stackup,
    /// The domain edges and the edges of the region of interest: `[-xd, -xe, xe, xd]`.
    outer: [f64; 4],
    y: [f64; 6],
    t: f64,
}

impl Walls {
    fn new(p: &PlanarParams, margin: f64) -> Self {
        let g = p.geometry(margin);
        Self {
            stack: p.stackup(),
            outer: [-g.xd, -g.xe, g.xe, g.xd],
            y: [g.ymin, g.ybot, 0.0, p.t, g.ytop, g.ymax],
            t: p.t,
        }
    }

    /// The wall positions in x at the fraction `f` of the copper thickness.
    fn x(&self, f: f64) -> Vec<f64> {
        let [xd0, xe0, xe1, xd1] = self.outer;
        let mut x = vec![xd0, xe0];
        x.extend(self.stack.copper_edges_x_at(f));
        x.extend([xe1, xd1]);
        x
    }

    /// The height `y` as a fraction of the copper thickness, limited to the copper layer.
    fn fraction(&self, y: f64) -> f64 {
        if self.t > 0.0 { (y / self.t).clamp(0.0, 1.0) } else { 0.0 }
    }
}

/// The piecewise linear map that takes every `from[i]` to `to[i]`. Walls that coincide in `from` (no copper
/// thickness, no open region beyond a plane) are one point of the map.
fn map(v: f64, from: &[f64], to: &[f64]) -> f64 {
    let last = from.len() - 1;
    // the interval [from[i], from[i + 1]] of non-zero length that contains v
    let i = (0..last).filter(|&i| from[i + 1] > from[i]).find(|&i| v <= from[i + 1]);
    let i = i.unwrap_or_else(|| (0..last).rev().find(|&i| from[i + 1] > from[i]).unwrap_or(0));
    if v == from[i] {
        return to[i];
    }
    if v == from[i + 1] {
        return to[i + 1];
    }
    to[i] + (v - from[i]) / (from[i + 1] - from[i]) * (to[i + 1] - to[i])
}

/// `mesh` of the structure `p` (domain extension `margin`) moved onto the structure `q`.
pub(super) fn move_mesh(mesh: &Mesh, p: &PlanarParams, q: &PlanarParams, margin: f64, q_margin: f64) -> Mesh {
    let (from, to) = (Walls::new(p, margin), Walls::new(q, q_margin));
    let etched = p.undercut() > 0.0;
    // vertical flanks: the same positions at every height
    let flat = (!etched).then(|| (from.x(0.0), to.x(0.0)));
    let mut out = mesh.clone();
    for pt in &mut out.pts {
        // the map in y is linear across the copper layer, so the fraction of its thickness is kept
        let x = match &flat {
            Some((a, b)) => map(pt[0], a, b),
            None => {
                let f = from.fraction(pt[1]);
                map(pt[0], &from.x(f), &to.x(f))
            }
        };
        *pt = [x, map(pt[1], &from.y, &to.y)];
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walls_land_on_walls_and_points_between_them_move_in_proportion() {
        let p = PlanarParams::default();
        let q =
            PlanarParams { w: p.w - 0.004, s: p.s + 0.002, t: p.t - 0.004, h1: p.h1 + 0.002, ..p.clone() };
        let (a, b) = (Walls::new(&p, 0.0), Walls::new(&q, 0.0));
        let (from, to) = ((a.x(0.0), a.y), (b.x(0.0), b.y));
        for (f, t) in from.0.iter().zip(&to.0) {
            assert_eq!(map(*f, &from.0, &to.0), *t);
        }
        for (f, t) in from.1.iter().zip(&to.1) {
            assert_eq!(map(*f, &from.1, &to.1), *t);
        }
        // the middle of the strip surface stays in the middle, the order of points is kept
        assert!(map(0.0, &from.0, &to.0).abs() < 1e-15);
        let ys: Vec<f64> =
            (0..=100).map(|k| from.1[0] + f64::from(k) / 100.0 * (from.1[5] - from.1[0])).collect();
        let moved: Vec<f64> = ys.iter().map(|y| map(*y, &from.1, &to.1)).collect();
        assert!(moved.windows(2).all(|w| w[1] > w[0]));
    }

    #[test]
    fn points_on_an_etched_flank_stay_on_the_flank() {
        use crate::planar::{EtchMode, WideSide};
        for etch_wide in [WideSide::Bottom, WideSide::Top] {
            let p =
                PlanarParams { etch: EtchMode::Factor, etch_factor: 0.8, etch_wide, ..Default::default() };
            let q = p.walls_receded(1e-4);
            let (from, to) = (Walls::new(&p, 0.0), Walls::new(&q, 0.0));
            for f in [0.0, 0.25, 0.7, 1.0] {
                let (a, b) = (from.x(f), to.x(f));
                assert!(a.windows(2).all(|w| w[1] >= w[0]), "{a:?}");
                for (x, want) in a.iter().zip(&b) {
                    assert!((map(*x, &a, &b) - want).abs() < 1e-15);
                }
            }
        }
    }

    #[test]
    fn zero_thickness_copper_stays_flat() {
        let p = PlanarParams { t: 0.0, ..PlanarParams::default() };
        let q = PlanarParams { h1: p.h1 + 0.002, h2: p.h2 + 0.002, ..p.clone() };
        let (a, b) = (Walls::new(&p, 0.0), Walls::new(&q, 0.0));
        let (from, to) = ((a.x(0.0), a.y), (b.x(0.0), b.y));
        assert_eq!(map(0.0, &from.1, &to.1), 0.0);
        assert_eq!(map(from.1[4], &from.1, &to.1), to.1[4]);
        assert_eq!(map(from.1[1], &from.1, &to.1), to.1[1]);
    }
}
