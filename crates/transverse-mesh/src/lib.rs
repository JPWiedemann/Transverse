//! Generic 2D triangle mesher: a [`CrossSection`] in, a [`Mesh`] out.
//!
//! Pipeline: PSLG of all shape boundaries (`planar`), size field (`sizefield`), constrained Delaunay
//! triangulation with boundary points at the local size and alternating quality / size refinement
//! (`refine`), then regions by triangle centre: dielectric triangles get their material, triangles
//! inside conductors are dropped and their boundary becomes Dirichlet. All interfaces and conductor
//! edges are constraint edges, so no triangle straddles an interface.

pub mod planar;
pub mod quality;
pub mod refine;
pub mod sizefield;

use transverse_core::{Ctl, TvResult};
use transverse_geom::{ConductorId, CrossSection, MaterialId, P2, Paint, Shape, SizeSpec, Tag};

use crate::planar::project;
pub use crate::sizefield::Hint;
use crate::sizefield::SizeField;

/// Piece of conductor (or grounded box) boundary: a mesh edge with the conductor and tag it belongs to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoundaryEdge {
    pub a: u32,
    pub b: u32,
    pub id: ConductorId,
    pub tag: Tag,
}

#[derive(Clone, Debug, Default)]
pub struct Mesh {
    pub pts: Vec<P2>,
    /// Counter-clockwise vertex indices; triangles inside conductors are not part of the mesh.
    pub tris: Vec<[u32; 3]>,
    pub tri_mat: Vec<MaterialId>,
    /// `Some(conductor)` for nodes on a conductor or on the grounded box (Dirichlet nodes).
    pub node_cond: Vec<Option<ConductorId>>,
    /// All conductor and box boundary edges (for fluxes and the ground split).
    pub edges: Vec<BoundaryEdge>,
}

/// Minimum angle the quality refinement aims for [deg].
pub const MIN_ANGLE_DEG: f64 = 25.0;

/// Mesh `cs` with element sizes from `spec`. `max_nodes` bounds the number of vertices
/// (`TvError::MeshTooLarge` beyond it); `ctl` cancels and reports progress.
pub fn mesh(cs: &CrossSection, spec: &SizeSpec, max_nodes: usize, ctl: &Ctl) -> TvResult<Mesh> {
    mesh_hinted(cs, spec, None, max_nodes, ctl)
}

/// As [`mesh`], with additional local size targets from an earlier mesh (adaptive refinement).
pub fn mesh_hinted(
    cs: &CrossSection,
    spec: &SizeSpec,
    hint: Option<Hint<'_>>,
    max_nodes: usize,
    ctl: &Ctl,
) -> TvResult<Mesh> {
    spec.validate()?;
    let pslg = planar::build(cs)?;
    let field = SizeField::with_hint(&pslg, *spec, hint);
    let conductors: Vec<&Shape> = cs
        .items
        .iter()
        .filter(|it| matches!(it.paint, Paint::Conductor { .. }))
        .map(|it| &it.shape)
        .collect();
    let in_conductor = |p: P2| conductors.iter().any(|s| s.contains(p));
    let st = refine::Settings { min_angle_deg: MIN_ANGLE_DEG, max_nodes, in_conductor: &in_conductor };
    let cdt = refine::triangulate(&pslg, &field, &st, ctl)?;
    ctl.check()?;
    Ok(extract(cs, &pslg, &cdt))
}

/// What is painted at `p` (the last item containing it wins): a material, or a conductor.
pub enum Region {
    Dielectric(MaterialId),
    Conductor,
}

pub fn region_of(cs: &CrossSection, p: P2) -> Region {
    let mut r = Region::Dielectric(cs.background);
    for it in &cs.items {
        if it.shape.contains(p) {
            r = match it.paint {
                Paint::Dielectric(m) => Region::Dielectric(m),
                Paint::Conductor { .. } => Region::Conductor,
            };
        }
    }
    r
}

fn extract(cs: &CrossSection, pslg: &planar::Pslg, cdt: &refine::Cdt) -> Mesh {
    use spade::Triangulation;
    let tol = 1e-9 * cs.diag();
    let mut mesh = Mesh::default();
    let mut new_index: Vec<Option<u32>> = vec![None; cdt.num_vertices()];
    let mut node = |mesh: &mut Mesh, idx: usize, p: P2| -> u32 {
        *new_index[idx].get_or_insert_with(|| {
            mesh.pts.push(p);
            (mesh.pts.len() - 1) as u32
        })
    };
    for f in cdt.inner_faces() {
        let v = f.positions();
        let c = [(v[0].x + v[1].x + v[2].x) / 3.0, (v[0].y + v[1].y + v[2].y) / 3.0];
        let Region::Dielectric(m) = region_of(cs, c) else { continue };
        let vs = f.vertices();
        let mut t = [0u32; 3];
        for k in 0..3 {
            t[k] = node(&mut mesh, vs[k].fix().index(), [v[k].x, v[k].y]);
        }
        // counter-clockwise
        let area2 = (v[1].x - v[0].x) * (v[2].y - v[0].y) - (v[2].x - v[0].x) * (v[1].y - v[0].y);
        if area2 < 0.0 {
            t.swap(1, 2);
        }
        mesh.tris.push(t);
        mesh.tri_mat.push(m);
    }
    // conductor and box boundary: constraint edges that lie on an owned PSLG edge
    let owned: Vec<_> = pslg
        .edges
        .iter()
        .filter_map(|e| e.owner.map(|o| (pslg.pts[e.a as usize], pslg.pts[e.b as usize], o)))
        .collect();
    mesh.node_cond = vec![None; mesh.pts.len()];
    let mut node_rank: Vec<i64> = vec![i64::MIN; mesh.pts.len()];
    for e in cdt.undirected_edges() {
        if !e.is_constraint_edge() {
            continue;
        }
        let [va, vb] = e.vertices();
        let (Some(a), Some(b)) = (new_index[va.fix().index()], new_index[vb.fix().index()]) else { continue };
        let (pa, pb) = (va.position(), vb.position());
        let mid = [0.5 * (pa.x + pb.x), 0.5 * (pa.y + pb.y)];
        let owner = owned
            .iter()
            .filter(|(s, t, _)| project(mid, *s, *t).1 <= tol)
            .map(|(_, _, o)| *o)
            .max_by_key(|o| o.rank);
        if let Some(o) = owner {
            mesh.edges.push(BoundaryEdge { a, b, id: o.id, tag: o.tag });
            for n in [a, b] {
                if o.rank > node_rank[n as usize] {
                    node_rank[n as usize] = o.rank;
                    mesh.node_cond[n as usize] = Some(o.id);
                }
            }
        }
    }
    mesh
}
