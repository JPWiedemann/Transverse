//! Assembly and solution of `div(er grad V) = 0` with fixed potentials on a triangle mesh.

use std::collections::HashMap;

use faer::linalg::solvers::Solve;
use faer::sparse::{SparseColMat, Triplet};
use faer::{Mat, Side};

use crate::control::Ctl;
use crate::error::{TvError, TvResult};
use crate::{C_LIGHT, EPS0};

/// Element order: linear (nodes only) or quadratic (nodes and edge midpoints).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Order {
    P1,
    P2,
}

/// The mesh and material data of one solve.
#[derive(Clone, Copy)]
pub struct FemInput<'a> {
    pub pts: &'a [[f64; 2]],
    /// Counter-clockwise node indices.
    pub tris: &'a [[u32; 3]],
    /// Relative permittivity per triangle.
    pub er: &'a [f64],
    /// Loss tangent per triangle.
    pub tand: &'a [f64],
    /// `Some(V)` for nodes with a fixed potential (conductors), `None` for free nodes.
    pub fixed: &'a [Option<f64>],
    /// Edges that lie on a conductor (node pair with `a < b`, potential): their midpoints are fixed too
    /// with quadratic elements. An edge between two conductor nodes that crosses a dielectric is not one.
    pub fixed_edges: &'a [([u32; 2], f64)],
}

/// Capacitance values per metre and the dielectric loss term of one solution.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lumped {
    /// With the dielectrics [F/m].
    pub c: f64,
    /// In air [F/m].
    pub c0: f64,
    /// G / omega [F/m].
    pub g_over_omega: f64,
}

impl Lumped {
    pub fn eeff(&self) -> f64 {
        self.c / self.c0
    }

    pub fn z0(&self) -> f64 {
        1.0 / (C_LIGHT * (self.c * self.c0).sqrt())
    }

    /// Inductance per metre [H/m].
    pub fn l(&self) -> f64 {
        1.0 / (C_LIGHT * C_LIGHT * self.c0)
    }
}

/// Potentials of one solve at all degrees of freedom: the nodes first, then (quadratic elements) the
/// midpoints of `edges` in that order.
#[derive(Clone, Debug)]
pub struct Solved {
    pub lumped: Lumped,
    /// With the dielectrics.
    pub v: Vec<f64>,
    /// In air.
    pub v0: Vec<f64>,
    /// Edges (node pair `a < b`) of the triangulation, empty for linear elements.
    pub edges: Vec<[u32; 2]>,
}

/// Gradient coefficients and area of a triangle: `grad lambda_i = (b_i, c_i) / (2 A)`.
#[derive(Clone, Copy)]
struct Tri {
    b: [f64; 3],
    c: [f64; 3],
    area: f64,
}

