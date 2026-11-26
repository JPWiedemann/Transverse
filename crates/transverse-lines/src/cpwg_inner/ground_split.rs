//! Split of the electric flux and of the HF return current over the ground conductors.
//!
//! Field: dielectric solution (where the field lines end). Current: air solution, where the
//! surface charge equals the HF surface current (valid when the skin depth is much smaller than t).

use serde::{Deserialize, Serialize};
use transverse_core::fd2d::CAT_SIGNAL;
use transverse_core::quasistatic::Solution;

use super::{C_BND, C_COP, C_F1, C_F2, C_PL1, C_PL2, CpwgParams};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum GroundCat {
    Plane1,
    Plane2,
    CopFlank,
    CopTop,
    CopBottom,
    CopOuter,
    Fence1,
    Fence2,
    Boundary,
}

impl GroundCat {
    pub const ALL: [GroundCat; 9] = [
        Self::Plane1,
        Self::Plane2,
        Self::CopFlank,
        Self::CopTop,
        Self::CopBottom,
        Self::CopOuter,
        Self::Fence1,
        Self::Fence2,
        Self::Boundary,
    ];
    pub const COPLANAR: [GroundCat; 4] = [Self::CopFlank, Self::CopTop, Self::CopBottom, Self::CopOuter];

    pub fn label(self) -> &'static str {
        match self {
            Self::Plane1 => "Ground plane 1 (top)",
            Self::Plane2 => "Ground plane 2 (bottom)",
            Self::CopFlank => "gap flank",
            Self::CopTop => "top face",
            Self::CopBottom => "bottom face",
            Self::CopOuter => "outer edge",
            Self::Fence1 => "Via fence 1 (to top)",
            Self::Fence2 => "Via fence 2 (to bottom)",
            Self::Boundary => "Domain boundary (far field)",
        }
    }

    /// Stable identifier (used in reference data and serialised results).
    pub fn key(self) -> &'static str {
        match self {
            Self::Plane1 => "plane1",
            Self::Plane2 => "plane2",
            Self::CopFlank => "cop_flank",
            Self::CopTop => "cop_top",
            Self::CopBottom => "cop_bottom",
            Self::CopOuter => "cop_outer",
            Self::Fence1 => "fence1",
            Self::Fence2 => "fence2",
            Self::Boundary => "boundary",
        }
    }

    pub(super) fn index(self) -> usize {
        Self::ALL.iter().position(|&c| c == self).unwrap()
    }
}

/// Shares in percent of the total flux into all ground conductors.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct Shares {
    pct: [f64; 9],
    /// Sum of all coplanar-ground faces.
    pub coplanar: f64,
}

impl Shares {
    /// Shares in percent of the sum of all fluxes, from the flux sums per category (in the order of [`GroundCat::ALL`]).
    pub(super) fn from_sums(sum: [f64; 9]) -> (Shares, f64) {
        let tot: f64 = sum.iter().sum();
        let mut s = Shares::default();
        for (i, v) in sum.iter().enumerate() {
            s.pct[i] = 100.0 * v / tot;
        }
        s.coplanar = GroundCat::COPLANAR.iter().map(|&c| s.get(c)).sum();
        (s, tot)
    }

    pub fn get(&self, c: GroundCat) -> f64 {
        self.pct[c.index()]
    }
}

