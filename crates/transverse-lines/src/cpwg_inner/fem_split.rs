//! Split of the flux and of the HF return current over the ground conductors on the FEM mesh.
//!
//! Same definitions as [`super::ground_split`] for the grid: the dielectric solution gives the E-field share
//! (where the field lines end), the air solution the return-current share (valid when the skin depth is much
//! smaller than the thickness). The fluxes come from the reactions at the midpoints of the conductor boundary
//! edges (`transverse_core::fem::boundary_fluxes`); the coplanar-ground faces are told apart by the side of the
//! edge the element lies on.

use std::collections::HashMap;

use transverse_core::fem::{BoundaryFlux, boundary_fluxes};
use transverse_core::{TvError, TvResult};
use transverse_geom::GROUND;

use super::fem::{FemSolution, InputData};
use super::ground_split::{GroundCat, Profile, Shares, Split, merge_equal_x};
use super::{C_BND, C_COP, C_F1, C_F2, C_PL1, C_PL2, CpwgParams};

/// One side of a ground boundary edge.
struct Contact {
    cat: GroundCat,
    /// Centre of the edge: x [mm].
    x: f64,
    /// Range of |x| the edge covers [mm].
    reach: (f64, f64),
    len: f64,
    flux_d: f64,
    flux_0: f64,
}

/// Ground split of a FEM solution (quadratic elements).
pub fn ground_split_fem(p: &CpwgParams, sol: &FemSolution) -> TvResult<Split> {
    let mesh = &sol.mesh;
    let cs = p.cross_section(sol.margin);
    let data = InputData::new(&cs, mesh);
    let inp = data.input(mesh);

    // ground boundary edges with their tag (signal edges are not part of the split)
    let tags: HashMap<(u32, u32), u8> =
        mesh.edges.iter().filter(|e| e.id == GROUND).map(|e| ((e.a.min(e.b), e.a.max(e.b)), e.tag)).collect();
    let boundary: Vec<[u32; 2]> = tags.keys().map(|&(a, b)| [a, b]).collect();
    let fd: Vec<BoundaryFlux> = boundary_fluxes(&inp, &sol.edges, &sol.v, &|k| data.er[k], &boundary)?;
    let f0: Vec<BoundaryFlux> = boundary_fluxes(&inp, &sol.edges, &sol.v0, &|_| 1.0, &boundary)?;
    if fd.len() != f0.len() {
        return Err(TvError::Solver("ground split: inconsistent boundary fluxes".into()));
    }

    let mut contacts = Vec::with_capacity(fd.len());
    for (d, z) in fd.iter().zip(&f0) {
        let tag = tags[&(d.a, d.b)];
        let (pa, pb) = (mesh.pts[d.a as usize], mesh.pts[d.b as usize]);
        let mid = [0.5 * (pa[0] + pb[0]), 0.5 * (pa[1] + pb[1])];
        let t = mesh.tris[d.tri as usize];
        let c = [0, 1, 2].map(|k| mesh.pts[t[k] as usize]);
        let centre = [(c[0][0] + c[1][0] + c[2][0]) / 3.0, (c[0][1] + c[1][1] + c[2][1]) / 3.0];
        let (dx, dy) = ((pb[0] - pa[0]).abs(), (pb[1] - pa[1]).abs());
        let cat = match tag {
            C_PL1 => GroundCat::Plane1,
            C_PL2 => GroundCat::Plane2,
            C_F1 => GroundCat::Fence1,
            C_F2 => GroundCat::Fence2,
            C_BND => GroundCat::Boundary,
            // the flux around the tip of a zero-thickness coplanar ground faces the gap: the gap flank
            C_COP if d.side == 0 => GroundCat::CopFlank,
            C_COP if dy < dx => {
                if centre[1] > mid[1] {
                    GroundCat::CopTop
                } else {
                    GroundCat::CopBottom
                }
            }
            C_COP if centre[0].abs() < mid[0].abs() => GroundCat::CopFlank,
            C_COP => GroundCat::CopOuter,
            _ => continue,
        };
        // ground edges do not cross x = 0 unless they are planes, which are split there by symmetry of |x|
        let (xa, xb) = (pa[0].abs(), pb[0].abs());
        let low = if pa[0] * pb[0] < 0.0 { 0.0 } else { xa.min(xb) };
        let reach = (low, xa.max(xb));
        contacts.push(Contact { cat, x: mid[0], reach, len: dx.hypot(dy), flux_d: d.flux, flux_0: z.flux });
    }

    let sums = |pick: fn(&Contact) -> f64| {
        let mut sum = [0.0; 9];
        for c in &contacts {
            sum[c.cat.index()] += pick(c);
        }
        Shares::from_sums(sum)
    };
    let (field, _) = sums(|c| c.flux_d);
    let (current, tot) = sums(|c| c.flux_0);

    // distribution of the air-solution (return) current
    let spread = |cats: &[GroundCat]| -> Vec<((f64, f64), f64)> {
        contacts.iter().filter(|c| cats.contains(&c.cat)).map(|c| (c.reach, c.flux_0)).collect()
    };
    let x90_planes = x90_spread(&spread(&[GroundCat::Plane1, GroundCat::Plane2]));
    let x90_cop = x90_spread(&spread(&GroundCat::COPLANAR)).map_or(f64::NAN, |x| x - (p.w / 2.0 + p.s));

    let mut profiles = Vec::new();
    for cat in [GroundCat::Plane1, GroundCat::Plane2, GroundCat::CopTop, GroundCat::CopBottom] {
        let mut v: Vec<(f64, f64)> =
            contacts.iter().filter(|c| c.cat == cat).map(|c| (c.x, c.flux_0 / c.len / tot)).collect();
        if v.is_empty() {
            continue;
        }
        v.sort_by(|a, b| a.0.total_cmp(&b.0));
        let (x, js) = merge_equal_x(v);
        profiles.push(Profile { cat, x, js });
    }
    Ok(Split { field, current, x90_planes, x90_cop, profiles })
}

