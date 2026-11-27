//! Field data on a triangle mesh with quadratic elements: the counterpart of [`crate::model::FieldData`]
//! (which is on the tensor grid) with the same methods, so frontends can draw both.

use transverse_core::fem::p2_values;
use transverse_core::postproc::mesh::{Located, TriIndex, triangle_contour};
use transverse_core::postproc::{StreamOptions, field_lines_with};

use crate::model::{Bounds, DrawRect};

/// Potentials (both solutions), the recovered E field and the drawing information of a mesh solution.
/// Fields are for 1 V on the signal, lengths in mm, E in V/mm.
#[derive(Clone, Debug)]
pub struct MeshFieldData {
    pub pts: Vec<[f64; 2]>,
    /// Counter-clockwise node indices (only triangles in the dielectric).
    pub tris: Vec<[u32; 3]>,
    /// Per triangle: its three nodes, then the midpoints of the edges opposite to them (dof indices into `v`).
    pub dofs: Vec<[u32; 6]>,
    /// Relative permittivity per triangle.
    pub tri_er: Vec<f64>,
    /// Potential with dielectrics at all degrees of freedom (nodes, then edge midpoints).
    pub v: Vec<f64>,
    /// Potential in air.
    pub v0: Vec<f64>,
    /// E field of the dielectric solution, continuous (recovered from the element gradients), at the nodes.
    pub e_nodal: Vec<[f64; 2]>,
    /// All edges of the triangulation (for a mesh overlay); the midpoint dofs follow the nodes in this order.
    pub edges: Vec<[u32; 2]>,
    pub index: TriIndex,
    /// `[x0, x1, y0, y1]` of the mesh.
    pub bounds: Bounds,
    pub conductors: Vec<DrawRect>,
    /// y positions of dashed guide lines (board surface where a plane is missing).
    pub dashed_y: Vec<f64>,
    /// y positions of dotted lines (copper surfaces).
    pub dotted_y: Vec<f64>,
    pub view_zoom: Bounds,
    pub view_full: Bounds,
    pub z0: f64,
}

impl MeshFieldData {
    pub fn locate(&self, p: [f64; 2]) -> Option<Located> {
        self.index.locate(&self.pts, &self.tris, p)
    }

    fn dof_values(&self, tri: usize, v: &[f64]) -> [f64; 6] {
        self.dofs[tri].map(|d| v[d as usize])
    }

    /// Potential at a point of the dielectric (`None` inside a conductor or outside the domain).
    pub fn potential(&self, p: [f64; 2], air: bool) -> Option<f64> {
        let loc = self.locate(p)?;
        Some(p2_values(loc.l, self.dof_values(loc.tri, if air { &self.v0 } else { &self.v })))
    }

    /// Recovered E vector (V/mm) at a point.
    pub fn e_vector(&self, p: [f64; 2]) -> Option<[f64; 2]> {
        let loc = self.locate(p)?;
        let t = self.tris[loc.tri];
        let comp = |c: usize| (0..3).map(|i| loc.l[i] * self.e_nodal[t[i] as usize][c]).sum::<f64>();
        Some([comp(0), comp(1)])
    }

    /// |E| (V/mm) at a point, interpolated from the nodal magnitudes.
    pub fn e_mag(&self, p: [f64; 2]) -> Option<f64> {
        let loc = self.locate(p)?;
        let t = self.tris[loc.tri];
        Some(
            (0..3)
                .map(|i| loc.l[i] * self.e_nodal[t[i] as usize][0].hypot(self.e_nodal[t[i] as usize][1]))
                .sum(),
        )
    }

    /// |E| at the nodes (V/mm).
    pub fn e_mag_nodal(&self) -> Vec<f64> {
        self.e_nodal.iter().map(|e| e[0].hypot(e[1])).collect()
    }

    /// A third of the area of the triangles around each node.
    pub fn node_areas(&self) -> Vec<f64> {
        let mut a = vec![0.0; self.pts.len()];
        for t in &self.tris {
            let [p, q, r] = t.map(|i| self.pts[i as usize]);
            let area = 0.5 * ((q[0] - p[0]) * (r[1] - p[1]) - (r[0] - p[0]) * (q[1] - p[1])).abs();
            for &n in t {
                a[n as usize] += area / 3.0;
            }
        }
        a
    }

    pub fn er_at(&self, p: [f64; 2]) -> Option<f64> {
        Some(self.tri_er[self.locate(p)?.tri])
    }

    /// Equipotential segments for the given potentials (V): marching triangles on the quadratic interpolant,
    /// each element split into four linear pieces at its edge midpoints.
    pub fn equipotentials(&self, levels: &[f64]) -> Vec<(f64, Vec<[[f64; 2]; 2]>)> {
        let mut out: Vec<(f64, Vec<[[f64; 2]; 2]>)> = levels.iter().map(|&l| (l, Vec::new())).collect();
        for (k, t) in self.tris.iter().enumerate() {
            let c = [0, 1, 2].map(|i| self.pts[t[i] as usize]);
            let val = self.dof_values(k, &self.v);
            let (lo, hi) =
                val.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |r, &v| (r.0.min(v), r.1.max(v)));
            // midpoint of the edge opposite to vertex m: position and dof value
            let mid = |m: usize| {
                let (a, b) = (c[(m + 1) % 3], c[(m + 2) % 3]);
                ([0.5 * (a[0] + b[0]), 0.5 * (a[1] + b[1])], val[3 + m])
            };
            let (m0, m1, m2) = (mid(0), mid(1), mid(2));
            // corner pieces and the middle one
            let pieces: [([[f64; 2]; 3], [f64; 3]); 4] = [
                ([c[0], m2.0, m1.0], [val[0], m2.1, m1.1]),
                ([c[1], m0.0, m2.0], [val[1], m0.1, m2.1]),
                ([c[2], m1.0, m0.0], [val[2], m1.1, m0.1]),
                ([m0.0, m1.0, m2.0], [m0.1, m1.1, m2.1]),
            ];
            for (level, segs) in &mut out {
                if *level < lo || *level > hi {
                    continue;
                }
                for (p, v) in &pieces {
                    segs.extend(triangle_contour(*p, *v, *level));
                }
            }
        }
        out
    }

    /// Field lines of the dielectric solution inside `view`.
    pub fn field_lines(&self, view: Bounds, density: (f64, f64)) -> Vec<Vec<[f64; 2]>> {
        field_lines_with(
            self.bounds,
            &|p| self.e_vector(p).map_or((0.0, 0.0), |e| (e[0], e[1])),
            &|p| self.locate(p).is_none(),
            &StreamOptions { view, density },
        )
    }
}
