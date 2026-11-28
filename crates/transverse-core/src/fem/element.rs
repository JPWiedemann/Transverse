//! Triangles, quadratic shape functions, the degrees of freedom of a mesh and the element integrals.

use std::collections::HashMap;

use super::solve::{FemInput, Order};

/// Gradient coefficients and area of a triangle: `grad lambda_i = (b_i, c_i) / (2 A)`.
#[derive(Clone, Copy)]
pub(super) struct Tri {
    pub(super) b: [f64; 3],
    pub(super) c: [f64; 3],
    pub(super) area: f64,
}

pub(super) fn tri(pts: &[[f64; 2]], t: [u32; 3]) -> Tri {
    let p = [pts[t[0] as usize], pts[t[1] as usize], pts[t[2] as usize]];
    let b = [p[1][1] - p[2][1], p[2][1] - p[0][1], p[0][1] - p[1][1]];
    let c = [p[2][0] - p[1][0], p[0][0] - p[2][0], p[1][0] - p[0][0]];
    let area = 0.5 * ((p[1][0] - p[0][0]) * (p[2][1] - p[0][1]) - (p[2][0] - p[0][0]) * (p[1][1] - p[0][1]));
    Tri { b, c, area }
}

/// Edge (node pair) of a triangle opposite to its vertex `m`.
fn opposite_edge(t: [u32; 3], m: usize) -> (u32, u32) {
    let (a, b) = (t[(m + 1) % 3], t[(m + 2) % 3]);
    (a.min(b), a.max(b))
}

/// Unique edges of the triangulation and, per triangle, the edge index opposite to each vertex.
fn edges_of(tris: &[[u32; 3]]) -> (Vec<[u32; 2]>, Vec<[u32; 3]>) {
    let mut index: HashMap<(u32, u32), u32> = HashMap::new();
    let mut edges = Vec::new();
    let mut per_tri = Vec::with_capacity(tris.len());
    for &t in tris {
        let mut e = [0u32; 3];
        for (m, slot) in e.iter_mut().enumerate() {
            let key = opposite_edge(t, m);
            *slot = *index.entry(key).or_insert_with(|| {
                edges.push([key.0, key.1]);
                (edges.len() - 1) as u32
            });
        }
        per_tri.push(e);
    }
    (edges, per_tri)
}

/// Quadrature points (barycentric) that integrate quadratics exactly: the edge midpoints, weights A/3.
const QP: [[f64; 3]; 3] = [[0.0, 0.5, 0.5], [0.5, 0.0, 0.5], [0.5, 0.5, 0.0]];

/// Gradients of the six quadratic shape functions at barycentric point `l`: vertices 0..2, then the edge
/// functions opposite to vertex 0..2.
pub(super) fn p2_gradients(g: &Tri, l: [f64; 3]) -> [[f64; 2]; 6] {
    let gl: [[f64; 2]; 3] = std::array::from_fn(|p| [g.b[p] / (2.0 * g.area), g.c[p] / (2.0 * g.area)]);
    let comb = |d: [f64; 3]| [0, 1].map(|k| d[0] * gl[0][k] + d[1] * gl[1][k] + d[2] * gl[2][k]);
    let mut out = [[0.0; 2]; 6];
    for i in 0..3 {
        // N_i = l_i (2 l_i - 1)
        let mut d = [0.0; 3];
        d[i] = 4.0 * l[i] - 1.0;
        out[i] = comb(d);
        // edge function opposite to vertex i: N = 4 l_j l_k
        let (j, k) = ((i + 1) % 3, (i + 2) % 3);
        let mut d = [0.0; 3];
        d[j] = 4.0 * l[k];
        d[k] = 4.0 * l[j];
        out[3 + i] = comb(d);
    }
    out
}

/// Degrees of freedom of every element and the conductors they lie on.
pub(super) struct System<'a> {
    pub(super) inp: &'a FemInput<'a>,
    pub(super) order: Order,
    /// Per triangle: dof indices (3 or 6 used).
    pub(super) dofs: Vec<[usize; 6]>,
    pub(super) ndof: usize,
    /// Per degree of freedom: the conductor it lies on (Dirichlet), `None` if free.
    pub(super) cond: Vec<Option<u8>>,
    pub(super) edges: Vec<[u32; 2]>,
}

