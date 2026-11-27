//! Post-processing on triangle meshes: point location, recovery of a continuous E field from element
//! gradients, and equipotentials by marching triangles.

/// A triangle containing a point and the barycentric coordinates of the point in it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Located {
    pub tri: usize,
    pub l: [f64; 3],
}

/// Bucket grid over the bounding boxes of the triangles, for point location.
#[derive(Clone, Debug)]
pub struct TriIndex {
    x0: f64,
    y0: f64,
    inv: f64,
    nx: usize,
    ny: usize,
    cells: Vec<Vec<u32>>,
}

fn bary(pts: &[[f64; 2]], t: [u32; 3], p: [f64; 2]) -> [f64; 3] {
    let (a, b, c) = (pts[t[0] as usize], pts[t[1] as usize], pts[t[2] as usize]);
    let den = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1]);
    let l0 = ((b[1] - c[1]) * (p[0] - c[0]) + (c[0] - b[0]) * (p[1] - c[1])) / den;
    let l1 = ((c[1] - a[1]) * (p[0] - c[0]) + (a[0] - c[0]) * (p[1] - c[1])) / den;
    [l0, l1, 1.0 - l0 - l1]
}

impl TriIndex {
    pub fn new(pts: &[[f64; 2]], tris: &[[u32; 3]]) -> Self {
        let (mut x0, mut x1, mut y0, mut y1) =
            (f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY);
        for p in pts {
            x0 = x0.min(p[0]);
            x1 = x1.max(p[0]);
            y0 = y0.min(p[1]);
            y1 = y1.max(p[1]);
        }
        let n = (tris.len() as f64).sqrt().ceil().max(1.0);
        let inv = n / (x1 - x0).max(y1 - y0).max(f64::MIN_POSITIVE);
        let nx = (((x1 - x0) * inv).ceil() as usize).max(1);
        let ny = (((y1 - y0) * inv).ceil() as usize).max(1);
        let mut cells = vec![Vec::new(); nx * ny];
        let cell = |v: f64, lo: f64, n: usize| (((v - lo) * inv) as usize).min(n - 1);
        for (k, t) in tris.iter().enumerate() {
            let p = [0, 1, 2].map(|i| pts[t[i] as usize]);
            let (bx0, bx1) = (p[0][0].min(p[1][0]).min(p[2][0]), p[0][0].max(p[1][0]).max(p[2][0]));
            let (by0, by1) = (p[0][1].min(p[1][1]).min(p[2][1]), p[0][1].max(p[1][1]).max(p[2][1]));
            for j in cell(by0, y0, ny)..=cell(by1, y0, ny) {
                for i in cell(bx0, x0, nx)..=cell(bx1, x0, nx) {
                    cells[j * nx + i].push(k as u32);
                }
            }
        }
        Self { x0, y0, inv, nx, ny, cells }
    }

    /// The triangle containing `p` (boundary included, with a small relative tolerance), if any.
    pub fn locate(&self, pts: &[[f64; 2]], tris: &[[u32; 3]], p: [f64; 2]) -> Option<Located> {
        let (u, w) = ((p[0] - self.x0) * self.inv, (p[1] - self.y0) * self.inv);
        if u < -1e-9 || w < -1e-9 || u > self.nx as f64 + 1e-9 || w > self.ny as f64 + 1e-9 {
            return None;
        }
        let (i, j) = ((u.max(0.0) as usize).min(self.nx - 1), (w.max(0.0) as usize).min(self.ny - 1));
        const EPS: f64 = -1e-9;
        self.cells[j * self.nx + i].iter().find_map(|&k| {
            let l = bary(pts, tris[k as usize], p);
            l.iter().all(|v| *v >= EPS).then_some(Located { tri: k as usize, l })
        })
    }
}

/// Continuous nodal vectors from element vectors given at the element corners: each node gets the area-weighted
/// average of what its elements say at that corner (nodes without elements get zero).
pub fn recover_nodal_corners(
    pts: &[[f64; 2]],
    tris: &[[u32; 3]],
    at_corners: &[[[f64; 2]; 3]],
) -> Vec<[f64; 2]> {
    let mut sum = vec![[0.0; 2]; pts.len()];
    let mut area = vec![0.0; pts.len()];
    for (t, g) in tris.iter().zip(at_corners) {
        let (a, b, c) = (pts[t[0] as usize], pts[t[1] as usize], pts[t[2] as usize]);
        let ar = 0.5 * ((b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1])).abs();
        for (&n, g) in t.iter().zip(g) {
            sum[n as usize][0] += ar * g[0];
            sum[n as usize][1] += ar * g[1];
            area[n as usize] += ar;
        }
    }
    sum.iter().zip(&area).map(|(s, a)| if *a > 0.0 { [s[0] / a, s[1] / a] } else { [0.0, 0.0] }).collect()
}

