//! The finite-element backend of the CPWG: mesh the cross-section, solve with quadratic elements and refine
//! the mesh where the error indicator is large until Z0 has settled (or solve uniform mesh levels, or one
//! manual mesh). The finite-difference path ([`super::analyze`]) is separate.

use transverse_core::fem::{self, FemInput, Level, LevelRun, Order};
use transverse_core::quasistatic::GridSource;
use transverse_core::{Ctl, TvResult};
use transverse_geom::{CrossSection, SIGNAL, SizeSpec};
use transverse_mesh::Mesh;

use super::{CpwgParams, MeshMode};

/// Options of the FEM analysis.
#[derive(Clone, Copy, Debug)]
pub struct FemOptions {
    /// Target error of Z0 in percent.
    pub tol_pct: f64,
    pub max_nodes: usize,
    /// Domain extension for open structures [mm]; 0 = automatic.
    pub margin: f64,
    /// Manual mesh: element sizes (min, max) of a single level, `None` = automatic levels.
    pub manual: Option<(f64, f64)>,
    /// Automatic levels: size factor of the first level (relative to the sizes of the grid's first level)
    /// and the largest number of levels.
    pub start_scale: f64,
    pub max_levels: usize,
    pub order: Order,
    /// Refine where the error indicator is large instead of uniformly (automatic mesh only).
    pub adaptive: bool,
}

/// First mesh level of the uniform refinement: coarse, so that the three levels needed for the error estimate
/// stay cheap.
pub const START_SCALE: f64 = 4.0;
/// Size field of the first mesh of the adaptive refinement, relative to the sizes of the grid's first level:
/// coarse, because the refinement puts the elements where they are needed.
pub const ADAPTIVE_BASE: f64 = 4.0;
pub const MAX_LEVELS: usize = 12;
/// Fraction of the squared error indicator that the refined elements carry (Doerfler marking).
const MARK_FRACTION: f64 = 0.5;
/// Fewest elements marked per round, as a fraction of all: at the start nearly all of the indicator sits in a
/// handful of elements at the conductor corners, and marking only those would take many rounds.
const MIN_MARKED: f64 = 0.03;
/// Largest number of adaptive rounds.
const MAX_ROUNDS: usize = 60;

impl FemOptions {
    /// Uniform mesh levels instead of adaptive refinement (with the coarse first level they need).
    pub fn uniform(self) -> Self {
        Self { adaptive: false, start_scale: START_SCALE, ..self }
    }
}

impl CpwgParams {
    pub fn fem_options(&self) -> FemOptions {
        FemOptions {
            tol_pct: self.tol,
            max_nodes: self.max_nodes,
            margin: self.margin,
            manual: (self.mesh_mode == MeshMode::Manual).then_some((self.dmin, self.dmax)),
            start_scale: ADAPTIVE_BASE,
            max_levels: MAX_LEVELS,
            order: Order::P2,
            adaptive: true,
        }
    }
}

/// Result of a FEM analysis: lumped values, accuracy, and the finest mesh with its potentials.
#[derive(Clone, Debug)]
pub struct FemSolution {
    /// Capacitance with dielectrics [F/m].
    pub c: f64,
    /// Capacitance in air [F/m].
    pub c0: f64,
    pub g_over_omega: f64,
    pub z0: f64,
    pub eeff: f64,
    /// Inductance per metre [H/m].
    pub l: f64,
    pub accuracy: Level,
    /// Domain extension used [mm] (0 for closed structures).
    pub margin: f64,
    pub mesh: Mesh,
    /// Potentials of the finest level: with dielectrics and in air.
    pub v: Vec<f64>,
    pub v0: Vec<f64>,
    /// Edges whose midpoints carry the extra degrees of freedom of quadratic elements (after the nodes).
    pub edges: Vec<[u32; 2]>,
    pub order: Order,
    /// Size field of the mesh; for an adaptive mesh, of the first one (which was then refined locally).
    pub spec: SizeSpec,
    /// Whether the mesh was refined adaptively (marked triangles split) rather than in uniform levels.
    pub adaptive: bool,
}

/// Mesh and potentials of one level.
pub(super) struct Payload {
    pub mesh: Mesh,
    pub v: Vec<f64>,
    pub v0: Vec<f64>,
    pub edges: Vec<[u32; 2]>,
    pub lumped: fem::Lumped,
}

/// Per-triangle materials and the fixed potentials of a mesh, owned so that [`FemInput`] can borrow them.
pub(super) struct InputData {
    pub er: Vec<f64>,
    pub tand: Vec<f64>,
    pub fixed: Vec<Option<f64>>,
    /// Conductor boundary edges: with quadratic elements their midpoints are fixed as well.
    pub fixed_edges: Vec<([u32; 2], f64)>,
}

impl InputData {
    pub fn new(cs: &CrossSection, mesh: &Mesh) -> Self {
        let volt = |id: u8| if id == SIGNAL { 1.0 } else { 0.0 };
        Self {
            er: mesh.tri_mat.iter().map(|m| cs.materials[usize::from(*m)].er).collect(),
            tand: mesh.tri_mat.iter().map(|m| cs.materials[usize::from(*m)].tand).collect(),
            fixed: mesh.node_cond.iter().map(|c| c.map(volt)).collect(),
            fixed_edges: mesh.edges.iter().map(|e| ([e.a.min(e.b), e.a.max(e.b)], volt(e.id))).collect(),
        }
    }

