//! The structure as a stack-up: dielectric layers, copper layers with their traces and coplanar ground,
//! ground planes and side walls. The mesher's cross-section, the field drawing and the moving mesh of the
//! conductor loss are made from it. One trace in one copper layer today; further traces and copper layers
//! (differential pairs) are further entries.
//!
//! y = 0 is the bottom of the (lowest) copper layer, x = 0 the middle of the structure. All lengths in mm.

use transverse_geom::{
    ConductorId, CrossSection, GROUND, Item, Material, MaterialId, Paint, SIGNAL, Shape, Tag,
};

use super::{C_BND, C_COP, C_F1, C_F2, C_PL1, C_PL2, C_SIG, PlanarParams, WideSide};

/// Materials of a cross-section, by index.
pub const MAT_AIR: MaterialId = 0;
pub const MAT_UPPER: MaterialId = 1;
pub const MAT_LOWER: MaterialId = 2;
pub const MAT_GAP: MaterialId = 3;

/// A dielectric layer across the whole width.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Slab {
    pub y0: f64,
    pub y1: f64,
    pub material: MaterialId,
}

/// A signal conductor in a copper layer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Trace {
    pub x0: f64,
    pub x1: f64,
    pub signal: ConductorId,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CopperLayer {
    /// Bottom of the layer and its thickness (0: conductors without thickness).
    pub y0: f64,
    pub t: f64,
    /// Etched flanks: how far the bottom and the top face of every conductor are set back from the positions
    /// below, which are those of the wide face. A trace gets narrower, a coplanar ground recedes from it.
    pub inset: [f64; 2],
    /// Material beside the conductors.
    pub fill: MaterialId,
    pub traces: Vec<Trace>,
    /// Coplanar ground: its edges facing the traces, left and right. It reaches to the domain edges.
    pub ground: Option<[f64; 2]>,
}

/// Walls without thickness at `|x| = x`, on both sides, from `y0` to `y1`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WallSpan {
    pub y0: f64,
    pub y1: f64,
    pub tag: Tag,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Stackup {
    pub materials: Vec<Material>,
    pub slabs: Vec<Slab>,
    pub layers: Vec<CopperLayer>,
    /// Heights of the ground planes.
    pub top_plane: Option<f64>,
    pub bottom_plane: Option<f64>,
    pub wall_x: f64,
    pub walls: Vec<WallSpan>,
}

impl CopperLayer {
    /// The positions in x of the copper edges of this layer at the fraction `f` of its thickness (0 bottom,
    /// 1 top), in no particular order.
    pub fn edges_x(&self, f: f64) -> impl Iterator<Item = f64> + '_ {
        let i = self.inset[0] + f * (self.inset[1] - self.inset[0]);
        let traces = self.traces.iter().flat_map(move |t| [t.x0 + i, t.x1 - i]);
        traces.chain(self.ground.into_iter().flat_map(move |[left, right]| [left - i, right + i]))
    }
}

impl Stackup {
    /// The positions in x of all copper edges, in ascending order; of etched copper at the bottom faces.
    pub fn copper_edges_x(&self) -> Vec<f64> {
        self.copper_edges_x_at(0.0)
    }

    /// As [`Self::copper_edges_x`] at the fraction `f` of the copper thickness (see [`CopperLayer::edges_x`]).
    pub fn copper_edges_x_at(&self, f: f64) -> Vec<f64> {
        let mut x: Vec<f64> = self.layers.iter().flat_map(|l| l.edges_x(f)).collect();
        x.sort_by(f64::total_cmp);
        x.dedup();
        x
    }

