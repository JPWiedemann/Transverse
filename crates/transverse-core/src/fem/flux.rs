//! Flux into conductor boundary edges (for the split of the flux over the ground conductors).
//!
//! The residual ("reaction") at a Dirichlet degree of freedom is the weak flux `R_i = integral(f N_i ds)`
//! of the equilibrated flux density `f` into the conductor. The reactions of all degrees of freedom of a
//! conductor add up to its charge, so distributing them over the boundary edges conserves the total exactly:
//!
//! * the reaction at the midpoint of an edge belongs to that edge, and to the side (element) it comes from;
//! * the reaction at a vertex is distributed by angular sector: the elements around the vertex are split by
//!   the boundary edges at that vertex, and the part of each sector goes half to each of the two edges that
//!   bound it (to the side facing the sector). At the end of a zero-thickness conductor the single edge gets
//!   the whole reaction, by the side each element lies on.
//!
//! Taking the reactions per element keeps the two sides of a zero-thickness conductor apart.

use std::collections::{HashMap, HashSet};

use super::solve::{FemInput, Order, System};
use crate::error::{TvError, TvResult};

/// Flux through one side of a boundary edge into the conductor, positive when the potential falls towards
/// the conductor, in units of the element weights (multiply by EPS0 for C/m with potentials in V).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoundaryFlux {
    /// Nodes of the edge (`a < b`).
    pub a: u32,
    pub b: u32,
    /// The side of the edge the flux comes from: +1 left of `a -> b`, -1 right, 0 for the flux around the
    /// free end of a zero-thickness conductor (from the elements in front of its tip).
    pub side: i8,
    /// A triangle on this side of the edge.
    pub tri: u32,
    pub flux: f64,
}

type Key = (u32, u32);

/// Side of the edge `key` a point lies on: +1 left of `a -> b`, -1 right.
fn side(pts: &[[f64; 2]], key: Key, p: [f64; 2]) -> i8 {
    let (a, b) = (pts[key.0 as usize], pts[key.1 as usize]);
    let cross = (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]);
    if cross >= 0.0 { 1 } else { -1 }
}

