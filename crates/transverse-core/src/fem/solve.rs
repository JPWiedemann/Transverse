//! Assembly and solution of `div(er grad V) = 0` with fixed potentials on a triangle mesh.
//!
//! Conductor 0 is ground; the conductors 1 to N are the signals. There is one solution per signal (that one at
//! 1 V, all other conductors at 0 V); they share the matrix, which is factorised once.

use faer::linalg::solvers::Solve;
use faer::sparse::{SparseColMat, Triplet};
use faer::{Mat, Side};

use super::element::System;
use crate::control::Ctl;
use crate::error::{TvError, TvResult};
use crate::matrix::SymMatrix;
use crate::modes::CapMatrices;
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
    /// `Some(conductor)` for nodes on a conductor (0 is ground, 1..=N the signals), `None` for free nodes.
    pub node_cond: &'a [Option<u8>],
    /// Edges that lie on a conductor (node pair with `a < b`, conductor): their midpoints are fixed too
    /// with quadratic elements. An edge between two conductor nodes that crosses a dielectric is not one.
    pub edge_cond: &'a [([u32; 2], u8)],
}

impl FemInput<'_> {
    /// Number of signals N: the highest conductor number. Every signal up to it must have nodes.
    pub fn signals(&self) -> TvResult<usize> {
        let mut seen = [false; 256];
        for c in self.node_cond.iter().flatten() {
            seen[usize::from(*c)] = true;
        }
        let n = (1..256).rev().find(|&c| seen[c]).unwrap_or(0);
        if n == 0 || !seen[1..=n].iter().all(|s| *s) {
            return Err(TvError::InvalidParam("fem: the signal conductors must be numbered 1 to N".into()));
        }
        Ok(n)
    }
}

/// Capacitance values per metre and the dielectric loss term of a line with one signal.
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

/// The solutions of one mesh: potentials at all degrees of freedom, the nodes first, then (quadratic elements)
/// the midpoints of `edges` in that order.
#[derive(Clone, Debug)]
pub struct Solved {
    pub caps: CapMatrices,
    /// With the dielectrics, one per signal: `v[i]` has signal `i + 1` at 1 V.
    pub v: Vec<Vec<f64>>,
    /// In air, one per signal.
    pub v0: Vec<Vec<f64>>,
    /// Edges (node pair `a < b`) of the triangulation, empty for linear elements.
    pub edges: Vec<[u32; 2]>,
}

/// Solve with element weights `weight(k)` (the permittivity, or 1 for air): one potential per signal.
fn potentials(
    sys: &System<'_>,
    signals: usize,
    weight: &dyn Fn(usize) -> f64,
    ctl: &Ctl,
) -> TvResult<Vec<Vec<f64>>> {
    ctl.check()?;
    ctl.count_solve();
    let mut free = vec![usize::MAX; sys.ndof];
    let mut nfree = 0;
    for (k, c) in sys.cond.iter().enumerate() {
        if c.is_none() {
            free[k] = nfree;
            nfree += 1;
        }
    }
    if nfree == 0 {
        return Err(TvError::Solver("no free nodes in the mesh".into()));
    }
    let nl = sys.nloc();
    let mut rhs = Mat::<f64>::zeros(nfree, signals);
    let mut trip: Vec<Triplet<usize, usize, f64>> = Vec::with_capacity(nl * nl / 2 * sys.inp.tris.len());
    for k in 0..sys.inp.tris.len() {
        let m = sys.local(k, weight(k));
        let d = &sys.dofs[k];
        for i in 0..nl {
            for j in 0..nl {
                let (a, b) = (d[i], d[j]);
                if free[a] == usize::MAX {
                    continue;
                }
                match sys.cond[b] {
                    // lower triangle: every unordered pair once
                    None => {
                        if free[a] >= free[b] {
                            trip.push(Triplet::new(free[a], free[b], m[i][j]));
                        }
                    }
                    // signal c at 1 V in its own solution; ground adds nothing
                    Some(c) if c > 0 => rhs[(free[a], usize::from(c) - 1)] -= m[i][j],
                    Some(_) => {}
                }
            }
        }
    }
    let a = SparseColMat::<usize, f64>::try_new_from_triplets(nfree, nfree, &trip)
        .map_err(|e| TvError::Solver(format!("matrix assembly: {e:?}")))?;
    let llt =
        a.sp_cholesky(Side::Lower).map_err(|e| TvError::Solver(format!("Cholesky factorisation: {e:?}")))?;
    llt.solve_in_place(rhs.as_mut());
    Ok((0..signals)
        .map(|s| {
            let at = |k: usize| match sys.cond[k] {
                Some(c) => f64::from(usize::from(c) == s + 1),
                None => rhs[(free[k], s)],
            };
            (0..sys.ndof).map(at).collect()
        })
        .collect())
}

/// Solve with the dielectrics and in air, once for every signal; returns the capacitance matrices and the
/// potentials.
pub fn solve_signals(inp: &FemInput<'_>, order: Order, ctl: &Ctl) -> TvResult<Solved> {
    let (n, nt) = (inp.pts.len(), inp.tris.len());
    if inp.node_cond.len() != n || inp.er.len() != nt || inp.tand.len() != nt {
        return Err(TvError::InvalidParam("fem: inconsistent array sizes".into()));
    }
    let signals = inp.signals()?;
    let sys = System::new(inp, order);
    let v = potentials(&sys, signals, &|k| inp.er[k], ctl)?;
    // a structure without dielectrics: the air solution is the same system
    let v0 =
        if inp.er.iter().all(|e| *e == 1.0) { v.clone() } else { potentials(&sys, signals, &|_| 1.0, ctl)? };
    // the energy of two solutions is their entry of the capacitance matrix
    let matrix = |u: &[Vec<f64>], weight: &dyn Fn(usize) -> f64| {
        SymMatrix::from_fn(signals, |i, j| EPS0 * sys.energy(&u[i], &u[j], weight))
    };
    let caps = CapMatrices {
        c: matrix(&v, &|k| inp.er[k]),
        c0: matrix(&v0, &|_| 1.0),
        g_over_omega: matrix(&v, &|k| inp.er[k] * inp.tand[k]),
    };
    Ok(Solved { caps, v, v0, edges: sys.edges })
}

