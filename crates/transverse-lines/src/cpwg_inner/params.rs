use serde::{Deserialize, Serialize};
use transverse_core::quasistatic::{FdOptions, MeshSpec};
use transverse_core::special::k_ratio;
use transverse_core::{C_LIGHT, TvError, TvResult};

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GapMode {
    #[default]
    Upper,
    Lower,
    Custom,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MeshMode {
    #[default]
    Auto,
    Manual,
}

/// Numerical method of the field solution.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Solver {
    /// Finite volumes on a tensor grid (the reference).
    #[default]
    Fd,
    /// Quadratic finite elements on an adaptively refined triangle mesh.
    Fem,
}

/// Parameters of the CPWG inner-layer line (field names match the Python reference).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CpwgParams {
    // geometry
    pub w: f64,
    pub s: f64,
    pub t: f64,
    /// Distance gap edge -> via fence (coplanar ground continues beyond).
    pub g: f64,
    // upper dielectric / plane 1 / fence 1
    pub h1: f64,
    pub er1: f64,
    pub tand1: f64,
    pub plane1: bool,
    pub fence1: bool,
    // lower dielectric / plane 2 / fence 2
    pub h2: f64,
    pub er2: f64,
    pub tand2: f64,
    pub plane2: bool,
    pub fence2: bool,
    // gap material
    pub gap_mode: GapMode,
    pub er_gap: f64,
    pub tand_gap: f64,
    // conductor
    /// Conductivity [S/m].
    pub sigma: f64,
    /// RMS roughness [mm].
    pub rq: f64,
    // frequency / length
    pub f: f64,
    pub length: f64,
    // solver and mesh
    pub solver: Solver,
    pub mesh_mode: MeshMode,
    /// Target relative error [%] (auto mesh, domain size).
    pub tol: f64,
    /// Manual: smallest cell (at conductor edges) [mm].
    pub dmin: f64,
    /// Manual: largest cell in the core region [mm].
    pub dmax: f64,
    /// Manual: second level with half cell size + extrapolation.
    pub richardson: bool,
    pub max_nodes: usize,
    /// Domain extension for open structures [mm], 0 = auto.
    pub margin: f64,
}

impl Default for CpwgParams {
    fn default() -> Self {
        Self {
            w: 0.3605,
            s: 0.11,
            t: 0.018,
            g: 1.0,
            h1: 0.204,
            er1: 3.52,
            tand1: 0.02,
            plane1: true,
            fence1: true,
            h2: 0.254,
            er2: 3.66,
            tand2: 0.02,
            plane2: true,
            fence2: true,
            gap_mode: GapMode::Upper,
            er_gap: 3.52,
            tand_gap: 0.02,
            sigma: 5.8e7,
            rq: 0.0,
            f: 1e9,
            length: 100.0,
            solver: Solver::Fd,
            mesh_mode: MeshMode::Auto,
            tol: 0.5,
            dmin: 0.002,
            dmax: 0.02,
            richardson: true,
            max_nodes: 600_000,
            margin: 0.0,
        }
    }
}

impl CpwgParams {
    /// (er, tand) of the material between signal and coplanar ground.
    pub fn gap_props(&self) -> (f64, f64) {
        match self.gap_mode {
            GapMode::Upper => (self.er1, self.tand1),
            GapMode::Lower => (self.er2, self.tand2),
            GapMode::Custom => (self.er_gap, self.tand_gap),
        }
    }

    /// Fully enclosed by planes and fences.
    pub fn closed(&self) -> bool {
        self.plane1 && self.plane2 && self.fence1 && self.fence2
    }

    /// Wavelength [mm] in the densest dielectric at `f`.
    pub fn wavelength(&self) -> f64 {
        let er = self.er1.max(self.er2).max(self.gap_props().0);
        C_LIGHT / (self.f * er.sqrt()) * 1e3
    }

    pub fn validate(&self) -> TvResult<()> {
        let positive = [
            ("w", self.w),
            ("s", self.s),
            ("h1", self.h1),
            ("h2", self.h2),
            ("g", self.g),
            ("er1", self.er1),
            ("er2", self.er2),
            ("sigma", self.sigma),
            ("f", self.f),
            ("tol", self.tol),
        ];
        for (name, v) in positive {
            if v.is_nan() || v <= 0.0 {
                return Err(TvError::InvalidParam(format!("{name} must be > 0")));
            }
        }
        if self.t.is_nan() || self.t < 0.0 {
            return Err(TvError::InvalidParam("t must be >= 0".into()));
        }
        if self.gap_props().0 <= 0.0 {
            return Err(TvError::InvalidParam("er_gap must be > 0".into()));
        }
        if self.mesh_mode == MeshMode::Manual && (self.dmin <= 0.0 || self.dmax < self.dmin) {
            return Err(TvError::InvalidParam("manual mesh: need 0 < min cell <= max cell".into()));
        }
        Ok(())
    }

    pub fn fd_options(&self) -> FdOptions {
        FdOptions {
            mesh: match self.mesh_mode {
                MeshMode::Auto => MeshSpec::Auto,
                MeshMode::Manual => {
                    MeshSpec::Manual { dmin: self.dmin, dmax: self.dmax, richardson: self.richardson }
                }
            },
            tol_pct: self.tol,
            max_nodes: self.max_nodes,
            margin: self.margin,
        }
    }
}

/// Partial-capacitance conformal mapping, t = 0, infinite coplanar ground.
///
/// A missing plane is treated as a half-space filled with that layer's er (exact only if that
/// er = 1). Returns (Z0, eps_eff, exact).
pub fn analytic(p: &CpwgParams) -> (f64, f64, bool) {
    use std::f64::consts::PI;
    let k0 = p.w / (p.w + 2.0 * p.s);
    let q = |h: f64, plane: bool| {
        if !plane {
            return k_ratio(k0);
        }
        let k = (PI * p.w / (4.0 * h)).tanh() / (PI * (p.w + 2.0 * p.s) / (4.0 * h)).tanh();
        k_ratio(k)
    };
    let (q1, q2) = (q(p.h1, p.plane1), q(p.h2, p.plane2));
    let eeff = (p.er1 * q1 + p.er2 * q2) / (q1 + q2);
    let z0 = 60.0 * PI / (eeff.sqrt() * (q1 + q2));
    let exact = (p.plane1 || p.er1 == 1.0) && (p.plane2 || p.er2 == 1.0);
    (z0, eeff, exact)
}
