//! The CPWG cross-section as generic geometry for the mesher (the FEM counterpart of `grid.rs`).
//!
//! Same domain, same layers and same conductor pieces as the tensor grid: the grounded box around the
//! structure (closed by planes and fences, or enlarged by the margin where they are missing), dielectric
//! layers across the whole width, via fences at |x| = w/2 + s + g, planes across the whole width, the
//! coplanar ground from the gap to the domain edge, and the signal strip. Zero-thickness metal (planes,
//! fences, and the strip and ground for t = 0) are segments, copper of thickness t is rectangles.

use transverse_core::quasistatic::GridSource;
use transverse_geom::{CrossSection, Far, SizeSpec};

use super::PlanarParams;

/// Grading of the size field away from conductor edges.
pub const GROWTH: f64 = 1.25;
/// Grading exponent towards conductor edges (1 = linear).
pub const TIP_POWER: f64 = 1.0;

impl PlanarParams {
    /// The cross-section for a domain extension `margin` of the open regions.
    pub fn cross_section(&self, margin: f64) -> CrossSection {
        let g = self.geometry(margin);
        self.stackup().cross_section([-g.xd, g.xd, g.ymin, g.ymax])
    }

    /// Element sizes of the first mesh level: the sizes of the grid's first level, coarse cells in the open
    /// space around the board.
    pub fn size_spec(&self, margin: f64) -> SizeSpec {
        let (dmin, dmax) = self.base_sizes();
        self.size_spec_for(margin, dmin, dmax)
    }

    /// As [`Self::size_spec`] with given sizes (manual mesh).
    pub fn size_spec_for(&self, margin: f64, dmin: f64, dmax: f64) -> SizeSpec {
        let g = self.geometry(margin);
        let far = (!self.closed())
            .then(|| Far { core: [-g.xe, g.xe, g.ybot, g.ytop], dfar: (margin / 6.0).max(dmax) });
        SizeSpec { dmin, dmax, growth: GROWTH, power: TIP_POWER, far }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::planar::{C_BND, C_F1, C_PL1, C_SIG, MAT_GAP};
    use transverse_geom::{Paint, SIGNAL, Shape};

    #[test]
    fn the_default_stackup_is_a_valid_closed_section_with_all_conductor_pieces() {
        let p = PlanarParams::default();
        let cs = p.cross_section(0.0);
        cs.validate().unwrap();
        let g = p.geometry(0.0);
        assert_eq!(cs.domain, [-g.xe, g.xe, g.ybot, g.ytop]);
        // 2 dielectric layers (the gap material is that of the upper one: one layer), 4 fence pieces, 2 planes,
        // 2 coplanar ground rectangles, the signal strip
        assert_eq!(cs.items.len(), 2 + 4 + 2 + 2 + 1);
        assert_eq!(cs.materials[usize::from(MAT_GAP)].er, p.er1);
        assert!(cs.box_grounded && cs.box_tag == C_BND);
        assert!(cs.items.iter().any(|it| it.paint == Paint::Conductor { id: SIGNAL, tag: C_SIG }));
    }

    #[test]
    fn missing_planes_and_fences_open_the_domain_and_t_zero_uses_segments() {
        let p = PlanarParams { plane1: false, fence1: false, t: 0.0, ..PlanarParams::default() };
        let cs = p.cross_section(3.0);
        cs.validate().unwrap();
        let g = p.geometry(3.0);
        assert_eq!(cs.domain, [-g.xd, g.xd, g.ybot, g.ytop + 3.0]);
        // no gap layer, no thick copper
        assert_eq!(cs.items.iter().filter(|it| matches!(it.paint, Paint::Dielectric(_))).count(), 2);
        let signal =
            cs.items.iter().find(|it| it.paint == Paint::Conductor { id: SIGNAL, tag: C_SIG }).unwrap();
        assert!(matches!(signal.shape, Shape::Segment { .. }));
        // no plane 1, no fence 1: those pieces are absent
        assert!(
            !cs.items
                .iter()
                .any(|it| matches!(it.paint, Paint::Conductor { tag, .. } if tag == C_PL1 || tag == C_F1))
        );
        // far field only where the structure is open
        assert!(p.size_spec(3.0).far.is_some());
        assert!(PlanarParams::default().size_spec(0.0).far.is_none());
    }

    #[test]
    fn the_copper_layer_is_a_dielectric_of_its_own_only_with_a_material_of_its_own() {
        use crate::planar::GapMode;
        let layers = |p: &PlanarParams| -> Vec<(f64, f64)> {
            p.cross_section(1.0)
                .items
                .iter()
                .filter_map(|it| match (&it.paint, &it.shape) {
                    (Paint::Dielectric(_), Shape::Rect { y0, y1, .. }) => Some((*y0, *y1)),
                    _ => None,
                })
                .collect()
        };
        let p = PlanarParams::default();
        let (t, h1, h2) = (p.t, p.h1, p.h2);
        assert_eq!(layers(&p), [(-h2, 0.0), (0.0, t + h1)]);
        let lower = PlanarParams { gap_mode: GapMode::Lower, ..p.clone() };
        assert_eq!(layers(&lower), [(-h2, t), (t, t + h1)]);
        let custom = PlanarParams { gap_mode: GapMode::Custom, er_gap: 2.0, ..p };
        assert_eq!(layers(&custom), [(-h2, 0.0), (t, t + h1), (0.0, t)]);
    }

    #[test]
    fn the_first_level_uses_the_grid_base_sizes() {
        let p = PlanarParams::default();
        let (dmin, dmax) = p.base_sizes();
        let s = p.size_spec(0.0);
        assert_eq!((s.dmin, s.dmax, s.growth), (dmin, dmax, GROWTH));
        s.validate().unwrap();
    }
}
