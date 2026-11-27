//! Conductor loss by Wheeler's incremental-inductance rule.
//!
//! All metal walls are moved by +dn / -dn in the air solution; dZ0/dn gives the loss.

use transverse_core::quasistatic::Solution;
use transverse_core::{Ctl, TvResult};

use super::{GapMode, MeshMode, PlanarParams, analyze};

impl PlanarParams {
    /// The structure with every metal surface moved by `d` into its metal (`d < 0`: out of it), as new
    /// dimensions. The heights h1 and h2 are measured from the copper surfaces, which move as well, so a plane
    /// that recedes gets `2 d` further from a thick copper layer (`d` from a zero-thickness one, whose faces
    /// cannot move). The via fences keep their distance g: they recede with the coplanar-ground edge.
    pub fn walls_receded(&self, d: f64) -> Self {
        let thick = self.t > 0.0;
        let plane = if thick { 2.0 * d } else { d };
        Self {
            w: self.w - 2.0 * d,
            s: self.s + 2.0 * d,
            t: if thick { (self.t - 2.0 * d).max(0.0) } else { 0.0 },
            h1: if self.plane1 { self.h1 + plane } else { self.h1 },
            h2: if self.plane2 { self.h2 + plane } else { self.h2 },
            ..self.clone()
        }
    }
}

/// dZ0_air/dn [Ohm/m] with the same mesh settings and domain as `sol`.
pub fn wheeler_dzdn(p: &PlanarParams, sol: &Solution, ctl: &Ctl) -> TvResult<f64> {
    // wall shift relative to the geometry
    let mut dn = (p.s / 20.0).min(p.w / 20.0);
    if p.t > 0.0 {
        dn = dn.min(p.t / 8.0);
    }
    let (dmin, dmax, richardson) = sol.accuracy.mesh_used;
    let pa = PlanarParams {
        er1: 1.0,
        er2: 1.0,
        gap_mode: GapMode::Custom,
        er_gap: 1.0,
        mesh_mode: MeshMode::Manual,
        dmin,
        dmax,
        richardson,
        margin: if sol.accuracy.margin > 0.0 { sol.accuracy.margin } else { 1.0 },
        ..p.clone()
    };
    let z = |d: f64| -> TvResult<f64> {
        ctl.msg(&format!("Conductor loss (Wheeler): wall shift {:+.2} um ...", d * 1e3));
        Ok(analyze(&pa.walls_receded(d), ctl)?.z0)
    };
    Ok((z(dn)? - z(-dn)?) / (2.0 * dn * 1e-3))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Where the metal surfaces of `p` are [mm], with `y0` the height of the bottom of the signal layer. In the
    /// order: signal side face, coplanar face at the gap, via fence, copper bottom, copper top, plane 1, plane 2.
    fn surfaces(p: &PlanarParams, y0: f64) -> [f64; 7] {
        let g = p.geometry(0.0);
        [p.w / 2.0, p.w / 2.0 + p.s, g.xe, y0, y0 + p.t, y0 + g.ytop, y0 + g.ybot]
    }

    /// Every metal surface moves by `d` into its own metal: the direction (+x or +y) in which it goes.
    #[test]
    fn every_metal_surface_recedes_into_its_metal() {
        let d = 1e-3;
        for (t, faces) in [(0.018, 1.0), (0.035, 1.0), (0.0, 0.0)] {
            let p = PlanarParams { t, ..PlanarParams::default() };
            let q = p.walls_receded(d);
            // the bottom face of thick copper recedes upwards, and the dimensions of `q` are measured from it
            let (before, after) = (surfaces(&p, 0.0), surfaces(&q, faces * d));
            let direction = [-1.0, 1.0, 1.0, faces, -faces, 1.0, -1.0];
            for k in 0..7 {
                assert!(
                    (after[k] - before[k] - direction[k] * d).abs() < 1e-12,
                    "t = {t}: surface {k} moved by {} instead of {}",
                    after[k] - before[k],
                    direction[k] * d
                );
            }
        }
    }

    #[test]
    fn a_missing_plane_has_no_surface_to_move() {
        let p = PlanarParams { plane1: false, fence1: false, ..PlanarParams::default() };
        let q = p.walls_receded(1e-3);
        assert_eq!((q.h1, q.h2), (p.h1, p.h2 + 2e-3));
    }
}
