//! The finite-element backend of the CPWG: mesh the cross-section, solve with P1 elements, converge over
//! mesh levels. Library / CLI only for now; the field views, losses and ground split of this backend
//! follow in later steps. The finite-difference path ([`super::analyze`]) is not touched.

use transverse_core::fem::{self, FemInput, Level, LevelRun, Order};
use transverse_core::quasistatic::GridSource;
use transverse_core::{Ctl, TvResult};
use transverse_geom::{GROUND, SIGNAL};
use transverse_mesh::Mesh;

use super::{CpwgParams, MeshMode};

/// Options of the FEM analysis.
#[derive(Clone, Copy, Debug)]
pub struct FemOptions {
    /// Target error of Z0 in percent (relative change between the last two levels).
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
}

/// First mesh level: coarse, so that the three levels needed for the error estimate stay cheap.
pub const START_SCALE: f64 = 4.0;
pub const MAX_LEVELS: usize = 12;

impl CpwgParams {
    pub fn fem_options(&self) -> FemOptions {
        FemOptions {
            tol_pct: self.tol,
            max_nodes: self.max_nodes,
            margin: self.margin,
            manual: (self.mesh_mode == MeshMode::Manual).then_some((self.dmin, self.dmax)),
            start_scale: START_SCALE,
            max_levels: MAX_LEVELS,
            order: Order::P2,
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
}

type Payload = (Mesh, Vec<f64>, Vec<f64>, Vec<[u32; 2]>);

/// Mesh and solve one level with the element sizes scaled by `scale`.
fn solve_level(
    p: &CpwgParams,
    margin: f64,
    sizes: Option<(f64, f64)>,
    scale: f64,
    max_nodes: usize,
    order: Order,
    ctl: &Ctl,
) -> TvResult<LevelRun<Payload>> {
    let cs = p.cross_section(margin);
    let spec = match sizes {
        Some((dmin, dmax)) => p.size_spec_for(margin, dmin, dmax),
        None => p.size_spec(margin),
    }
    .scaled(scale);
    let mesh = transverse_mesh::mesh(&cs, &spec, max_nodes, ctl)?;
    let er: Vec<f64> = mesh.tri_mat.iter().map(|m| cs.materials[usize::from(*m)].er).collect();
    let tand: Vec<f64> = mesh.tri_mat.iter().map(|m| cs.materials[usize::from(*m)].tand).collect();
    let fixed: Vec<Option<f64>> = mesh
        .node_cond
        .iter()
        .map(|c| match c {
            Some(id) if *id == SIGNAL => Some(1.0),
            Some(id) if *id == GROUND => Some(0.0),
            Some(_) => Some(0.0),
            None => None,
        })
        .collect();
    // conductor edges: the midpoints of these edges are fixed as well with quadratic elements
    let fixed_edges: Vec<([u32; 2], f64)> = mesh
        .edges
        .iter()
        .map(|e| ([e.a.min(e.b), e.a.max(e.b)], if e.id == SIGNAL { 1.0 } else { 0.0 }))
        .collect();
    let inp = FemInput {
        pts: &mesh.pts,
        tris: &mesh.tris,
        er: &er,
        tand: &tand,
        fixed: &fixed,
        fixed_edges: &fixed_edges,
    };
    let s = fem::solve_lumped(&inp, order, ctl)?;
    let nodes = mesh.pts.len();
    Ok(LevelRun { lumped: s.lumped, nodes, payload: (mesh, s.v, s.v0, s.edges) })
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
            Ok(solve_level(p, m, opt.manual, 2.0, opt.max_nodes, opt.order, ctl)?.lumped.z0())
        })?
    };
    let (lumped, (mesh, v, v0, edges), accuracy) = match opt.manual {
        Some(sizes) => {
            ctl.msg("Solving (manual mesh) ...");
            let r = solve_level(p, margin, Some(sizes), 1.0, opt.max_nodes, opt.order, ctl)?;
            let acc = Level { nodes: vec![r.nodes], z0: vec![r.lumped.z0()], est_err: 0.0, order: None };
            (r.lumped, r.payload, acc)
        }
        None => fem::converge(opt.tol_pct, opt.max_levels, opt.start_scale, ctl, |scale, _| {
            solve_level(p, margin, None, scale, opt.max_nodes, opt.order, ctl)
        })?,
    };
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
    })
}