    /// The geometry for the mesher, in the box `domain = [x0, x1, y0, y1]` (grounded). Painting order is the
    /// precedence where pieces coincide: dielectrics, walls, planes, coplanar ground, traces.
    pub fn cross_section(&self, domain: [f64; 4]) -> CrossSection {
        let [x0, x1, ..] = domain;
        let diel = |shape, m| Item { shape, paint: Paint::Dielectric(m) };
        let cond = |shape, id, tag| Item { shape, paint: Paint::Conductor { id, tag } };
        let mut slabs = self.slabs.clone();
        let mut fills = Vec::new();
        for layer in self.layers.iter().filter(|l| l.t > 0.0) {
            let (bottom, top) = (layer.y0, layer.y0 + layer.t);
            // A fill of the same material as the dielectric next to it becomes part of that dielectric: an
            // interface between equal materials is none, and a layer as thin as the copper across the whole
            // width would force tiny elements everywhere.
            let same =
                |s: &Slab| self.materials[usize::from(s.material)] == self.materials[usize::from(layer.fill)];
            if let Some(above) = slabs.iter_mut().find(|s| s.y0 == top && same(s)) {
                above.y0 = bottom;
            } else if let Some(below) = slabs.iter_mut().find(|s| s.y1 == bottom && same(s)) {
                below.y1 = top;
            } else {
                fills.push(Slab { y0: bottom, y1: top, material: layer.fill });
            }
        }
        let mut items: Vec<Item> =
            slabs.iter().chain(&fills).map(|s| diel(Shape::rect(x0, s.y0, x1, s.y1), s.material)).collect();
        for side in [-self.wall_x, self.wall_x] {
            for w in &self.walls {
                items.push(cond(Shape::segment([side, w.y0], [side, w.y1]), GROUND, w.tag));
            }
        }
        for (y, tag) in [(self.top_plane, C_PL1), (self.bottom_plane, C_PL2)] {
            if let Some(y) = y {
                items.push(cond(Shape::segment([x0, y], [x1, y]), GROUND, tag));
            }
        }
        for layer in &self.layers {
            // a piece of copper between a and b: a rectangle, a trapezoid where a side is an etched flank, or a
            // line without thickness
            let piece = |a: f64, b: f64, flank_a: bool, flank_b: bool| {
                let (bottom, top) = (layer.y0, layer.y0 + layer.t);
                let [ib, it] = layer.inset;
                let at = |flank: bool, i: f64| if flank { i } else { 0.0 };
                if layer.t <= 0.0 {
                    Shape::segment([a, bottom], [b, bottom])
                } else if ib == it {
                    Shape::rect(a, bottom, b, top)
                } else {
                    Shape::Polygon(vec![
                        [a + at(flank_a, ib), bottom],
                        [b - at(flank_b, ib), bottom],
                        [b - at(flank_b, it), top],
                        [a + at(flank_a, it), top],
                    ])
                }
            };
            if let Some([left, right]) = layer.ground {
                items.push(cond(piece(right, x1, true, false), GROUND, C_COP));
                items.push(cond(piece(x0, left, false, true), GROUND, C_COP));
            }
            for t in &layer.traces {
                items.push(cond(piece(t.x0, t.x1, true, true), t.signal, C_SIG));
            }
        }
        CrossSection {
            domain,
            background: MAT_AIR,
            materials: self.materials.clone(),
            items,
            box_grounded: true,
            box_tag: C_BND,
        }
    }
}

impl PlanarParams {
    pub fn stackup(&self) -> Stackup {
        let p = self.effective();
        let (er_gap, tand_gap) = p.gap_props();
        let materials = vec![
            Material { er: 1.0, tand: 0.0 },
            Material { er: p.er1, tand: p.tand1 },
            Material { er: p.er2, tand: p.tand2 },
            Material { er: er_gap, tand: tand_gap },
        ];
        let (ytop, ybot) = (p.t + p.h1, -p.h2);
        let half = p.w / 2.0;
        let u = p.undercut();
        let layer = CopperLayer {
            y0: 0.0,
            t: p.t,
            inset: match p.etch_wide {
                WideSide::Bottom => [0.0, u],
                WideSide::Top => [u, 0.0],
            },
            fill: MAT_GAP,
            traces: vec![Trace { x0: -half, x1: half, signal: SIGNAL }],
            ground: p.coplanar.then(|| [-(half + p.s), half + p.s]),
        };
        let mut walls = Vec::new();
        if p.side_walls {
            // from plane to plane: the upper span includes the copper layer
            walls.push(WallSpan { y0: 0.0, y1: ytop, tag: C_F1 });
            walls.push(WallSpan { y0: ybot, y1: 0.0, tag: C_F2 });
        }
        if p.fence1 {
            walls.push(WallSpan { y0: p.t, y1: ytop, tag: C_F1 });
        }
        if p.fence2 {
            walls.push(WallSpan { y0: ybot, y1: 0.0, tag: C_F2 });
        }
        Stackup {
            materials,
            slabs: vec![
                Slab { y0: ybot, y1: 0.0, material: MAT_LOWER },
                Slab { y0: p.t, y1: ytop, material: MAT_UPPER },
            ],
            layers: vec![layer],
            top_plane: p.plane1.then_some(ytop),
            bottom_plane: p.plane2.then_some(ybot),
            wall_x: p.wall_x(),
            walls,
        }
    }
}
