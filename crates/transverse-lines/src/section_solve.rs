//! Solving a [`CrossSection`] with any number of signal conductors: the bridge between a mesh and the finite
//! elements of the core, and the adaptive refinement of a section. What the line types share.

use transverse_core::fem::{self, FemInput, Level, LevelRun, Order, Solved};
use transverse_core::modes::CapMatrices;
use transverse_core::{Ctl, TvResult};
use transverse_geom::CrossSection;
use transverse_mesh::Mesh;

/// Fraction of the squared error indicator that the refined elements carry (Doerfler marking).
const MARK_FRACTION: f64 = 0.5;
/// Fewest elements marked per round, as a fraction of all: at the start nearly all of the indicator sits in a
/// handful of elements at the conductor corners, and marking only those would take many rounds.
const MIN_MARKED: f64 = 0.03;
/// Largest number of adaptive rounds.
const MAX_ROUNDS: usize = 60;

/// Per-triangle materials and the conductor edges of a mesh, owned so that [`FemInput`] can borrow them.
pub struct InputData {
    pub er: Vec<f64>,
    pub tand: Vec<f64>,
    /// Conductor boundary edges: with quadratic elements their midpoints are fixed as well.
    pub edge_cond: Vec<([u32; 2], u8)>,
}

impl InputData {
    pub fn new(cs: &CrossSection, mesh: &Mesh) -> Self {
        Self {
            er: mesh.tri_mat.iter().map(|m| cs.materials[usize::from(*m)].er).collect(),
            tand: mesh.tri_mat.iter().map(|m| cs.materials[usize::from(*m)].tand).collect(),
            edge_cond: mesh.edges.iter().map(|e| ([e.a.min(e.b), e.a.max(e.b)], e.id)).collect(),
        }
    }

    pub fn input<'a>(&'a self, mesh: &'a Mesh) -> FemInput<'a> {
        FemInput {
            pts: &mesh.pts,
            tris: &mesh.tris,
            er: &self.er,
            tand: &self.tand,
            node_cond: &mesh.node_cond,
            edge_cond: &self.edge_cond,
        }
    }
}

/// A mesh with its solutions, one per signal.
pub struct SectionSolution {
    pub mesh: Mesh,
    pub solved: Solved,
}

/// Solve on a given mesh of `cs`.
pub fn solve_mesh(
    cs: &CrossSection,
    mesh: Mesh,
    order: Order,
    ctl: &Ctl,
) -> TvResult<LevelRun<SectionSolution>> {
    let data = InputData::new(cs, &mesh);
    let solved = fem::solve_signals(&data.input(&mesh), order, ctl)?;
    let nodes = mesh.pts.len();
    let caps = solved.caps.clone();
    Ok(LevelRun { caps, nodes, payload: SectionSolution { mesh, solved } })
}

/// Solve the mesh that `first` returns, then split the elements with the largest error indicators until every
/// impedance has settled to `tol_pct` percent. Quadratic elements only.
pub fn refine_adaptively(
    cs: &CrossSection,
    tol_pct: f64,
    max_nodes: usize,
    ctl: &Ctl,
    mut first: impl FnMut() -> TvResult<LevelRun<SectionSolution>>,
) -> TvResult<(CapMatrices, SectionSolution, Level)> {
    fem::converge_adaptive(tol_pct, MAX_ROUNDS, ctl, |_, prev: Option<&SectionSolution>| {
        let Some(prev) = prev else { return first() };
        let data = InputData::new(cs, &prev.mesh);
        let eta = fem::error_indicators(&data.input(&prev.mesh), &prev.solved)?;
        let marked = fem::mark(&eta, MARK_FRACTION, MIN_MARKED);
        let mesh = transverse_mesh::refine_marked(&prev.mesh, &marked, max_nodes)?;
        solve_mesh(cs, mesh, Order::P2, ctl)
    })
}
