//! Planar straight-line graph (PSLG) of a cross-section: all shape boundaries split at their
//! mutual intersections and T-junctions, with coinciding pieces merged.
//!
//! The mesher turns every PSLG edge into constraint edges, so material interfaces and conductor
//! edges are always edges of the triangulation. Vertices closer than `1e-9 * diagonal` are one vertex
//! (relative tolerance, no absolute lengths).

use transverse_core::TvResult;
use transverse_geom::{ConductorId, CrossSection, P2, Paint, Tag};

/// Which conductor an edge belongs to; the highest `rank` wins where pieces coincide. The grounded
/// box has the lowest rank (-1), items rank by their position in the painting order.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Owner {
    pub rank: i64,
    pub id: ConductorId,
    pub tag: Tag,
}

#[derive(Clone, Copy, Debug)]
pub struct PEdge {
    pub a: u32,
    pub b: u32,
    /// `None`: only a material interface (or the edge of an ungrounded box).
    pub owner: Option<Owner>,
}

#[derive(Debug, Default)]
pub struct Pslg {
    pub pts: Vec<P2>,
    pub edges: Vec<PEdge>,
}

struct Raw {
    a: P2,
    b: P2,
    owner: Option<Owner>,
}

fn dist(a: P2, b: P2) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

/// Parameter (0..1) of the projection of `p` on the segment and the distance to the segment.
pub fn project(p: P2, a: P2, b: P2) -> (f64, f64) {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len2 = dx * dx + dy * dy;
    let t = if len2 > 0.0 { (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / len2).clamp(0.0, 1.0) } else { 0.0 };
    (t, dist(p, [a[0] + t * dx, a[1] + t * dy]))
}

/// Proper crossing of two segments (interior of both), if any.
fn crossing(a: P2, b: P2, c: P2, d: P2) -> Option<P2> {
    let (r, s) = ([b[0] - a[0], b[1] - a[1]], [d[0] - c[0], d[1] - c[1]]);
    let den = r[0] * s[1] - r[1] * s[0];
    let scale = (r[0].hypot(r[1])) * (s[0].hypot(s[1]));
    if den.abs() <= 1e-12 * scale {
        return None; // parallel or collinear: overlaps are handled by the endpoints lying on the other edge
    }
    let (qp0, qp1) = (c[0] - a[0], c[1] - a[1]);
    let t = (qp0 * s[1] - qp1 * s[0]) / den;
    let u = (qp0 * r[1] - qp1 * r[0]) / den;
    ((0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u)).then(|| [a[0] + t * r[0], a[1] + t * r[1]])
}

struct Vertices {
    pts: Vec<P2>,
    tol: f64,
}

impl Vertices {
    fn get(&mut self, p: P2) -> u32 {
        if let Some(k) = self.pts.iter().position(|q| dist(*q, p) <= self.tol) {
            return k as u32;
        }
        self.pts.push(p);
        (self.pts.len() - 1) as u32
    }
}

/// Build the graph of `cs`.
pub fn build(cs: &CrossSection) -> TvResult<Pslg> {
    cs.validate()?;
    let tol = 1e-9 * cs.diag();
    let d = cs.domain;
    let box_owner =
        cs.box_grounded.then_some(Owner { rank: -1, id: transverse_geom::GROUND, tag: cs.box_tag });
    let mut raw: Vec<Raw> = Vec::new();
    let corners = [[d[0], d[2]], [d[1], d[2]], [d[1], d[3]], [d[0], d[3]]];
    for k in 0..4 {
        raw.push(Raw { a: corners[k], b: corners[(k + 1) % 4], owner: box_owner });
    }
    for (k, it) in cs.items.iter().enumerate() {
        let owner = match it.paint {
            Paint::Conductor { id, tag } => Some(Owner { rank: k as i64, id, tag }),
            Paint::Dielectric(_) => None,
        };
        for (a, b) in it.shape.edges() {
            raw.push(Raw { a, b, owner });
        }
    }

    let mut verts = Vertices { pts: Vec::new(), tol };
    let mut edges: Vec<PEdge> = Vec::new();
    for (i, e) in raw.iter().enumerate() {
        // points on this edge: its ends, crossings with other edges, and ends of other edges lying on it
        let mut on: Vec<(f64, P2)> = vec![(0.0, e.a), (1.0, e.b)];
        for (j, o) in raw.iter().enumerate() {
            if i == j {
                continue;
            }
            if let Some(p) = crossing(e.a, e.b, o.a, o.b) {
                on.push((project(p, e.a, e.b).0, p));
            }
            for q in [o.a, o.b] {
                let (t, dd) = project(q, e.a, e.b);
                if dd <= tol && t > 0.0 && t < 1.0 {
                    on.push((t, q));
                }
            }
        }
        on.sort_by(|x, y| x.0.total_cmp(&y.0));
        let ids: Vec<u32> = on.iter().map(|(_, p)| verts.get(*p)).collect();
        for w in ids.windows(2) {
            if w[0] != w[1] {
                edges.push(PEdge { a: w[0].min(w[1]), b: w[0].max(w[1]), owner: e.owner });
            }
        }
    }

    // merge coinciding pieces, the highest-ranked owner wins
    edges.sort_by_key(|e| (e.a, e.b));
    let mut merged: Vec<PEdge> = Vec::new();
    for e in edges {
        match merged.last_mut() {
            Some(l) if l.a == e.a && l.b == e.b => {
                l.owner = match (l.owner, e.owner) {
                    (Some(x), Some(y)) => Some(if y.rank > x.rank { y } else { x }),
                    (x, y) => x.or(y),
                };
            }
            _ => merged.push(e),
        }
    }
    Ok(Pslg { pts: verts.pts, edges: merged })
}