#[cfg(test)]
mod tests {
    use super::super::element::{p2_gradients, tri};
    use super::*;

    struct Plate {
        pts: Vec<[f64; 2]>,
        tris: Vec<[u32; 3]>,
        er: Vec<f64>,
        node_cond: Vec<Option<u8>>,
        edge_cond: Vec<([u32; 2], u8)>,
    }

    impl Plate {
        fn input<'a>(&'a self, tand: &'a [f64]) -> FemInput<'a> {
            FemInput {
                pts: &self.pts,
                tris: &self.tris,
                er: &self.er,
                tand,
                node_cond: &self.node_cond,
                edge_cond: &self.edge_cond,
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
        let node_cond = (0..nx * ny)
            .map(|k| {
                if k < nx {
                    Some(1)
                } else if k >= (ny - 1) * nx {
                    Some(0)
                } else {
                    None
                }
            })
            .collect();
        let mut edge_cond = Vec::new();
        for i in 0..nx - 1 {
            edge_cond.push(([i as u32, i as u32 + 1], 1));
            let t = ((ny - 1) * nx + i) as u32;
            edge_cond.push(([t, t + 1], 0));
        }
        Plate { pts, tris, er, node_cond, edge_cond }
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
            let s = solve_signals(&pl.input(&tand), order, &Ctl::new()).unwrap();
            let l = s.caps.single().unwrap();
            assert!(rel(l.c, EPS0 * 4.0 * 3.0 / 2.0) < 1e-12, "{order:?}: {} vs {}", l.c, EPS0 * 6.0);
            assert!(rel(l.c0, EPS0 * 3.0 / 2.0) < 1e-12);
            assert!(rel(l.g_over_omega, l.c * 0.01) < 1e-12);
            assert!(rel(l.eeff(), 4.0) < 1e-12);
            // the potential is linear in y at the nodes (and at the edge midpoints for quadratic elements)
            for (p, v) in pl.pts.iter().zip(&s.v[0]) {
                assert!((v - (1.0 - p[1] / 2.0)).abs() < 1e-12);
            }
            for (e, v) in s.edges.iter().zip(s.v[0].iter().skip(pl.pts.len())) {
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
            let l = solve_signals(&pl.input(&tand), order, &Ctl::new()).unwrap().caps.single().unwrap();
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
        assert!(solve_signals(&bad, Order::P1, &Ctl::new()).is_err());
        // cancel is honoured
        let ctl = Ctl::new();
        ctl.cancel();
        assert!(matches!(solve_signals(&pl.input(&tand), Order::P2, &ctl), Err(TvError::Cancelled)));
    }

    /// Both electrodes of the plate as signals, with ground nowhere: the field between them is that of the
    /// capacitor, so the matrix is `C [[1, -1], [-1, 1]]`, and one solve per signal gives complementary potentials.
    #[test]
    fn two_signals_give_the_capacitance_matrix_with_one_factorisation_per_material_set() {
        let mut pl = plate(3.0, 2.0, 7, 5, &[4.0; 4]);
        for c in pl.node_cond.iter_mut().flatten() {
            *c = if *c == 1 { 1 } else { 2 };
        }
        for (_, c) in &mut pl.edge_cond {
            *c = if *c == 1 { 1 } else { 2 };
        }
        let tand = vec![0.01; pl.er.len()];
        for order in ORDERS {
            let ctl = Ctl::new();
            let s = solve_signals(&pl.input(&tand), order, &ctl).unwrap();
            let c = EPS0 * 4.0 * 3.0 / 2.0;
            for (i, j) in [(0, 0), (0, 1), (1, 1)] {
                let sign = if i == j { 1.0 } else { -1.0 };
                assert!(rel(s.caps.c.get(i, j), sign * c) < 1e-12, "{order:?} ({i}, {j})");
                assert!(rel(s.caps.c0.get(i, j), sign * c / 4.0) < 1e-12);
                assert!(rel(s.caps.g_over_omega.get(i, j), sign * c * 0.01) < 1e-12);
            }
            for (a, b) in s.v[0].iter().zip(&s.v[1]) {
                assert!((a + b - 1.0).abs() < 1e-12);
            }
        }
    }

    #[test]
    fn signals_must_be_numbered_without_gaps() {
        let mut pl = plate(1.0, 1.0, 3, 3, &[1.0; 2]);
        let tand = vec![0.0; pl.er.len()];
        assert_eq!(pl.input(&tand).signals().unwrap(), 1);
        for c in pl.node_cond.iter_mut().flatten() {
            *c *= 2;
        }
        assert!(solve_signals(&pl.input(&tand), Order::P2, &Ctl::new()).is_err());
        pl.node_cond.iter_mut().for_each(|c| *c = c.map(|_| 0));
        assert!(pl.input(&tand).signals().is_err());
    }
}
