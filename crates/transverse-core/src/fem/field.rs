//! Field quantities of a quadratic solution: gradients and values at points of the elements.

use super::solve::{FemInput, Order, System, p2_gradients, tri};
use crate::error::{TvError, TvResult};

/// Degrees of freedom of every triangle: its three nodes, then the midpoints of the edges opposite to them.
pub fn element_dofs(inp: &FemInput<'_>, edges: &[[u32; 2]]) -> TvResult<Vec<[u32; 6]>> {
    let sys = System::new(inp, Order::P2);
    if sys.edges != edges {
        return Err(TvError::InvalidParam("fem: the edge list does not belong to this mesh".into()));
    }
    Ok(sys.dofs.iter().map(|d| d.map(|x| x as u32)).collect())
}

/// Gradient of the potential `v` at the three corners of every triangle [V/mm]. The gradient of a quadratic
/// element is linear, so these describe it completely, and they are much closer to the field at the corners of
/// a large element than its gradient at the centroid.
pub fn corner_gradients(inp: &FemInput<'_>, edges: &[[u32; 2]], v: &[f64]) -> TvResult<Vec<[[f64; 2]; 3]>> {
    let dofs = element_dofs(inp, edges)?;
    if v.len() != inp.pts.len() + edges.len() {
        return Err(TvError::InvalidParam("fem: wrong number of potentials".into()));
    }
    const CORNERS: [[f64; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    Ok(inp
        .tris
        .iter()
        .zip(&dofs)
        .map(|(&t, d)| {
            let g = tri(inp.pts, t);
            CORNERS.map(|l| {
                let gr = p2_gradients(&g, l);
                [0, 1].map(|c| (0..6).map(|i| v[d[i] as usize] * gr[i][c]).sum())
            })
        })
        .collect())
}

/// Value of a quadratic function at barycentric `l` from its six dof values (3 nodes, then the 3 edge midpoints
/// opposite to nodes 0, 1, 2).
pub fn p2_values(l: [f64; 3], dof: [f64; 6]) -> f64 {
    let mut s = 0.0;
    for i in 0..3 {
        s += dof[i] * l[i] * (2.0 * l[i] - 1.0);
        s += dof[3 + i] * 4.0 * l[(i + 1) % 3] * l[(i + 2) % 3];
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::Ctl;
    use crate::fem::{Order, solve_lumped};

    #[test]
    fn gradients_and_values_of_a_linear_potential_are_exact() {
        // 3 x 3 node grid on [0, 2] x [0, 1], bottom row 1 V, top row 0 V: V = 1 - y
        let (nx, ny) = (3usize, 3usize);
        let pts: Vec<[f64; 2]> =
            (0..ny).flat_map(|j| (0..nx).map(move |i| [i as f64, j as f64 / 2.0])).collect();
        let mut tris = Vec::new();
        for j in 0..ny - 1 {
            for i in 0..nx - 1 {
                let n = |a: usize, b: usize| (b * nx + a) as u32;
                tris.push([n(i, j), n(i + 1, j), n(i + 1, j + 1)]);
                tris.push([n(i, j), n(i + 1, j + 1), n(i, j + 1)]);
            }
        }
        let er = vec![1.0; tris.len()];
        let tand = vec![0.0; tris.len()];
        let fixed: Vec<Option<f64>> = (0..nx * ny)
            .map(|k| {
                if k < nx {
                    Some(1.0)
                } else if k >= nx * (ny - 1) {
                    Some(0.0)
                } else {
                    None
                }
            })
            .collect();
        let mut fe = Vec::new();
        for i in 0..nx - 1 {
            fe.push(([i as u32, i as u32 + 1], 1.0));
            let t = (nx * (ny - 1) + i) as u32;
            fe.push(([t, t + 1], 0.0));
        }
        let inp = FemInput { pts: &pts, tris: &tris, er: &er, tand: &tand, fixed: &fixed, fixed_edges: &fe };
        let s = solve_lumped(&inp, Order::P2, &Ctl::new()).unwrap();
        for g in corner_gradients(&inp, &s.edges, &s.v).unwrap().into_iter().flatten() {
            assert!((g[0]).abs() < 1e-12 && (g[1] + 1.0).abs() < 1e-12, "{g:?}");
        }
        let dofs = element_dofs(&inp, &s.edges).unwrap();
        for (t, d) in tris.iter().zip(&dofs) {
            let dof = d.map(|k| s.v[k as usize]);
            let l = [0.2, 0.3, 0.5];
            let y: f64 = (0..3).map(|i| l[i] * pts[t[i] as usize][1]).sum();
            assert!((p2_values(l, dof) - (1.0 - y)).abs() < 1e-12);
        }
        assert!(corner_gradients(&inp, &s.edges[1..], &s.v).is_err());
    }
}
