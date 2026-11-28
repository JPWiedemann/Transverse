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
    /// cannot move). Via fences and side walls are vertical and move by `d` as well.
    ///
    /// A surface moves along its normal, so an etched flank keeps its angle `a` to the wide face. With both
    /// faces and the flank moved by `d`, the wide edge moves sideways by `k d` with `k = cot(a / 2)`, which is
    /// 1 for a vertical flank: `w - 2 k d` and `s + 2 k d`. The undercut shrinks with the thickness.
    pub fn walls_receded(&self, d: f64) -> Self {
        let thick = self.t > 0.0;
        let plane = if thick { 2.0 * d } else { d };
        let t = if thick { (self.t - 2.0 * d).max(0.0) } else { 0.0 };
        let k = self.edge_shift_ratio();
        // a side wall is measured from the trace edge (which moves by -k d, the wall by +d), a fence on the
        // coplanar ground from the ground edge (which moves by +k d)
        let g = if self.has_walls() { self.g + d * (1.0 + k) } else { self.g - d * (k - 1.0) };
        Self {
            w: self.w - 2.0 * k * d,
            s: self.s + 2.0 * k * d,
            t,
            etch_delta: if thick { self.etch_delta * t / self.t } else { self.etch_delta },
            h1: if self.plane1 { self.h1 + plane } else { self.h1 },
            h2: if self.plane2 { self.h2 + plane } else { self.h2 },
            g,
            ..self.clone()
        }
    }

    /// `k` of [`Self::walls_receded`]: how far a wide copper edge moves sideways per distance the surfaces
    /// recede.
    fn edge_shift_ratio(&self) -> f64 {
        let u = self.undercut();
        if u > 0.0 { (self.t.hypot(u) + u) / self.t } else { 1.0 }
    }
}

/// The wall shift of the difference quotient [mm], relative to the geometry.
pub(super) fn wheeler_step(p: &PlanarParams) -> f64 {
    let mut dn = p.narrow_width() / 20.0;
    if p.coplanar {
        dn = dn.min(p.s / 20.0);
    }
    if p.t > 0.0 {
        dn = dn.min(p.t / 8.0);
    }
    // with flat flanks the edges move further than the surfaces
    dn / p.edge_shift_ratio()
}

/// dZ0_air/dn [Ohm/m] with the same mesh settings and domain as `sol`.
pub fn wheeler_dzdn(p: &PlanarParams, sol: &Solution, ctl: &Ctl) -> TvResult<f64> {
    let dn = wheeler_step(p);
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

    /// The corners of the trace and of the right coplanar ground, wide edge first, with the bottom of the
    /// copper at `y0`.
    fn flank_corners(p: &PlanarParams, y0: f64) -> [[[f64; 2]; 2]; 2] {
        let stack = p.stackup();
        let layer = &stack.layers[0];
        let corner = |f: f64, x: f64| {
            let i = layer.inset[0] + f * (layer.inset[1] - layer.inset[0]);
            [x, i, y0 + f * p.t]
        };
        let wide = if layer.inset[0] == 0.0 { 0.0 } else { 1.0 };
        let [trace, ground] = [(p.w / 2.0, -1.0), (p.w / 2.0 + p.s, 1.0)].map(|(x, sign)| {
            [wide, 1.0 - wide].map(|f| {
                let [x, i, y] = corner(f, x);
                [x + sign * i, y]
            })
        });
        [trace, ground]
    }

    #[test]
    fn etched_flanks_recede_along_their_normals() {
        use crate::planar::{EtchMode, WideSide};
        let d = 1e-4;
        for (etch, etch_wide) in [
            (EtchMode::Factor, WideSide::Bottom),
            (EtchMode::Factor, WideSide::Top),
            (EtchMode::Delta, WideSide::Bottom),
            (EtchMode::Delta, WideSide::Top),
        ] {
            // flat flanks (30 deg to 60 deg) make the difference to the vertical rule large
            let p =
                PlanarParams { etch, etch_wide, etch_factor: 0.7, etch_delta: 0.05, ..Default::default() };
            let q = p.walls_receded(d);
            assert!((q.flank_angle_deg() - p.flank_angle_deg()).abs() < 1e-9, "the angle stays");
            let (before, after) = (flank_corners(&p, 0.0), flank_corners(&q, d));
            for (k, into_metal) in [(0, -1.0), (1, 1.0)] {
                let [a, b] = before[k];
                let len = (b[0] - a[0]).hypot(b[1] - a[1]);
                let mut n = [(b[1] - a[1]) / len, -(b[0] - a[0]) / len];
                if n[0] * into_metal < 0.0 {
                    n = [-n[0], -n[1]];
                }
                for c in after[k] {
                    let moved = (c[0] - a[0]) * n[0] + (c[1] - a[1]) * n[1];
                    assert!((moved - d).abs() < 1e-12, "{etch:?} {etch_wide:?} piece {k}: {moved}");
                }
            }
            // the top and the bottom face move by d towards each other
            assert!((q.t - (p.t - 2.0 * d)).abs() < 1e-15);
        }
    }

    #[test]
    fn walls_next_to_etched_copper_move_by_d() {
        use crate::planar::EtchMode;
        let d = 1e-4;
        let base = PlanarParams { etch: EtchMode::Factor, etch_factor: 0.7, ..Default::default() };
        let q = base.walls_receded(d);
        assert!((q.wall_x() - (base.wall_x() + d)).abs() < 1e-15, "fence on the coplanar ground");
        let p = PlanarParams { coplanar: false, side_walls: true, ..base };
        let q = p.walls_receded(d);
        assert!((q.wall_x() - (p.wall_x() + d)).abs() < 1e-15, "side wall");
    }

    #[test]
    fn a_missing_plane_has_no_surface_to_move() {
        let p = PlanarParams { plane1: false, fence1: false, ..PlanarParams::default() };
        let q = p.walls_receded(1e-3);
        assert_eq!((q.h1, q.h2), (p.h1, p.h2 + 2e-3));
    }

    #[test]
    fn a_side_wall_recedes_away_from_the_trace() {
        let d = 1e-3;
        for t in [0.018, 0.0] {
            let p = PlanarParams { coplanar: false, side_walls: true, t, ..PlanarParams::default() };
            let q = p.walls_receded(d);
            // the trace edge goes inwards, the wall outwards, in absolute positions
            assert!((q.w / 2.0 - (p.w / 2.0 - d)).abs() < 1e-15);
            assert!((q.wall_x() - (p.wall_x() + d)).abs() < 1e-15, "{} {}", q.wall_x(), p.wall_x());
        }
        // fences on a coplanar ground keep g: they recede together with the gap edge
        let p = PlanarParams::default();
        let q = p.walls_receded(d);
        assert!((q.g - p.g).abs() < 1e-15 && (q.wall_x() - (p.wall_x() + d)).abs() < 1e-15);
    }
}
