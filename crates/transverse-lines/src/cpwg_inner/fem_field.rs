//! Field data of a FEM solution: the recovered E field and the drawing information.

use transverse_core::TvResult;
use transverse_core::fem::{centroid_gradients, element_dofs};
use transverse_core::postproc::mesh::{TriIndex, recover_nodal};

use super::CpwgParams;
use super::fem::{FemSolution, InputData};
use super::field::drawing;
use crate::mesh_field::MeshFieldData;

/// Field data of `sol` (quadratic elements): potentials, E recovered at the nodes from the element
/// gradients, point location and the conductor drawing.
pub fn field_data_fem(p: &CpwgParams, sol: &FemSolution) -> TvResult<MeshFieldData> {
    let mesh = &sol.mesh;
    let cs = p.cross_section(sol.margin);
    let data = InputData::new(&cs, mesh);
    let inp = data.input(mesh);
    // E = -grad V, from the gradient at the centroid of every element
    let e_tri: Vec<[f64; 2]> =
        centroid_gradients(&inp, &sol.edges, &sol.v)?.into_iter().map(|g| [-g[0], -g[1]]).collect();
    let d = drawing(p, sol.margin);
    let mut bounds = [f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY];
    for q in &mesh.pts {
        bounds = [bounds[0].min(q[0]), bounds[1].max(q[0]), bounds[2].min(q[1]), bounds[3].max(q[1])];
    }
    Ok(MeshFieldData {
        pts: mesh.pts.clone(),
        tris: mesh.tris.clone(),
        dofs: element_dofs(&inp, &sol.edges)?,
        tri_er: data.er.clone(),
        v: sol.v.clone(),
        v0: sol.v0.clone(),
        e_nodal: recover_nodal(&mesh.pts, &mesh.tris, &e_tri),
        edges: sol.edges.clone(),
        index: TriIndex::new(&mesh.pts, &mesh.tris),
        bounds,
        conductors: d.conductors,
        dashed_y: d.dashed_y,
        dotted_y: d.dotted_y,
        view_zoom: d.view_zoom,
        view_full: d.view_full,
        z0: sol.z0,
    })
}
