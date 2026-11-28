//! Field data of a FEM solution: the recovered E field, the excitations to choose from and the drawing information.

use transverse_core::TvResult;
use transverse_core::fem::{corner_gradients, element_dofs};
use transverse_core::postproc::mesh::{TriIndex, recover_nodal_corners};

use super::fem::FemSolution;
use super::field::drawing;
use super::{ModeKind, PlanarParams};
use crate::mesh_field::{FieldBasis, MeshFieldData};
use crate::model::Excitation;
use crate::section_solve::InputData;

/// The excitations of a pair: its two modes, and each trace on its own. Empty for one signal.
fn excitations(sol: &FemSolution) -> Vec<Excitation> {
    let (Some(common), zdiff) = (sol.common_impedance(), sol.target_impedance()) else { return Vec::new() };
    let one =
        |key, label, drive, voltages: [f64; 2]| Excitation { key, label, drive, voltages: voltages.to_vec() };
    // one driven trace excites both modes at once, so no impedance belongs to it
    let alone = |on: u8, off: u8| format!("+1 V on trace {on}, trace {off} at 0 V (both modes at once)");
    vec![
        one(
            "odd",
            "Differential",
            format!("+1 V / -1 V on the pair (odd mode), Zdiff = {zdiff:.2} Ohm"),
            [1.0, -1.0],
        ),
        one(
            "even",
            "Common",
            format!("+1 V on both traces (even mode), Zcomm = {common:.2} Ohm"),
            [1.0, 1.0],
        ),
        one("trace1", "Trace 1 (left)", alone(1, 2), [1.0, 0.0]),
        one("trace2", "Trace 2 (right)", alone(2, 1), [0.0, 1.0]),
    ]
}

/// Field data of `sol` (quadratic elements): potentials, E recovered at the nodes from the element gradients,
/// point location and the conductor drawing. It shows the mode `sol` is in; a pair can be switched to the other
/// excitations.
pub fn field_data_fem(p: &PlanarParams, sol: &FemSolution) -> TvResult<MeshFieldData> {
    let mesh = &sol.mesh;
    let cs = p.cross_section(sol.margin);
    let data = InputData::new(&cs, mesh);
    let inp = data.input(mesh);
    // E = -grad V, from the gradients of every element at its corners
    let e_nodal = |v: &[f64]| -> TvResult<Vec<[f64; 2]>> {
        let e: Vec<[[f64; 2]; 3]> =
            corner_gradients(&inp, &sol.edges, v)?.into_iter().map(|g| g.map(|g| [-g[0], -g[1]])).collect();
        Ok(recover_nodal_corners(&mesh.pts, &mesh.tris, &e))
    };
    let excitations = excitations(sol);
    // the recovery is linear in the potential: the fields per signal give those of every excitation
    let basis = if excitations.is_empty() {
        Vec::new()
    } else {
        sol.signal_v
            .iter()
            .zip(&sol.signal_v0)
            .map(|(v, v0)| Ok(FieldBasis { v: v.clone(), v0: v0.clone(), e_nodal: e_nodal(v)? }))
            .collect::<TvResult<_>>()?
    };
    let d = drawing(p, sol.margin);
    let mut bounds = [f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY];
    for q in &mesh.pts {
        bounds = [bounds[0].min(q[0]), bounds[1].max(q[0]), bounds[2].min(q[1]), bounds[3].max(q[1])];
    }
    let mut out = MeshFieldData {
        pts: mesh.pts.clone(),
        tris: mesh.tris.clone(),
        dofs: element_dofs(&inp, &sol.edges)?,
        tri_er: data.er.clone(),
        v: sol.v.clone(),
        v0: sol.v0.clone(),
        e_nodal: e_nodal(&sol.v)?,
        edges: sol.edges.clone(),
        index: TriIndex::new(&mesh.pts, &mesh.tris),
        bounds,
        conductors: d.conductors,
        dashed_y: d.dashed_y,
        dotted_y: d.dotted_y,
        view_zoom: d.view_zoom,
        view_full: d.view_full,
        drive: format!("1 V on signal, Z0 = {:.2} Ohm", sol.z0),
        excitations,
        excitation: 0,
        basis,
    };
    if !out.excitations.is_empty() {
        let kind = sol.modes[sol.mode].kind;
        out.select(usize::from(kind == ModeKind::Even));
    }
    Ok(out)
}