impl<'a> System<'a> {
    pub(super) fn new(inp: &'a FemInput<'a>, order: Order) -> Self {
        let n = inp.pts.len();
        let mut cond: Vec<Option<u8>> = inp.node_cond.to_vec();
        let (mut edges, mut dofs) = (Vec::new(), Vec::with_capacity(inp.tris.len()));
        if order == Order::P2 {
            let (e, per_tri) = edges_of(inp.tris);
            cond.resize(n + e.len(), None);
            let on_conductor: HashMap<(u32, u32), u8> =
                inp.edge_cond.iter().map(|(e, c)| ((e[0].min(e[1]), e[0].max(e[1])), *c)).collect();
            for (k, ed) in e.iter().enumerate() {
                if let Some(c) = on_conductor.get(&(ed[0], ed[1])) {
                    cond[n + k] = Some(*c);
                }
            }
            edges = e;
            for (t, pe) in inp.tris.iter().zip(&per_tri) {
                dofs.push([
                    t[0] as usize,
                    t[1] as usize,
                    t[2] as usize,
                    n + pe[0] as usize,
                    n + pe[1] as usize,
                    n + pe[2] as usize,
                ]);
            }
        } else {
            dofs.extend(inp.tris.iter().map(|t| [t[0] as usize, t[1] as usize, t[2] as usize, 0, 0, 0]));
        }
        let ndof = cond.len();
        Self { inp, order, dofs, ndof, cond, edges }
    }

    pub(super) fn nloc(&self) -> usize {
        if self.order == Order::P2 { 6 } else { 3 }
    }

    /// Element matrix for triangle `k` scaled by `w`.
    pub(super) fn local(&self, k: usize, w: f64) -> [[f64; 6]; 6] {
        let g = tri(self.inp.pts, self.inp.tris[k]);
        let mut m = [[0.0; 6]; 6];
        if self.order == Order::P1 {
            let s = w / (4.0 * g.area);
            for i in 0..3 {
                for j in 0..3 {
                    m[i][j] = s * (g.b[i] * g.b[j] + g.c[i] * g.c[j]);
                }
            }
            return m;
        }
        for q in QP {
            let gr = p2_gradients(&g, q);
            for i in 0..6 {
                for j in 0..6 {
                    m[i][j] += w * g.area / 3.0 * (gr[i][0] * gr[j][0] + gr[i][1] * gr[j][1]);
                }
            }
        }
        m
    }

    /// `sum_k weight(k) * integral grad U . grad V dA`.
    pub(super) fn energy(&self, u: &[f64], v: &[f64], weight: &dyn Fn(usize) -> f64) -> f64 {
        let nl = self.nloc();
        (0..self.inp.tris.len())
            .map(|k| {
                let g = tri(self.inp.pts, self.inp.tris[k]);
                let d = &self.dofs[k];
                let e = if self.order == Order::P1 {
                    let gradient = |w: &[f64]| {
                        (0..3).fold([0.0, 0.0], |s, i| [s[0] + w[d[i]] * g.b[i], s[1] + w[d[i]] * g.c[i]])
                    };
                    let (a, b) = (gradient(u), gradient(v));
                    (a[0] * b[0] + a[1] * b[1]) / (4.0 * g.area)
                } else {
                    QP.iter()
                        .map(|&q| {
                            let gr = p2_gradients(&g, q);
                            let gradient = |w: &[f64]| {
                                (0..nl).fold([0.0, 0.0], |s, i| {
                                    [s[0] + w[d[i]] * gr[i][0], s[1] + w[d[i]] * gr[i][1]]
                                })
                            };
                            let (a, b) = (gradient(u), gradient(v));
                            g.area / 3.0 * (a[0] * b[0] + a[1] * b[1])
                        })
                        .sum()
                };
                weight(k) * e
            })
            .sum()
    }
}