/// Surface-current density profile along x on one conductor face, J_s / I [1/mm].
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Profile {
    pub cat: GroundCat,
    pub x: Vec<f64>,
    pub js: Vec<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Split {
    pub field: Shares,
    pub current: Shares,
    /// |x| [mm] containing 90 % of the plane current (None without planes).
    pub x90_planes: Option<f64>,
    /// Distance [mm] from the gap edge containing 90 % of the coplanar current.
    pub x90_cop: f64,
    pub profiles: Vec<Profile>,
}

/// One edge between a grounded conductor node and a free node.
struct Contact {
    cat: GroundCat,
    edge: usize,
    cond: usize,
    free: usize,
    /// Length of the conductor boundary segment this edge represents [mm].
    dual: f64,
    x: f64,
}

fn dual_lengths(v: &[f64]) -> Vec<f64> {
    let mut d = vec![0.0; v.len()];
    for (k, w) in v.windows(2).enumerate() {
        d[k] += (w[1] - w[0]) / 2.0;
        d[k + 1] += (w[1] - w[0]) / 2.0;
    }
    d
}

/// First entry where the cumulative share reaches 90 % (numpy `searchsorted` semantics).
pub(super) fn x90(mut pts: Vec<(f64, f64)>) -> Option<f64> {
    if pts.is_empty() {
        return None;
    }
    pts.sort_by(|a, b| a.0.total_cmp(&b.0));
    let total: f64 = pts.iter().map(|p| p.1).sum();
    let mut cum = 0.0;
    for &(x, f) in &pts {
        cum += f;
        if cum / total >= 0.9 {
            return Some(x);
        }
    }
    Some(pts.last().unwrap().0)
}

/// Points sorted by x with the values of equal x added up.
pub(super) fn merge_equal_x(v: Vec<(f64, f64)>) -> (Vec<f64>, Vec<f64>) {
    let (mut x, mut js): (Vec<f64>, Vec<f64>) = (Vec::new(), Vec::new());
    for (xx, j) in v {
        if x.last() == Some(&xx) {
            *js.last_mut().unwrap() += j;
        } else {
            x.push(xx);
            js.push(j);
        }
    }
    (x, js)
}

pub fn ground_split(p: &CpwgParams, sol: &Solution) -> Split {
    let g = &sol.grid;
    let (nx, ny) = (g.nx(), g.ny());
    let fixed = |k: usize| g.cat[k] > 0;
    let (dxn, dyn_) = (dual_lengths(&g.x), dual_lengths(&g.y));

    let mut contacts = Vec::new();
    for (e, &[a, b]) in sol.edges.iter().enumerate() {
        let (a, b) = (a as usize, b as usize);
        if fixed(a) == fixed(b) {
            continue;
        }
        let (cond, free) = if fixed(a) { (a, b) } else { (b, a) };
        let (cj, ci, fj, fi) = (cond / nx, cond % nx, free / nx, free % nx);
        let cat = match g.cat[cond] {
            CAT_SIGNAL => continue,
            C_PL1 => GroundCat::Plane1,
            C_PL2 => GroundCat::Plane2,
            C_F1 => GroundCat::Fence1,
            C_F2 => GroundCat::Fence2,
            C_BND => GroundCat::Boundary,
            C_COP if fj > cj => GroundCat::CopTop,
            C_COP if fj < cj => GroundCat::CopBottom,
            C_COP if g.x[fi].abs() < g.x[ci].abs() => GroundCat::CopFlank,
            C_COP => GroundCat::CopOuter,
            _ => continue,
        };
        let dual = if fi == ci { dxn[ci] } else { dyn_[cj] };
        contacts.push(Contact { cat, edge: e, cond, free, dual, x: g.x[ci] });
    }
    debug_assert!(ny > 0);

    let flux = |v: &[f64], coef: &[f64], c: &Contact| coef[c.edge] * (v[c.free] - v[c.cond]);
    let shares = |v: &[f64], coef: &[f64]| -> (Shares, f64) {
        let mut sum = [0.0; 9];
        let mut tot = 0.0;
        for c in &contacts {
            let f = flux(v, coef, c);
            sum[c.cat.index()] += f;
            tot += f;
        }
        let mut s = Shares::default();
        for (i, v) in sum.iter().enumerate() {
            s.pct[i] = 100.0 * v / tot;
        }
        s.coplanar = GroundCat::COPLANAR.iter().map(|&c| s.get(c)).sum();
        (s, tot)
    };
    let (field, _) = shares(&sol.v, &sol.coef_d);
    let (current, tot) = shares(&sol.v0, &sol.coef_0);

    // distribution of the air-solution (return) current
    let cur: Vec<(&Contact, f64)> = contacts.iter().map(|c| (c, flux(&sol.v0, &sol.coef_0, c))).collect();
    let pts = |cats: &[GroundCat]| -> Vec<(f64, f64)> {
        cur.iter().filter(|(c, _)| cats.contains(&c.cat)).map(|(c, f)| (c.x.abs(), *f)).collect()
    };
    let x90_planes = x90(pts(&[GroundCat::Plane1, GroundCat::Plane2]));
    let x90_cop = x90(pts(&GroundCat::COPLANAR)).map_or(f64::NAN, |x| x - (p.w / 2.0 + p.s));

    let mut profiles = Vec::new();
    for cat in [GroundCat::Plane1, GroundCat::Plane2, GroundCat::CopTop, GroundCat::CopBottom] {
        let mut v: Vec<(f64, f64)> =
            cur.iter().filter(|(c, _)| c.cat == cat).map(|(c, f)| (c.x, f / c.dual / tot)).collect();
        if v.is_empty() {
            continue;
        }
        v.sort_by(|a, b| a.0.total_cmp(&b.0));
        let (x, js) = merge_equal_x(v);
        profiles.push(Profile { cat, x, js });
    }
    Split { field, current, x90_planes, x90_cop, profiles }
}