/// Fluxes through all sides of the `boundary` edges (node pairs) for the potentials `v` (all degrees of
/// freedom, as in [`super::Solved`]) with element weights `weight(k)` (the permittivity, or 1). `edges` is
/// the edge list of the solution (`Solved::edges`); only quadratic elements are supported.
pub fn boundary_fluxes(
    inp: &FemInput<'_>,
    edges: &[[u32; 2]],
    v: &[f64],
    weight: &dyn Fn(usize) -> f64,
    boundary: &[[u32; 2]],
) -> TvResult<Vec<BoundaryFlux>> {
    let sys = System::new(inp, Order::P2);
    if sys.edges != edges || v.len() != inp.pts.len() + edges.len() {
        return Err(TvError::InvalidParam("fem: the edge list does not belong to this mesh".into()));
    }
    let pts = inp.pts;
    let wanted: HashSet<Key> = boundary.iter().map(|e| (e[0].min(e[1]), e[0].max(e[1]))).collect();
    // reaction of a degree of freedom (local index 0..6) of triangle k
    let reaction = |k: usize, local: usize| -> f64 {
        let m = sys.local(k, weight(k));
        let d = &sys.dofs[k];
        (0..6).map(|j| m[local][j] * v[d[j]]).sum()
    };
    let centroid = |k: usize| {
        let t = inp.tris[k];
        let c = [0, 1, 2].map(|i| pts[t[i] as usize]);
        [(c[0][0] + c[1][0] + c[2][0]) / 3.0, (c[0][1] + c[1][1] + c[2][1]) / 3.0]
    };

    // contacts (edge, side) with their triangle, starting with the reaction at the edge midpoint
    let mut contact: HashMap<(Key, i8), (u32, f64)> = HashMap::new();
    let mut vert_edges: HashMap<u32, Vec<Key>> = HashMap::new();
    for (k, &t) in inp.tris.iter().enumerate() {
        for m in 0..3 {
            let (a, b) = (t[(m + 1) % 3], t[(m + 2) % 3]);
            let key = (a.min(b), a.max(b));
            if wanted.contains(&key) {
                let s = side(pts, key, centroid(k));
                contact.entry((key, s)).or_insert((k as u32, 0.0)).1 += reaction(k, 3 + m);
            }
        }
    }
    for &key in &wanted {
        vert_edges.entry(key.0).or_default().push(key);
        vert_edges.entry(key.1).or_default().push(key);
    }
    // vertex reactions by sector
    let mut tris_at: HashMap<u32, Vec<(usize, usize)>> = HashMap::new();
    for (k, t) in inp.tris.iter().enumerate() {
        for (i, n) in t.iter().enumerate() {
            if vert_edges.contains_key(n) {
                tris_at.entry(*n).or_default().push((k, i));
            }
        }
    }
    let add = |contact: &mut HashMap<(Key, i8), (u32, f64)>, key: Key, s: i8, r: f64| {
        // a side without elements (e.g. outside a box edge) falls back to the other side
        let slot = if contact.contains_key(&(key, s)) { (key, s) } else { (key, -s) };
        if let Some(c) = contact.get_mut(&slot) {
            c.1 += r;
        }
    };
    for (&u, es) in &vert_edges {
        let pu = pts[u as usize];
        // direction angle of every boundary edge at u (u -> other end) and the left side in absolute terms
        let mut dirs: Vec<(f64, Key, i8)> = es
            .iter()
            .map(|&key| {
                let other = if key.0 == u { key.1 } else { key.0 };
                let po = pts[other as usize];
                let left = if key.0 == u { 1 } else { -1 }; // left of u -> other, as a side of key
                ((po[1] - pu[1]).atan2(po[0] - pu[0]), key, left)
            })
            .collect();
        dirs.sort_by(|x, y| x.0.total_cmp(&y.0));
        for &(k, i) in tris_at.get(&u).map_or(&[][..], |v| v.as_slice()) {
            let r = reaction(k, i);
            let c = centroid(k);
            if dirs.len() == 1 {
                // the free end of a zero-thickness conductor: elements in front of the tip are the end contact,
                // the others belong to the side they lie on
                let (phi, key, _) = dirs[0];
                let front = (c[0] - pu[0]) * phi.cos() + (c[1] - pu[1]) * phi.sin() < 0.0;
                if front {
                    contact.entry((key, 0)).or_insert((k as u32, 0.0)).1 += r;
                } else {
                    add(&mut contact, key, side(pts, key, c), r);
                }
                continue;
            }
            let th = (c[1] - pu[1]).atan2(c[0] - pu[0]);
            // the sector [lo, hi) counter-clockwise from the last edge with angle <= th
            let lo = dirs.iter().rposition(|d| d.0 <= th).unwrap_or(dirs.len() - 1);
            let hi = (lo + 1) % dirs.len();
            let (_, klo, left_lo) = dirs[lo];
            let (_, khi, left_hi) = dirs[hi];
            add(&mut contact, klo, left_lo, 0.5 * r); // the sector is counter-clockwise of `lo`: its left side
            add(&mut contact, khi, -left_hi, 0.5 * r); // and clockwise of `hi`: its right side
        }
    }
    let mut out: Vec<BoundaryFlux> = contact
        .into_iter()
        .map(|((key, s), (tri, r))| BoundaryFlux { a: key.0, b: key.1, side: s, tri, flux: -r })
        .collect();
    out.sort_by_key(|f| (f.a, f.b, f.side, f.tri));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::super::solve::solve_lumped;
    use super::*;
    use crate::control::Ctl;

    type Plate = (Vec<[f64; 2]>, Vec<[u32; 3]>, Vec<f64>, Vec<Option<f64>>, Vec<([u32; 2], f64)>);

    /// Plate 3 x 2 split into triangles, bottom row 1 V, top row 0 V, one er per row of cells.
    fn plate(nx: usize, ny: usize, er_row: &[f64], power: f64) -> Plate {
        let (w, h) = (3.0, 2.0);
        let mut pts = Vec::new();
        for j in 0..ny {
            for i in 0..nx {
                pts.push([w * (i as f64 / (nx - 1) as f64).powf(power), h * j as f64 / (ny - 1) as f64]);
            }
        }
        let (mut tris, mut er) = (Vec::new(), Vec::new());
        for (j, &e) in er_row.iter().enumerate().take(ny - 1) {
            for i in 0..nx - 1 {
                let n = |a: usize, b: usize| (b * nx + a) as u32;
                tris.push([n(i, j), n(i + 1, j), n(i + 1, j + 1)]);
                tris.push([n(i, j), n(i + 1, j + 1), n(i, j + 1)]);
                er.extend([e, e]);
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
        let mut fe = Vec::new();
        for i in 0..nx - 1 {
            fe.push(([i as u32, i as u32 + 1], 1.0));
            let t = ((ny - 1) * nx + i) as u32;
            fe.push(([t, t + 1], 0.0));
        }
        (pts, tris, er, fixed, fe)
    }

    fn row(nx: usize, j: usize) -> Vec<[u32; 2]> {
        (0..nx - 1).map(|i| [(j * nx + i) as u32, (j * nx + i + 1) as u32]).collect()
    }

    #[test]
    fn the_flux_through_a_plate_electrode_is_the_charge_and_follows_the_edge_lengths() {
        // uniform spacing: every edge carries exactly its share, also the end edges
        let (pts, tris, er, fixed, fe) = plate(6, 5, &[4.0; 4], 1.0);
        let tand = vec![0.0; er.len()];
        let inp = FemInput { pts: &pts, tris: &tris, er: &er, tand: &tand, fixed: &fixed, fixed_edges: &fe };
        let s = solve_lumped(&inp, Order::P2, &Ctl::new()).unwrap();
        let f = boundary_fluxes(&inp, &s.edges, &s.v, &|k| er[k], &row(6, 4)).unwrap();
        assert_eq!(f.len(), 5, "one triangle per edge on the dielectric side");
        let total: f64 = f.iter().map(|x| x.flux).sum();
        // total flux = er * E * width = 4 * 0.5 * 3, the capacitance over EPS0
        assert!((total - 6.0).abs() < 1e-12, "{total}");
        for x in &f {
            let (pa, pb) = (pts[x.a as usize], pts[x.b as usize]);
            assert!((x.flux - 4.0 * 0.5 * (pb[0] - pa[0])).abs() < 1e-12, "flux follows the edge length");
        }
        // the bottom electrode carries the opposite charge
        let g = boundary_fluxes(&inp, &s.edges, &s.v, &|k| er[k], &row(6, 0)).unwrap();
        let tb: f64 = g.iter().map(|x| x.flux).sum();
        assert!((tb + 6.0).abs() < 1e-12, "{tb}");
    }

    #[test]
    fn the_total_is_conserved_on_uneven_meshes_and_in_layers() {
        let (pts, tris, er, fixed, fe) = plate(7, 4, &[9.0, 2.0, 2.0], 1.4);
        let tand = vec![0.0; er.len()];
        let inp = FemInput { pts: &pts, tris: &tris, er: &er, tand: &tand, fixed: &fixed, fixed_edges: &fe };
        let s = solve_lumped(&inp, Order::P2, &Ctl::new()).unwrap();
        let charge = s.lumped.c / crate::EPS0;
        let top = boundary_fluxes(&inp, &s.edges, &s.v, &|k| er[k], &row(7, 3)).unwrap();
        let sum: f64 = top.iter().map(|x| x.flux).sum();
        assert!((sum - charge).abs() < 1e-12 * charge, "{sum} vs {charge}");
        // in the layers: the flux is uniform in x, so every edge is within ten percent of its length share (the vertex reactions are split half and half between neighbours of different lengths)
        for x in &top {
            let (pa, pb) = (pts[x.a as usize], pts[x.b as usize]);
            let share = (pb[0] - pa[0]) / 3.0;
            assert!(
                (x.flux / charge - share).abs() < 0.10 * share + 1e-12,
                "edge flux {} share {share}",
                x.flux / charge
            );
        }
    }

    #[test]
    fn wrong_edge_lists_and_p1_solutions_are_refused() {
        let (pts, tris, er, fixed, fe) = plate(3, 3, &[1.0; 2], 1.0);
        let tand = vec![0.0; er.len()];
        let inp = FemInput { pts: &pts, tris: &tris, er: &er, tand: &tand, fixed: &fixed, fixed_edges: &fe };
        let s = solve_lumped(&inp, Order::P2, &Ctl::new()).unwrap();
        assert!(boundary_fluxes(&inp, &s.edges[1..], &s.v, &|_| 1.0, &[]).is_err());
        assert!(boundary_fluxes(&inp, &s.edges, &s.v[1..], &|_| 1.0, &[]).is_err());
    }
}