#[cfg(test)]
mod tests {
    use super::*;
    use transverse_geom::{Item, Material, SIGNAL, Shape};

    fn cs(items: Vec<Item>) -> CrossSection {
        CrossSection {
            domain: [0.0, 10.0, 0.0, 4.0],
            background: 0,
            materials: vec![Material { er: 1.0, tand: 0.0 }, Material { er: 3.5, tand: 0.0 }],
            items,
            box_grounded: true,
            box_tag: 9,
        }
    }

    fn cond(shape: Shape, id: u8, tag: u8) -> Item {
        Item { shape, paint: Paint::Conductor { id, tag } }
    }

    #[test]
    fn an_empty_section_is_just_the_box() {
        let g = build(&cs(vec![])).unwrap();
        assert_eq!((g.pts.len(), g.edges.len()), (4, 4));
        assert!(g.edges.iter().all(|e| e.owner.unwrap().tag == 9));
    }

    #[test]
    fn crossing_segments_are_split_at_the_crossing() {
        let g = build(&cs(vec![
            cond(Shape::segment([2.0, 1.0], [8.0, 3.0]), SIGNAL, 1),
            cond(Shape::segment([2.0, 3.0], [8.0, 1.0]), 0, 2),
        ]))
        .unwrap();
        // 4 box corners + 4 segment ends + 1 crossing; each segment becomes two pieces
        assert_eq!(g.pts.len(), 9);
        assert_eq!(g.edges.len(), 4 + 4);
        let centre =
            g.pts.iter().position(|p| (p[0] - 5.0).abs() < 1e-12 && (p[1] - 2.0).abs() < 1e-12).unwrap();
        assert_eq!(g.edges.iter().filter(|e| e.a as usize == centre || e.b as usize == centre).count(), 4);
    }

    #[test]
    fn t_junctions_split_the_edge_they_touch_and_overlaps_merge_with_the_higher_rank() {
        // a plane on the lower box edge (overlaps it), a fence standing on it
        let g = build(&cs(vec![
            cond(Shape::segment([0.0, 0.0], [10.0, 0.0]), 0, 5),
            cond(Shape::segment([4.0, 0.0], [4.0, 4.0]), 0, 6),
        ]))
        .unwrap();
        // the bottom edge is split at x = 4 into two pieces, both owned by the plane (rank 0 > box)
        let bottom: Vec<_> = g
            .edges
            .iter()
            .filter(|e| g.pts[e.a as usize][1] == 0.0 && g.pts[e.b as usize][1] == 0.0)
            .collect();
        assert_eq!(bottom.len(), 2);
        assert!(bottom.iter().all(|e| e.owner.unwrap().tag == 5));
        // the fence meets the top box edge in a T and splits it too
        let top =
            g.edges.iter().filter(|e| g.pts[e.a as usize][1] == 4.0 && g.pts[e.b as usize][1] == 4.0).count();
        assert_eq!(top, 2);
        let fence = g.edges.iter().filter(|e| e.owner.unwrap().tag == 6).count();
        assert_eq!(fence, 1);
    }

    #[test]
    fn dielectric_edges_have_no_owner_and_coinciding_ones_merge() {
        // a dielectric covering the lower half shares the box sides and the interface with nothing
        let g =
            build(&cs(vec![Item { shape: Shape::rect(0.0, 0.0, 10.0, 2.0), paint: Paint::Dielectric(1) }]))
                .unwrap();
        let interface =
            g.edges.iter().filter(|e| g.pts[e.a as usize][1] == 2.0 && g.pts[e.b as usize][1] == 2.0).count();
        assert_eq!(interface, 1);
        assert!(
            g.edges
                .iter()
                .find(|e| g.pts[e.a as usize][1] == 2.0 && g.pts[e.b as usize][1] == 2.0)
                .unwrap()
                .owner
                .is_none()
        );
        // the box keeps its owner where the dielectric edge coincides with it
        let bottom =
            g.edges.iter().find(|e| g.pts[e.a as usize][1] == 0.0 && g.pts[e.b as usize][1] == 0.0).unwrap();
        assert_eq!(bottom.owner.unwrap().tag, 9);
        // 4 box corners + 2 side points at y = 2: box sides split, so 6 box pieces + the interface
        assert_eq!(g.edges.len(), 7);
    }

    #[test]
    fn vertices_within_the_relative_tolerance_are_one_vertex_at_any_scale() {
        // a thin 18 um strip in a 10 mm domain, with a corner off by 1e-12 relative
        let t = 0.018;
        let g = build(&cs(vec![
            cond(Shape::rect(3.0, 2.0, 7.0, 2.0 + t), SIGNAL, 1),
            cond(Shape::segment([7.0 + 1e-12, 2.0], [9.0, 2.0]), 0, 3),
        ]))
        .unwrap();
        // the segment starts on the strip's lower right corner: no extra vertex, the corner is shared
        let near: Vec<_> =
            g.pts.iter().filter(|p| (p[0] - 7.0).abs() < 1e-9 && (p[1] - 2.0).abs() < 1e-9).collect();
        assert_eq!(near.len(), 1);
    }

    #[test]
    fn invalid_sections_are_rejected() {
        let mut c = cs(vec![]);
        c.domain = [1.0, 1.0, 0.0, 4.0];
        assert!(build(&c).is_err());
    }
}
