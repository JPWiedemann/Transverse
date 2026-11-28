//! Field data of a FEM solution: the recovered E field and the drawing information.

use transverse_core::TvResult;
use transverse_core::fem::{corner_gradients, element_dofs};
use transverse_core::postproc::mesh::{TriIndex, recover_nodal_corners};

use super::fem::FemSolution;
use super::field::drawing;
use super::{ModeKind, PlanarParams};
use crate::mesh_field::MeshFieldData;
use crate::section_solve::InputData;

/// Field data of `sol` (quadratic elements): potentials, E recovered at the nodes from the element
/// gradients, point location and the conductor drawing.
pub fn field_data_fem(p: &PlanarParams, sol: &FemSolution) -> TvResult<MeshFieldData> {
    let mesh = &sol.mesh;
    let cs = p.cross_section(sol.margin);
    let data = InputData::new(&cs, mesh);
    let inp = data.input(mesh);
    // E = -grad V, from the gradients of every element at its corners
    let e_tri: Vec<[[f64; 2]; 3]> =
        corner_gradients(&inp, &sol.edges, &sol.v)?.into_iter().map(|g| g.map(|g| [-g[0], -g[1]])).collect();
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
        e_nodal: recover_nodal_corners(&mesh.pts, &mesh.tris, &e_tri),
        edges: sol.edges.clone(),
        index: TriIndex::new(&mesh.pts, &mesh.tris),
        bounds,
        conductors: d.conductors,
        dashed_y: d.dashed_y,
        dotted_y: d.dotted_y,
        view_zoom: d.view_zoom,
        view_full: d.view_full,
        drive: match sol.modes[sol.mode].kind {
            ModeKind::Single => format!("1 V on signal, Z0 = {:.2} Ohm", sol.z0),
            ModeKind::Odd => {
                format!("+1 V / -1 V on the pair (odd mode), Zdiff = {:.2} Ohm", 2.0 * sol.z0)
            }
            ModeKind::Even => format!("1 V on both signals (even mode), Zcomm = {:.2} Ohm", sol.z0 / 2.0),
        },
    })
}
