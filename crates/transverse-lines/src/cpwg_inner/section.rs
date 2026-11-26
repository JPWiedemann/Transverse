//! The CPWG cross-section as generic geometry for the mesher (the FEM counterpart of `grid.rs`).
//!
//! Same domain, same layers and same conductor pieces as the tensor grid: the grounded box around the
//! structure (closed by planes and fences, or enlarged by the margin where they are missing), dielectric
//! layers across the whole width, via fences at |x| = w/2 + s + g, planes across the whole width, the
//! coplanar ground from the gap to the domain edge, and the signal strip. Zero-thickness metal (planes,
//! fences, and the strip and ground for t = 0) are segments, copper of thickness t is rectangles.

use transverse_core::quasistatic::GridSource;
use transverse_geom::{CrossSection, Far, GROUND, Item, Material, Paint, SIGNAL, Shape, SizeSpec};

use super::CpwgParams;
use super::grid::{C_BND, C_COP, C_F1, C_F2, C_PL1, C_PL2, C_SIG};

pub const MAT_AIR: u16 = 0;
pub const MAT_UPPER: u16 = 1;
pub const MAT_LOWER: u16 = 2;
pub const MAT_GAP: u16 = 3;

/// Grading of the size field away from conductor edges.
pub const GROWTH: f64 = 1.25;
/// Grading exponent towards conductor edges (1 = linear).
pub const TIP_POWER: f64 = 1.0;

impl CpwgParams {
    /// The cross-section for a domain extension `margin` of the open regions.
    pub fn cross_section(&self, margin: f64) -> CrossSection {
        let g = self.geometry(margin);
        let t = self.t;
        let xd = g.xd;
        let (er_g, td_g) = self.gap_props();
        let materials = vec![
            Material { er: 1.0, tand: 0.0 },
            Material { er: self.er1, tand: self.tand1 },
            Material { er: self.er2, tand: self.tand2 },
            Material { er: er_g, tand: td_g },
        ];
        let diel = |shape, m| Item { shape, paint: Paint::Dielectric(m) };
        let cond = |shape, id, tag| Item { shape, paint: Paint::Conductor { id, tag } };
        let mut items = vec![
            diel(Shape::rect(-xd, g.ybot, xd, 0.0), MAT_LOWER),
            diel(Shape::rect(-xd, t, xd, g.ytop), MAT_UPPER),
        ];
        if t > 0.0 {
            items.push(diel(Shape::rect(-xd, 0.0, xd, t), MAT_GAP));
        }
        // painting order = precedence where pieces coincide, as in the node categories of the grid
        for sx in [-g.xe, g.xe] {
            if self.fence1 {
                items.push(cond(Shape::segment([sx, t], [sx, g.ytop]), GROUND, C_F1));
            }
            if self.fence2 {
                items.push(cond(Shape::segment([sx, g.ybot], [sx, 0.0]), GROUND, C_F2));
            }
        }
        if self.plane1 {
            items.push(cond(Shape::segment([-xd, g.ytop], [xd, g.ytop]), GROUND, C_PL1));
        }
        if self.plane2 {
            items.push(cond(Shape::segment([-xd, g.ybot], [xd, g.ybot]), GROUND, C_PL2));
        }
        let (a, h) = (self.w / 2.0 + self.s, self.w / 2.0);
        if t > 0.0 {
            items.push(cond(Shape::rect(a, 0.0, xd, t), GROUND, C_COP));
            items.push(cond(Shape::rect(-xd, 0.0, -a, t), GROUND, C_COP));
            items.push(cond(Shape::rect(-h, 0.0, h, t), SIGNAL, C_SIG));
        } else {
            items.push(cond(Shape::segment([a, 0.0], [xd, 0.0]), GROUND, C_COP));
            items.push(cond(Shape::segment([-xd, 0.0], [-a, 0.0]), GROUND, C_COP));
            items.push(cond(Shape::segment([-h, 0.0], [h, 0.0]), SIGNAL, C_SIG));
        }
        CrossSection {
            domain: [-xd, xd, g.ymin, g.ymax],
            background: MAT_AIR,
            materials,
            items,
            box_grounded: true,
            box_tag: C_BND,
        }
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
    use transverse_geom::Shape;

    #[test]
    fn the_default_stackup_is_a_valid_closed_section_with_all_conductor_pieces() {
        let p = CpwgParams::default();
        let cs = p.cross_section(0.0);
        cs.validate().unwrap();
        let g = p.geometry(0.0);
        assert_eq!(cs.domain, [-g.xe, g.xe, g.ybot, g.ytop]);
        // 3 dielectric layers, 4 fence pieces, 2 planes, 2 coplanar ground rectangles, the signal strip
        assert_eq!(cs.items.len(), 3 + 4 + 2 + 2 + 1);
        assert_eq!(cs.materials[usize::from(MAT_GAP)].er, p.er1);
        assert!(cs.box_grounded && cs.box_tag == C_BND);
        assert!(cs.items.iter().any(|it| it.paint == Paint::Conductor { id: SIGNAL, tag: C_SIG }));
    }

    #[test]
    fn missing_planes_and_fences_open_the_domain_and_t_zero_uses_segments() {
        let p = CpwgParams { plane1: false, fence1: false, t: 0.0, ..CpwgParams::default() };
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
        assert!(CpwgParams::default().size_spec(0.0).far.is_none());
    }

    #[test]
    fn the_first_level_uses_the_grid_base_sizes() {
        let p = CpwgParams::default();
        let (dmin, dmax) = p.base_sizes();
        let s = p.size_spec(0.0);
        assert_eq!((s.dmin, s.dmax, s.growth), (dmin, dmax, GROWTH));
        s.validate().unwrap();
    }
}
