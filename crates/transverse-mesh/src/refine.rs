//! Triangulation of a PSLG with a size field: constrained Delaunay (spade), boundary points at the
//! local size, then alternating quality refinement (minimum angle, spade) and size-driven insertion
//! until every triangle is small enough for the size field.

use spade::handles::{FixedFaceHandle, FixedVertexHandle, InnerTag};
use spade::{
    AngleLimit, ConstrainedDelaunayTriangulation, InsertionError, Point2, RefinementParameters, Triangulation,
};
use transverse_core::{Ctl, TvError, TvResult};
use transverse_geom::P2;

use crate::planar::Pslg;
use crate::sizefield::SizeField;

pub type Cdt = ConstrainedDelaunayTriangulation<Point2<f64>>;

pub struct Settings<'a> {
    pub min_angle_deg: f64,
    pub max_nodes: usize,
    /// Triangles whose centre is inside a conductor are not refined by size (they are dropped later).
    pub in_conductor: &'a dyn Fn(P2) -> bool,
}

const MAX_ROUNDS: usize = 200;

fn ins(cdt: &mut Cdt, p: P2) -> TvResult<FixedVertexHandle> {
    cdt.insert(Point2::new(p[0], p[1]))
        .map_err(|e: InsertionError| TvError::Solver(format!("mesher: cannot insert a point: {e:?}")))
}

fn too_large(nodes: usize, max: usize) -> TvError {
    TvError::MeshTooLarge { nodes, max }
}

/// Points strictly between `a` and `b` such that no piece is longer than the size field at its middle.
fn subdivide(a: P2, b: P2, field: &SizeField, out: &mut Vec<P2>) {
    let (len, mid) = ((a[0] - b[0]).hypot(a[1] - b[1]), [0.5 * (a[0] + b[0]), 0.5 * (a[1] + b[1])]);
    if len > field.at(mid) {
        subdivide(a, mid, field, out);
        out.push(mid);
        subdivide(mid, b, field, out);
    }
}

fn centroid(v: [Point2<f64>; 3]) -> P2 {
    [(v[0].x + v[1].x + v[2].x) / 3.0, (v[0].y + v[1].y + v[2].y) / 3.0]
}

fn longest_edge(v: [Point2<f64>; 3]) -> f64 {
    (0..3).map(|k| (v[k].x - v[(k + 1) % 3].x).hypot(v[k].y - v[(k + 1) % 3].y)).fold(0.0, f64::max)
}

fn inside(v: [Point2<f64>; 3], p: P2) -> bool {
    let s = |a: Point2<f64>, b: Point2<f64>| (b.x - a.x) * (p[1] - a.y) - (b.y - a.y) * (p[0] - a.x);
    let (d0, d1, d2) = (s(v[0], v[1]), s(v[1], v[2]), s(v[2], v[0]));
    (d0 > 0.0 && d1 > 0.0 && d2 > 0.0) || (d0 < 0.0 && d1 < 0.0 && d2 < 0.0)
}

/// Triangulate. Every PSLG edge becomes constraint edges, so interfaces and conductor edges are
/// edges of the triangulation.
pub fn triangulate(pslg: &Pslg, field: &SizeField, st: &Settings<'_>, ctl: &Ctl) -> TvResult<Cdt> {
    let mut cdt = Cdt::new();
    let ids: Vec<FixedVertexHandle> = pslg.pts.iter().map(|p| ins(&mut cdt, *p)).collect::<TvResult<_>>()?;
    // all boundary points first, then the constraints between consecutive ones
    let mut chains: Vec<Vec<FixedVertexHandle>> = Vec::with_capacity(pslg.edges.len());
    for e in &pslg.edges {
        ctl.check()?;
        let (a, b) = (pslg.pts[e.a as usize], pslg.pts[e.b as usize]);
        let mut mid = Vec::new();
        subdivide(a, b, field, &mut mid);
        let mut chain = vec![ids[e.a as usize]];
        for p in mid {
            chain.push(ins(&mut cdt, p)?);
        }
        chain.push(ids[e.b as usize]);
        if cdt.num_vertices() > st.max_nodes {
            return Err(too_large(cdt.num_vertices(), st.max_nodes));
        }
        chains.push(chain);
    }
    for chain in &chains {
        for w in chain.windows(2) {
            cdt.add_constraint(w[0], w[1]);
        }
    }

    let angle = AngleLimit::from_deg(st.min_angle_deg);
    let quality = |cdt: &mut Cdt| -> TvResult<()> {
        let room = st.max_nodes.saturating_sub(cdt.num_vertices());
        let res = cdt.refine(
            RefinementParameters::<f64>::new().with_angle_limit(angle).with_max_additional_vertices(room),
        );
        if res.refinement_complete { Ok(()) } else { Err(too_large(cdt.num_vertices(), st.max_nodes)) }
    };
    quality(&mut cdt)?;

    for round in 0..MAX_ROUNDS {
        ctl.check()?;
        ctl.msg(&format!("Meshing: {} nodes", cdt.num_vertices()));
        // triangles that are larger than the size field asks, the longest first
        let mut cand: Vec<(f64, FixedFaceHandle<InnerTag>, P2)> = Vec::new();
        for f in cdt.inner_faces() {
            let v = f.positions();
            let c = centroid(v);
            if (st.in_conductor)(c) {
                continue;
            }
            let l = longest_edge(v);
            if l > field.at(c) {
                let cc = f.circumcenter();
                let p = if inside(v, [cc.x, cc.y]) { [cc.x, cc.y] } else { c };
                cand.push((l, f.fix(), p));
            }
        }
        if cand.is_empty() {
            return Ok(cdt);
        }
        cand.sort_by(|a, b| b.0.total_cmp(&a.0));
        // at most one new point per neighbourhood and round: a chosen triangle blocks itself and its neighbours
        let mut blocked = vec![false; cdt.num_all_faces()];
        let mut chosen: Vec<P2> = Vec::new();
        for (_, handle, p) in cand {
            if blocked[handle.index()] {
                continue;
            }
            blocked[handle.index()] = true;
            let face = cdt.face(handle);
            for e in face.adjacent_edges() {
                if let Some(n) = e.rev().face().as_inner() {
                    blocked[n.fix().index()] = true;
                }
            }
            chosen.push(p);
        }
        for p in chosen {
            ins(&mut cdt, p)?;
        }
        if cdt.num_vertices() > st.max_nodes {
            return Err(too_large(cdt.num_vertices(), st.max_nodes));
        }
        quality(&mut cdt)?;
        if round + 1 == MAX_ROUNDS {
            return Err(TvError::Solver("mesher: size refinement did not converge".into()));
        }
    }
    Ok(cdt)
}
