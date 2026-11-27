//! How a FEM solution is reported to the frontends: the generic mesh accuracy record and mesh measures.

use transverse_core::quasistatic::Accuracy;
use transverse_mesh::Mesh;

use super::FemSolution;

/// Length of the shortest triangle edge [mm].
pub(super) fn min_edge(mesh: &Mesh) -> f64 {
    let len = |a: u32, b: u32| {
        let (p, q) = (mesh.pts[a as usize], mesh.pts[b as usize]);
        (p[0] - q[0]).hypot(p[1] - q[1])
    };
    mesh.tris.iter().flat_map(|t| (0..3).map(|k| len(t[k], t[(k + 1) % 3]))).fold(f64::INFINITY, f64::min)
}

/// The accuracy record shown next to the field views. A triangle mesh has no grid size: `nx` and `ny` are 0.
pub fn fem_accuracy(sol: &FemSolution) -> Accuracy {
    let k = sol.accuracy.z0.len();
    Accuracy {
        nodes: sol.mesh.pts.len(),
        nx: 0,
        ny: 0,
        dmin_actual: min_edge(&sol.mesh),
        levels: (k >= 2).then(|| (sol.accuracy.z0[k - 2], sol.accuracy.z0[k - 1])),
        est_err: (k >= 2).then_some(sol.accuracy.est_err),
        mesh_used: (sol.spec.dmin, sol.spec.dmax, sol.adaptive),
        margin: sol.margin,
    }
}