    pub fn input<'a>(&'a self, mesh: &'a Mesh) -> FemInput<'a> {
        FemInput {
            pts: &mesh.pts,
            tris: &mesh.tris,
            er: &self.er,
            tand: &self.tand,
            fixed: &self.fixed,
            fixed_edges: &self.fixed_edges,
        }
    }
}

/// Size field of a level: the first-level sizes (or the manual ones) scaled by `scale`.
fn level_spec(p: &CpwgParams, margin: f64, sizes: Option<(f64, f64)>, scale: f64) -> SizeSpec {
    match sizes {
        Some((dmin, dmax)) => p.size_spec_for(margin, dmin, dmax),
        None => p.size_spec(margin),
    }
    .scaled(scale)
}

/// Mesh and solve one level with the size field `spec`.
pub(super) fn solve_level(
    p: &CpwgParams,
    margin: f64,
    spec: &SizeSpec,
    max_nodes: usize,
    order: Order,
    ctl: &Ctl,
) -> TvResult<LevelRun<Payload>> {
    let cs = p.cross_section(margin);
    let mesh = transverse_mesh::mesh(&cs, spec, max_nodes, ctl)?;
    solve_mesh(&cs, mesh, order, ctl)
}

/// Solve on a given mesh of `cs`.
pub(super) fn solve_mesh(
    cs: &CrossSection,
    mesh: Mesh,
    order: Order,
    ctl: &Ctl,
) -> TvResult<LevelRun<Payload>> {
    let data = InputData::new(cs, &mesh);
    let s = fem::solve_lumped(&data.input(&mesh), order, ctl)?;
    let nodes = mesh.pts.len();
    let payload = Payload { mesh, v: s.v, v0: s.v0, edges: s.edges, lumped: s.lumped };
    Ok(LevelRun { lumped: s.lumped, nodes, payload })
}

/// Solve, then split the elements with the largest error indicators, until Z0 has settled.
fn refine_adaptively(
    p: &CpwgParams,
    margin: f64,
    spec: &SizeSpec,
    opt: &FemOptions,
    ctl: &Ctl,
) -> TvResult<(fem::Lumped, Payload, Level)> {
    let cs = p.cross_section(margin);
    fem::converge_adaptive(opt.tol_pct, MAX_ROUNDS, ctl, |_, prev: Option<&Payload>| {
        let Some(prev) = prev else {
            return solve_level(p, margin, spec, opt.max_nodes, opt.order, ctl);
        };
        let data = InputData::new(&cs, &prev.mesh);
        let inp = data.input(&prev.mesh);
        let eta = fem::error_indicators(&inp, &prev.edges, &prev.v, &prev.v0, &prev.lumped)?;
        let marked = fem::mark(&eta, MARK_FRACTION, MIN_MARKED);
        let mesh = transverse_mesh::refine_marked(&prev.mesh, &marked, opt.max_nodes)?;
        solve_mesh(&cs, mesh, opt.order, ctl)
    })
}

/// Analyse one geometry with the FEM backend.
pub fn analyze_fem(p: &CpwgParams, opt: &FemOptions, ctl: &Ctl) -> TvResult<FemSolution> {
    p.validate()?;
    let margin = if p.closed() {
        0.0
    } else if opt.margin > 0.0 {
        opt.margin
    } else {
        // a coarse mesh is enough for the domain-size check, as for the grid (twice the base sizes)
        fem::auto_margin_with(p.initial_margin(), opt.tol_pct, ctl, |m| {
            ctl.msg(&format!("Domain size check: margin {m:.3} mm ..."));
            Ok(solve_level(p, m, &level_spec(p, m, opt.manual, 2.0), opt.max_nodes, opt.order, ctl)?
                .lumped
                .z0())
        })?
    };
    let adaptive = opt.adaptive && opt.manual.is_none() && opt.order == Order::P2;
    let (lumped, Payload { mesh, v, v0, edges, .. }, accuracy) = match opt.manual {
        Some(sizes) => {
            ctl.msg("Solving (manual mesh) ...");
            let spec = level_spec(p, margin, Some(sizes), 1.0);
            let r = solve_level(p, margin, &spec, opt.max_nodes, opt.order, ctl)?;
            let acc = Level {
                nodes: vec![r.nodes],
                z0: vec![r.lumped.z0()],
                scales: vec![1.0],
                est_err: 0.0,
                order: None,
            };
            (r.lumped, r.payload, acc)
        }
        None if adaptive => {
            refine_adaptively(p, margin, &level_spec(p, margin, None, opt.start_scale), opt, ctl)?
        }
        None => fem::converge(opt.tol_pct, opt.max_levels, opt.start_scale, ctl, |scale, _| {
            solve_level(p, margin, &level_spec(p, margin, None, scale), opt.max_nodes, opt.order, ctl)
        })?,
    };
    let scale = if adaptive { opt.start_scale } else { accuracy.scales.last().copied().unwrap_or(1.0) };
    Ok(FemSolution {
        c: lumped.c,
        c0: lumped.c0,
        g_over_omega: lumped.g_over_omega,
        z0: lumped.z0(),
        eeff: lumped.eeff(),
        l: lumped.l(),
        accuracy,
        margin,
        mesh,
        v,
        v0,
        edges,
        order: opt.order,
        spec: level_spec(p, margin, opt.manual, scale),
        adaptive,
    })
}