/// Segments of the iso-line `value == level` inside one triangle with corner values `c` (marching triangles).
pub fn triangle_contour(p: [[f64; 2]; 3], c: [f64; 3], level: f64) -> Option<[[f64; 2]; 2]> {
    let above = [c[0] >= level, c[1] >= level, c[2] >= level];
    let n = above.iter().filter(|a| **a).count();
    if n == 0 || n == 3 {
        return None;
    }
    // the corner that is alone on its side, and the two crossings of the edges at it
    let alone = (0..3).find(|&k| above[k] != above[(k + 1) % 3] && above[k] != above[(k + 2) % 3])?;
    let cross = |a: usize, b: usize| {
        let t = (level - c[a]) / (c[b] - c[a]);
        [p[a][0] + t * (p[b][0] - p[a][0]), p[a][1] + t * (p[b][1] - p[a][1])]
    };
    Some([cross(alone, (alone + 1) % 3), cross(alone, (alone + 2) % 3)])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit_mesh() -> (Vec<[f64; 2]>, Vec<[u32; 3]>) {
        // 3 x 3 nodes on [0, 2] x [0, 1], uneven in x
        let xs = [0.0, 0.3, 2.0];
        let ys = [0.0, 0.4, 1.0];
        let mut pts = Vec::new();
        for y in ys {
            for x in xs {
                pts.push([x, y]);
            }
        }
        let mut tris = Vec::new();
        for j in 0..2 {
            for i in 0..2 {
                let n = |a: usize, b: usize| (b * 3 + a) as u32;
                tris.push([n(i, j), n(i + 1, j), n(i + 1, j + 1)]);
                tris.push([n(i, j), n(i + 1, j + 1), n(i, j + 1)]);
            }
        }
        (pts, tris)
    }

    #[test]
    fn points_are_located_with_their_barycentric_coordinates() {
        let (pts, tris) = unit_mesh();
        let idx = TriIndex::new(&pts, &tris);
        for p in [[0.1, 0.05], [1.0, 0.7], [1.99, 0.99], [0.3, 0.4], [0.0, 0.0], [2.0, 1.0]] {
            let loc = idx.locate(&pts, &tris, p).unwrap_or_else(|| panic!("{p:?} not found"));
            // the barycentric coordinates reproduce the point
            let t = tris[loc.tri];
            let q = [
                (0..3).map(|i| loc.l[i] * pts[t[i] as usize][0]).sum::<f64>(),
                (0..3).map(|i| loc.l[i] * pts[t[i] as usize][1]).sum::<f64>(),
            ];
            assert!((q[0] - p[0]).abs() < 1e-12 && (q[1] - p[1]).abs() < 1e-12);
            assert!((loc.l.iter().sum::<f64>() - 1.0).abs() < 1e-12);
        }
        assert!(idx.locate(&pts, &tris, [2.5, 0.5]).is_none());
        assert!(idx.locate(&pts, &tris, [1.0, -0.1]).is_none());
    }

    #[test]
    fn recovery_of_a_constant_field_is_constant_and_weights_by_area() {
        let (pts, tris) = unit_mesh();
        let g = vec![[[2.0, -3.0]; 3]; tris.len()];
        assert!(
            recover_nodal_corners(&pts, &tris, &g)
                .iter()
                .all(|v| (v[0] - 2.0).abs() < 1e-12 && (v[1] + 3.0).abs() < 1e-12)
        );
        // two triangles of different area share a node: the larger one counts more
        let pts = vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [-3.0, 0.0], [0.0, -1.0]];
        let tris = vec![[0, 1, 2], [0, 3, 4]];
        let r = recover_nodal_corners(&pts, &tris, &[[[1.0, 0.0]; 3], [[4.0, 0.0]; 3]]);
        // areas 0.5 and 1.5 -> (0.5 * 1 + 1.5 * 4) / 2 = 3.25
        assert!((r[0][0] - 3.25).abs() < 1e-12);
        assert_eq!(r[1], [1.0, 0.0]);
    }

    #[test]
    fn marching_triangles_find_the_isoline_of_a_linear_field() {
        let p = [[0.0, 0.0], [2.0, 0.0], [0.0, 2.0]];
        // V = x: the level 0.5 crosses the two edges at the corner (0, 0) ... corners 0 and 2 have V = 0
        let s = triangle_contour(p, [0.0, 2.0, 0.0], 0.5).unwrap();
        for q in s {
            assert!((q[0] - 0.5).abs() < 1e-12);
        }
        assert!(triangle_contour(p, [0.0, 2.0, 0.0], 3.0).is_none());
        assert!(triangle_contour(p, [1.0, 1.0, 1.0], 0.5).is_none());
        // one corner above: a segment across the corner
        let s = triangle_contour(p, [0.0, 0.0, 2.0], 1.0).unwrap();
        assert!(s.iter().all(|q| (q[1] - 1.0).abs() < 1e-12));
    }
}