/// The |x| within which 90 % of the flux lies, with the flux of every edge spread evenly over the range of |x| the
/// edge covers (edges across the plane of the cross-section are long where little happens, so the centre of an
/// edge would be a coarse answer). `None` without flux.
fn x90_spread(parts: &[((f64, f64), f64)]) -> Option<f64> {
    let total: f64 = parts.iter().map(|p| p.1).sum();
    if parts.is_empty() || total.abs() <= 0.0 {
        return None;
    }
    let within = |x: f64| -> f64 {
        parts
            .iter()
            .map(|&((lo, hi), f)| {
                let part = if hi > lo { ((x - lo) / (hi - lo)).clamp(0.0, 1.0) } else { f64::from(x >= lo) };
                f * part
            })
            .sum()
    };
    let (mut low, mut high) = (0.0, parts.iter().map(|p| p.0.1).fold(0.0, f64::max));
    for _ in 0..60 {
        let mid = 0.5 * (low + high);
        if within(mid) < 0.9 * total { low = mid } else { high = mid }
    }
    Some(high)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn x90_interpolates_inside_the_edge_that_crosses_ninety_percent() {
        // two edges of equal flux, [0, 1] and [1, 5]: 90 % is reached 0.8 of the way along the second
        let x = x90_spread(&[((0.0, 1.0), 1.0), ((1.0, 5.0), 1.0)]).unwrap();
        assert!((x - 4.2).abs() < 1e-9, "{x}");
        // a face parallel to y covers a single |x|
        let x = x90_spread(&[((0.5, 0.5), 3.0), ((0.5, 1.5), 1.0)]).unwrap();
        assert!((x - 1.1).abs() < 1e-9, "{x}");
        assert!(x90_spread(&[]).is_none());
    }
}