fn tri(pts: &[[f64; 2]], t: [u32; 3]) -> Tri {
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
fn p2_gradients(g: &Tri, l: [f64; 3]) -> [[f64; 2]; 6] {
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

/// Degrees of freedom of every element and the fixed values.
pub(super) struct System<'a> {
    inp: &'a FemInput<'a>,
    order: Order,
    /// Per triangle: dof indices (3 or 6 used).
    pub(super) dofs: Vec<[usize; 6]>,
    ndof: usize,
    fixed: Vec<Option<f64>>,
    pub(super) edges: Vec<[u32; 2]>,
}

impl<'a> System<'a> {
    pub(super) fn new(inp: &'a FemInput<'a>, order: Order) -> Self {
        let n = inp.pts.len();
        let mut fixed: Vec<Option<f64>> = inp.fixed.to_vec();
        let (mut edges, mut dofs) = (Vec::new(), Vec::with_capacity(inp.tris.len()));
        if order == Order::P2 {
            let (e, per_tri) = edges_of(inp.tris);
            fixed.resize(n + e.len(), None);
            let on_conductor: HashMap<(u32, u32), f64> =
                inp.fixed_edges.iter().map(|(e, v)| ((e[0].min(e[1]), e[0].max(e[1])), *v)).collect();
            for (k, ed) in e.iter().enumerate() {
                if let Some(v) = on_conductor.get(&(ed[0], ed[1])) {
                    fixed[n + k] = Some(*v);
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
        let ndof = fixed.len();
        Self { inp, order, dofs, ndof, fixed, edges }
    }

    fn nloc(&self) -> usize {
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

    /// Solve with element weights `weight(k)` (the permittivity, or 1 for air).
    fn potentials(&self, weight: &dyn Fn(usize) -> f64, ctl: &Ctl) -> TvResult<Vec<f64>> {
        ctl.check()?;
        ctl.count_solve();
        let mut free = vec![usize::MAX; self.ndof];
        let mut nfree = 0;
        for (k, f) in self.fixed.iter().enumerate() {
            if f.is_none() {
                free[k] = nfree;
                nfree += 1;
            }
        }
        if nfree == 0 {
            return Err(TvError::Solver("no free nodes in the mesh".into()));
        }
        let nl = self.nloc();
        let mut rhs = Mat::<f64>::zeros(nfree, 1);
        let mut trip: Vec<Triplet<usize, usize, f64>> = Vec::with_capacity(nl * nl / 2 * self.inp.tris.len());
        for k in 0..self.inp.tris.len() {
            let m = self.local(k, weight(k));
            let d = &self.dofs[k];
            for i in 0..nl {
                for j in 0..nl {
                    let (a, b) = (d[i], d[j]);
                    match (free[a] != usize::MAX, free[b] != usize::MAX) {
                        (true, true) => {
                            // lower triangle: every unordered pair once
                            if free[a] >= free[b] {
                                trip.push(Triplet::new(free[a], free[b], m[i][j]));
                            }
                        }
                        (true, false) => rhs[(free[a], 0)] -= m[i][j] * self.fixed[b].unwrap_or(0.0),
                        _ => {}
                    }
                }
            }
        }
        let a = SparseColMat::<usize, f64>::try_new_from_triplets(nfree, nfree, &trip)
            .map_err(|e| TvError::Solver(format!("matrix assembly: {e:?}")))?;
        let llt = a
            .sp_cholesky(Side::Lower)
            .map_err(|e| TvError::Solver(format!("Cholesky factorisation: {e:?}")))?;
        llt.solve_in_place(rhs.as_mut());
        Ok((0..self.ndof).map(|k| self.fixed[k].unwrap_or_else(|| rhs[(free[k], 0)])).collect())
    }

    /// `sum_k weight(k) * integral |grad V|^2 dA`.
    fn energy(&self, v: &[f64], weight: &dyn Fn(usize) -> f64) -> f64 {
        let nl = self.nloc();
        (0..self.inp.tris.len())
            .map(|k| {
                let g = tri(self.inp.pts, self.inp.tris[k]);
                let d = &self.dofs[k];
                let e = if self.order == Order::P1 {
                    let (mut gx, mut gy) = (0.0, 0.0);
                    for i in 0..3 {
                        gx += v[d[i]] * g.b[i];
                        gy += v[d[i]] * g.c[i];
                    }
                    (gx * gx + gy * gy) / (4.0 * g.area)
                } else {
                    QP.iter()
                        .map(|&q| {
                            let gr = p2_gradients(&g, q);
                            let (mut gx, mut gy) = (0.0, 0.0);
                            for i in 0..nl {
                                gx += v[d[i]] * gr[i][0];
                                gy += v[d[i]] * gr[i][1];
                            }
                            g.area / 3.0 * (gx * gx + gy * gy)
                        })
                        .sum()
                };
                weight(k) * e
            })
            .sum()
    }
}

/// Solve with the dielectrics and in air; returns the lumped values and both potentials.
pub fn solve_lumped(inp: &FemInput<'_>, order: Order, ctl: &Ctl) -> TvResult<Solved> {
    let (n, nt) = (inp.pts.len(), inp.tris.len());
    if inp.fixed.len() != n || inp.er.len() != nt || inp.tand.len() != nt {
        return Err(TvError::InvalidParam("fem: inconsistent array sizes".into()));
    }
    let sys = System::new(inp, order);
    let v = sys.potentials(&|k| inp.er[k], ctl)?;
    // a structure without dielectrics: the air solution is the same system
    let v0 = if inp.er.iter().all(|e| *e == 1.0) { v.clone() } else { sys.potentials(&|_| 1.0, ctl)? };
    let c = EPS0 * sys.energy(&v, &|k| inp.er[k]);
    let c0 = EPS0 * sys.energy(&v0, &|_| 1.0);
    let g = EPS0 * sys.energy(&v, &|k| inp.er[k] * inp.tand[k]);
    Ok(Solved { lumped: Lumped { c, c0, g_over_omega: g }, v, v0, edges: sys.edges })
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Plate {
        pts: Vec<[f64; 2]>,
        tris: Vec<[u32; 3]>,
        er: Vec<f64>,
        fixed: Vec<Option<f64>>,
        fixed_edges: Vec<([u32; 2], f64)>,
    }

    impl Plate {
        fn input<'a>(&'a self, tand: &'a [f64]) -> FemInput<'a> {
            FemInput {
                pts: &self.pts,
                tris: &self.tris,
                er: &self.er,
                tand,
                fixed: &self.fixed,
                fixed_edges: &self.fixed_edges,
            }
        }
    }

    /// Rectangular strip `w` x `h` in an `nx` x `ny` node grid, each cell split into two triangles;
    /// bottom row at 1 V, top row at 0 V, sides free (Neumann). Rows of cells have the given er.
    fn plate(w: f64, h: f64, nx: usize, ny: usize, er_row: &[f64]) -> Plate {
        let mut pts = Vec::new();
        for j in 0..ny {
            for i in 0..nx {
                // uneven spacing: the exact result must not depend on it
                let fx = (i as f64 / (nx - 1) as f64).powf(1.3);
                let fy = j as f64 / (ny - 1) as f64;
                pts.push([w * fx, h * fy]);
            }
        }
        let (mut tris, mut er) = (Vec::new(), Vec::new());
        for (j, &er_j) in er_row.iter().enumerate().take(ny - 1) {
            for i in 0..nx - 1 {
                let n = |a: usize, b: usize| (b * nx + a) as u32;
                tris.push([n(i, j), n(i + 1, j), n(i + 1, j + 1)]);
                tris.push([n(i, j), n(i + 1, j + 1), n(i, j + 1)]);
                er.extend([er_j, er_j]);
            }
        }
        let fixed = (0..nx * ny)
            .map(|k| {
                if k < nx {
                    Some(1.0)
                } else if k >= (ny - 1) * nx {
                    Some(0.0)
                } else {
                    None
                }
            })
            .collect();
        let mut fixed_edges = Vec::new();
        for i in 0..nx - 1 {
            fixed_edges.push(([i as u32, i as u32 + 1], 1.0));
            let t = ((ny - 1) * nx + i) as u32;
            fixed_edges.push(([t, t + 1], 0.0));
        }
        Plate { pts, tris, er, fixed, fixed_edges }
    }

    fn rel(a: f64, b: f64) -> f64 {
        (a / b - 1.0).abs()
    }

    const ORDERS: [Order; 2] = [Order::P1, Order::P2];

    #[test]
    fn a_parallel_plate_is_exact_for_both_orders() {
        let pl = plate(3.0, 2.0, 7, 5, &[4.0; 4]);
        let tand = vec![0.01; pl.er.len()];
        for order in ORDERS {
            let s = solve_lumped(&pl.input(&tand), order, &Ctl::new()).unwrap();
            let l = s.lumped;
            assert!(rel(l.c, EPS0 * 4.0 * 3.0 / 2.0) < 1e-12, "{order:?}: {} vs {}", l.c, EPS0 * 6.0);
            assert!(rel(l.c0, EPS0 * 3.0 / 2.0) < 1e-12);
            assert!(rel(l.g_over_omega, l.c * 0.01) < 1e-12);
            assert!(rel(l.eeff(), 4.0) < 1e-12);
            // the potential is linear in y at the nodes (and at the edge midpoints for quadratic elements)
            for (p, v) in pl.pts.iter().zip(&s.v) {
                assert!((v - (1.0 - p[1] / 2.0)).abs() < 1e-12);
            }
            for (e, v) in s.edges.iter().zip(s.v.iter().skip(pl.pts.len())) {
                let y = 0.5 * (pl.pts[e[0] as usize][1] + pl.pts[e[1] as usize][1]);
                assert!((v - (1.0 - y / 2.0)).abs() < 1e-12, "edge midpoint");
            }
            assert_eq!(s.edges.is_empty(), order == Order::P1);
        }
    }

    #[test]
    fn two_layers_in_series_are_exact_with_the_interface_on_element_edges() {
        let pl = plate(2.0, 3.0, 5, 4, &[9.0, 2.0, 2.0]);
        let tand = vec![0.0; pl.er.len()];
        for order in ORDERS {
            let l = solve_lumped(&pl.input(&tand), order, &Ctl::new()).unwrap().lumped;
            // layer heights 1, 1, 1 with er 9, 2, 2: series capacitance
            let exact = EPS0 * 2.0 / (1.0 / 9.0 + 1.0 / 2.0 + 1.0 / 2.0);
            assert!(rel(l.c, exact) < 1e-12, "{order:?}: {} vs {exact}", l.c);
            assert!(rel(l.c0, EPS0 * 2.0 / 3.0) < 1e-12);
        }
    }

    #[test]
    fn quadratic_shape_functions_interpolate_quadratics_exactly() {
        let t = [[0.3, 0.1], [2.1, 0.4], [0.9, 1.7]];
        let pts: Vec<[f64; 2]> = t.to_vec();
        let g = tri(&pts, [0, 1, 2]);
        assert!(g.area > 0.0);
        // f = x^2 + x y + 2 y: nodal values at the vertices and the edge midpoints
        let f = |p: [f64; 2]| p[0] * p[0] + p[0] * p[1] + 2.0 * p[1];
        let mid = |a: usize, b: usize| [0.5 * (t[a][0] + t[b][0]), 0.5 * (t[a][1] + t[b][1])];
        let nodal = [f(t[0]), f(t[1]), f(t[2]), f(mid(1, 2)), f(mid(2, 0)), f(mid(0, 1))];
        for l in [[0.2, 0.3, 0.5], [0.0, 0.5, 0.5], [1.0 / 3.0; 3], [0.7, 0.2, 0.1]] {
            let gr = p2_gradients(&g, l);
            let (mut gx, mut gy) = (0.0, 0.0);
            for i in 0..6 {
                gx += nodal[i] * gr[i][0];
                gy += nodal[i] * gr[i][1];
            }
            let p = [
                l[0] * t[0][0] + l[1] * t[1][0] + l[2] * t[2][0],
                l[0] * t[0][1] + l[1] * t[1][1] + l[2] * t[2][1],
            ];
            assert!(
                (gx - (2.0 * p[0] + p[1])).abs() < 1e-12 && (gy - (p[0] + 2.0)).abs() < 1e-12,
                "at {l:?}"
            );
            // the gradients of all shape functions sum to zero (partition of unity)
            let sum: f64 = gr.iter().map(|g| g[0]).sum();
            assert!(sum.abs() < 1e-12);
        }
    }

    #[test]
    fn derived_values_and_input_checks() {
        let l = Lumped { c: 1.8e-10, c0: 5.0e-11, g_over_omega: 0.0 };
        assert!(rel(l.eeff(), 3.6) < 1e-15);
        assert!(rel(l.z0(), 1.0 / (C_LIGHT * (1.8e-10f64 * 5.0e-11).sqrt())) < 1e-15);
        assert!(rel(l.l(), 1.0 / (C_LIGHT * C_LIGHT * 5.0e-11)) < 1e-15);
        let pl = plate(1.0, 1.0, 3, 3, &[1.0; 2]);
        let tand = vec![0.0; pl.er.len()];
        let bad = FemInput { er: &pl.er[1..], ..pl.input(&tand) };
        assert!(solve_lumped(&bad, Order::P1, &Ctl::new()).is_err());
        // cancel is honoured
        let ctl = Ctl::new();
        ctl.cancel();
        assert!(matches!(solve_lumped(&pl.input(&tand), Order::P2, &ctl), Err(TvError::Cancelled)));
    }
}
