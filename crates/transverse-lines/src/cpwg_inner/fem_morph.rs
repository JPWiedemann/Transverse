//! Moving a mesh with the walls of the structure: the mesh of one CPWG becomes a mesh of a CPWG with slightly
//! different dimensions, with the same elements.
//!
//! All walls are parallel to an axis, so each coordinate is mapped on its own, piecewise linearly between the
//! positions of the walls (conductor edges and faces, interfaces, domain edges).

use transverse_mesh::Mesh;

use super::CpwgParams;

/// Wall positions in x and y, in ascending order; the same list of walls for every set of dimensions.
fn walls(p: &CpwgParams, margin: f64) -> ([f64; 8], [f64; 6]) {
    let g = p.geometry(margin);
    let (inner, outer) = (p.w / 2.0, p.w / 2.0 + p.s);
    ([-g.xd, -g.xe, -outer, -inner, inner, outer, g.xe, g.xd], [g.ymin, g.ybot, 0.0, p.t, g.ytop, g.ymax])
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
pub(super) fn move_mesh(mesh: &Mesh, p: &CpwgParams, q: &CpwgParams, margin: f64, q_margin: f64) -> Mesh {
    let (from, to) = (walls(p, margin), walls(q, q_margin));
    let mut out = mesh.clone();
    for pt in &mut out.pts {
        *pt = [map(pt[0], &from.0, &to.0), map(pt[1], &from.1, &to.1)];
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walls_land_on_walls_and_points_between_them_move_in_proportion() {
        let p = CpwgParams::default();
        let q = CpwgParams { w: p.w - 0.004, s: p.s + 0.002, t: p.t - 0.004, h1: p.h1 + 0.002, ..p.clone() };
        let (from, to) = (walls(&p, 0.0), walls(&q, 0.0));
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
    fn zero_thickness_copper_stays_flat() {
        let p = CpwgParams { t: 0.0, ..CpwgParams::default() };
        let q = CpwgParams { h1: p.h1 + 0.002, h2: p.h2 + 0.002, ..p.clone() };
        let (from, to) = (walls(&p, 0.0), walls(&q, 0.0));
        assert_eq!(map(0.0, &from.1, &to.1), 0.0);
        assert_eq!(map(from.1[4], &from.1, &to.1), to.1[4]);
        assert_eq!(map(from.1[1], &from.1, &to.1), to.1[1]);
    }
}
