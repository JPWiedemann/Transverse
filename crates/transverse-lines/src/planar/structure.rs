//! The switches of the structure and what they mean: which conductors exist, where the side walls stand, how
//! far the region of interest reaches.

use super::{EtchMode, PlanarParams, Signals};

impl PlanarParams {
    /// The parameters as the solvers see them, with the switches resolved:
    ///
    /// * `upper_same`: the upper dielectric has the permittivity and loss tangent of the lower one.
    /// * no upper half: air above the copper, without ground plane 1 and via fence 1. The height `h1` then has
    ///   no physical meaning; it only steers mesh and domain sizes and takes the value of `h2`.
    /// * copper without thickness has no flanks to etch.
    /// * via fences stand on the coplanar ground; without it there are only the side walls from plane to plane.
    ///
    /// Applying it again changes nothing.
    pub fn effective(&self) -> Self {
        let mut e = self.clone();
        if e.upper_same {
            (e.er1, e.tand1) = (e.er2, e.tand2);
        }
        if !e.upper {
            (e.er1, e.tand1) = (1.0, 0.0);
            e.h1 = e.h2;
            (e.plane1, e.fence1) = (false, false);
        }
        if e.t.is_nan() || e.t <= 0.0 {
            e.etch = EtchMode::Rect;
        }
        if e.coplanar {
            e.side_walls = false;
        } else {
            (e.fence1, e.fence2) = (false, false);
            e.side_walls &= e.plane1 && e.plane2;
        }
        e
    }

    /// How far each etched flank leans [mm]: the horizontal distance between its bottom and top edge, by which
    /// the narrow face of a trace is set back on each side. 0 for vertical flanks.
    pub fn undercut(&self) -> f64 {
        let e = self.effective();
        match e.etch {
            EtchMode::Rect => 0.0,
            EtchMode::Factor => e.t / e.etch_factor,
            EtchMode::Delta => e.etch_delta / 2.0,
        }
    }

    /// Width of the narrow face of the trace [mm] (`w` for vertical flanks).
    pub fn narrow_width(&self) -> f64 {
        self.w - 2.0 * self.undercut()
    }

    /// Angle between an etched flank and the wide face [deg]: 90 for vertical flanks.
    pub fn flank_angle_deg(&self) -> f64 {
        let u = self.undercut();
        if u > 0.0 { self.t.atan2(u).to_degrees() } else { 90.0 }
    }

    pub fn pair(&self) -> bool {
        self.signals == Signals::Pair
    }

    /// Width of the copper from the outer edge of one trace to the outer edge of the other (`w` for one trace),
    /// at the wide face.
    pub fn trace_span(&self) -> f64 {
        if self.pair() { 2.0 * self.w + self.d } else { self.w }
    }

    /// |x| of the outermost trace edge.
    pub fn trace_half_width(&self) -> f64 {
        self.trace_span() / 2.0
    }

    /// Side walls from plane to plane, `g` outside the trace edges (only without a coplanar ground).
    pub fn has_walls(&self) -> bool {
        self.effective().side_walls
    }

    /// Fully enclosed by planes and fences or side walls.
    pub fn closed(&self) -> bool {
        let e = self.effective();
        e.plane1 && e.plane2 && if e.coplanar { e.fence1 && e.fence2 } else { e.side_walls }
    }

    /// Outer edge of the conductors of the copper layer next to the traces: the gap edge of the coplanar ground,
    /// or the outer trace edge itself without one.
    pub fn inner_half_width(&self) -> f64 {
        self.trace_half_width() + self.gap_or_zero()
    }

    /// The gap to the coplanar ground, 0 without one (for sizes that grow with it).
    pub fn gap_or_zero(&self) -> f64 {
        if self.coplanar { self.s } else { 0.0 }
    }

    /// |x| where via fences or side walls stand, `g` outside [`Self::inner_half_width`].
    pub fn wall_x(&self) -> f64 {
        self.inner_half_width() + self.g
    }

    /// Half width of the region of interest, from which an open structure is extended by the margin: up to the
    /// fences; without coplanar ground and walls a region around the trace that holds most of its field.
    pub fn core_half_width(&self) -> f64 {
        let e = self.effective();
        if e.coplanar && !(e.fence1 || e.fence2) {
            // without fences g only sizes this region, which has to hold the whole etched edge of the ground
            e.inner_half_width() + e.g.max(2.0 * e.undercut())
        } else if e.coplanar || e.side_walls {
            e.wall_x()
        } else {
            e.trace_half_width() + e.w.max(2.0 * e.h1.max(e.h2))
        }
    }

    /// Short name of the structure, for reports.
    pub fn structure_name(&self) -> &'static str {
        let e = self.effective();
        match (e.coplanar, e.upper, e.plane1, e.plane2) {
            (true, _, false, false) => "coplanar waveguide",
            (true, _, _, _) => "grounded coplanar waveguide",
            (false, false, _, _) => "microstrip",
            (false, true, true, true) => "stripline",
            (false, true, _, _) => "buried microstrip",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effective_resolves_the_switches_and_is_idempotent() {
        let base = PlanarParams {
            er1: 3.0,
            er2: 4.4,
            tand1: 0.01,
            tand2: 0.03,
            h1: 0.1,
            h2: 0.3,
            ..Default::default()
        };
        // the defaults change nothing
        assert_eq!(base.effective(), base);
        let same = PlanarParams { upper_same: true, ..base.clone() }.effective();
        assert_eq!((same.er1, same.tand1, same.h1), (4.4, 0.03, 0.1));
        let open = PlanarParams { upper: false, upper_same: true, ..base.clone() }.effective();
        assert_eq!((open.er1, open.tand1, open.h1, open.plane1, open.fence1), (1.0, 0.0, 0.3, false, false));
        assert_eq!(open.gap_props(), (1.0, 0.0), "the gap of the upper material is air then");
        for (plane1, walls) in [(true, true), (false, false)] {
            let p = PlanarParams { coplanar: false, side_walls: true, plane1, ..base.clone() };
            let e = p.effective();
            assert_eq!((e.fence1, e.fence2, e.side_walls), (false, false, walls));
            assert_eq!((p.closed(), p.has_walls()), (walls, walls));
            assert_eq!(e.effective(), e);
        }
        // side walls mean nothing next to a coplanar ground
        assert!(!PlanarParams { side_walls: true, ..base }.has_walls());
    }

    #[test]
    fn widths_follow_the_switches_and_names_the_structure() {
        let p = PlanarParams::default();
        assert_eq!(p.core_half_width(), p.w / 2.0 + p.s + p.g);
        let walls = PlanarParams { coplanar: false, side_walls: true, ..p.clone() };
        assert_eq!(walls.core_half_width(), p.w / 2.0 + p.g);
        let open = PlanarParams { coplanar: false, ..p.clone() };
        assert_eq!(open.core_half_width(), p.w / 2.0 + p.w.max(2.0 * p.h2));
        assert_eq!(p.structure_name(), "grounded coplanar waveguide");
        assert_eq!(open.structure_name(), "stripline");
        assert_eq!(PlanarParams { upper: false, ..open.clone() }.structure_name(), "microstrip");
        assert_eq!(PlanarParams { plane1: false, ..open }.structure_name(), "buried microstrip");
        let cpw = PlanarParams { plane1: false, plane2: false, ..p };
        assert_eq!(cpw.structure_name(), "coplanar waveguide");
    }
}
