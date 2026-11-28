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

/// The signal conductors.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Signals {
    /// One trace.
    #[default]
    Single,
    /// Two equal traces of width `w` in one layer, `d` apart (differential pair).
    Pair,
}

/// How the etched flanks of the copper (trace and coplanar ground) are given.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EtchMode {
    /// Vertical flanks.
    #[default]
    Rect,
    /// Etch factor: copper thickness / undercut of one side.
    Factor,
    /// Width difference: the narrow side of a trace is this much narrower than the wide one.
    Delta,
}

/// Which face of the copper layer is the wide one (the side on the laminate it was etched on).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WideSide {
    #[default]
    Bottom,
    Top,
}

/// Parameters of the planar line (the field names of the CPWG match the Python reference).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PlanarParams {
    // geometry
    pub signals: Signals,
    /// Pair: gap between the two traces, edge to edge.
    pub d: f64,
    pub w: f64,
    pub s: f64,
    pub t: f64,
    /// Distance gap edge -> via fence (coplanar ground continues beyond).
    pub g: f64,
    // etched flanks: `w`, `s` and `g` refer to the wide face of the copper
    pub etch: EtchMode,
    pub etch_factor: f64,
    /// Width of the wide face minus width of the narrow face of a trace [mm].
    pub etch_delta: f64,
    pub etch_wide: WideSide,
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
    // structure switches (see `structure.rs`)
    /// Coplanar ground beside the trace, `s` away.
    pub coplanar: bool,
    /// Upper half: dielectric `h1` and optionally plane 1 and fence 1 above the copper; otherwise air.
    pub upper: bool,
    /// The upper dielectric has `er2` and `tand2`.
    pub upper_same: bool,
    /// Without coplanar ground: walls from plane to plane at `g` outside the trace edges.
    pub side_walls: bool,
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
    // mesh
    pub mesh_mode: MeshMode,
    /// Target relative error [%] (auto mesh, domain size).
    pub tol: f64,
    /// Manual: smallest cell (at conductor edges) [mm].
    pub dmin: f64,
    /// Manual: largest cell in the core region [mm].
    pub dmax: f64,
    /// Grid solver (test reference) with a manual mesh: second level with half cell size + extrapolation.
    pub richardson: bool,
    pub max_nodes: usize,
    /// Domain extension for open structures [mm], 0 = auto.
    pub margin: f64,
}

impl Default for PlanarParams {
    fn default() -> Self {
        Self {
            signals: Signals::Single,
            d: 0.2,
            w: 0.3605,
            s: 0.11,
            t: 0.018,
            g: 1.0,
            etch: EtchMode::Rect,
            etch_factor: 2.0,
            etch_delta: 0.02,
            etch_wide: WideSide::Bottom,
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
            coplanar: true,
            upper: true,
            upper_same: false,
            side_walls: false,
            gap_mode: GapMode::Upper,
            er_gap: 3.52,
            tand_gap: 0.02,
            sigma: 5.8e7,
            rq: 0.0,
            f: 1e9,
            length: 100.0,
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

impl PlanarParams {
    /// (er, tand) of the material beside the trace (between it and a coplanar ground). Of resolved switches
    /// ([`Self::effective`]) when the upper half is concerned.
    pub fn gap_props(&self) -> (f64, f64) {
        match self.gap_mode {
            GapMode::Upper => (self.er1, self.tand1),
            GapMode::Lower => (self.er2, self.tand2),
            GapMode::Custom => (self.er_gap, self.tand_gap),
        }
    }

    /// Wavelength [mm] in the densest dielectric at `f`.
    pub fn wavelength(&self) -> f64 {
        let e = self.effective();
        let er = e.er1.max(e.er2).max(e.gap_props().0);
        C_LIGHT / (e.f * er.sqrt()) * 1e3
    }

    /// Only what the structure uses has to be valid.
    pub fn validate(&self) -> TvResult<()> {
        let e = self.effective();
        let fences = e.coplanar || e.side_walls;
        let positive = [
            ("w", e.w, true),
            ("d", e.d, e.pair()),
            ("s", e.s, e.coplanar),
            ("h1", e.h1, true),
            ("h2", e.h2, true),
            ("g", e.g, fences),
            ("er1", e.er1, true),
            ("er2", e.er2, true),
            ("sigma", e.sigma, true),
            ("f", e.f, true),
            ("tol", e.tol, true),
        ];
        for (name, v, used) in positive {
            if used && (v.is_nan() || v <= 0.0) {
                return Err(TvError::InvalidParam(format!("{name} must be > 0")));
            }
        }
        if !(e.coplanar || e.plane1 || e.plane2) {
            return Err(TvError::InvalidParam(
                "the structure has no ground: switch on the coplanar ground or a ground plane".into(),
            ));
        }
        if e.t.is_nan() || e.t < 0.0 {
            return Err(TvError::InvalidParam("t must be >= 0".into()));
        }
        match e.etch {
            EtchMode::Factor if e.etch_factor.is_nan() || e.etch_factor <= 0.0 => {
                return Err(TvError::InvalidParam("etch factor must be > 0".into()));
            }
            EtchMode::Delta if e.etch_delta.is_nan() || e.etch_delta < 0.0 => {
                return Err(TvError::InvalidParam("etch width difference must be >= 0".into()));
            }
            _ => {}
        }
        let u = e.undercut();
        if e.narrow_width() <= 0.0 {
            return Err(TvError::InvalidParam(format!(
                "etching leaves no trace: the undercut of {:.2} um per side needs w > {:.2} um",
                u * 1e3,
                2.0 * u * 1e3
            )));
        }
        if (e.fence1 || e.fence2) && e.g <= u {
            return Err(TvError::InvalidParam(format!(
                "g must be larger than the undercut of {:.2} um, or the via fences miss the coplanar ground",
                u * 1e3
            )));
        }
        if e.gap_props().0 <= 0.0 {
            return Err(TvError::InvalidParam("er_gap must be > 0".into()));
        }
        if e.mesh_mode == MeshMode::Manual && (e.dmin <= 0.0 || e.dmax < e.dmin) {
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

/// An estimate of Z0 without a field solver, for t = 0.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Analytic {
    pub z0: f64,
    pub eeff: f64,
    /// Exact for this structure at t = 0 (up to the infinitely wide coplanar ground and planes).
    pub exact: bool,
    pub method: &'static str,
}

/// Estimate for the structure, if there is a formula for it:
///
/// * with a coplanar ground, and for a stripline: partial-capacitance conformal mapping, each half of the cross
///   section on its own. A missing plane of a coplanar waveguide is treated as a half-space filled with that
///   layer's er (exact only if that er = 1). A stripline is the limit of an infinitely wide gap.
/// * microstrip (air above, one plane below): Hammerstad and Jensen, within about 1 %.
pub fn analytic(p: &PlanarParams) -> Option<Analytic> {
    use std::f64::consts::PI;
    let p = p.effective();
    if p.pair() {
        return None;
    }
    let microstrip = !p.coplanar && !p.plane1 && p.plane2 && p.er1 == 1.0 && p.gap_props().0 == 1.0;
    if microstrip {
        let (z0, eeff) = hammerstad_jensen(p.w / p.h2, p.er2);
        return Some(Analytic { z0, eeff, exact: false, method: "Hammerstad-Jensen" });
    }
    if !p.coplanar && !(p.plane1 && p.plane2) {
        return None;
    }
    // modulus of one half: its capacitance is proportional to K(k)/K(k')
    let q = |h: f64, plane: bool| {
        let k0 = p.w / (p.w + 2.0 * p.s);
        if !plane {
            return k_ratio(k0);
        }
        let near = (PI * p.w / (4.0 * h)).tanh();
        let k = if p.coplanar { near / (PI * (p.w + 2.0 * p.s) / (4.0 * h)).tanh() } else { near };
        k_ratio(k)
    };
    let (q1, q2) = (q(p.h1, p.plane1), q(p.h2, p.plane2));
    let eeff = (p.er1 * q1 + p.er2 * q2) / (q1 + q2);
    let z0 = 60.0 * PI / (eeff.sqrt() * (q1 + q2));
    let exact = if p.coplanar {
        (p.plane1 || p.er1 == 1.0) && (p.plane2 || p.er2 == 1.0)
    } else {
        // exact for the symmetric stripline in one material
        p.er1 == p.er2 && p.h1 == p.h2
    };
    Some(Analytic { z0, eeff, exact, method: "conformal mapping" })
}

/// Microstrip without thickness, `u = w / h`: (Z0, eps_eff). E. Hammerstad and O. Jensen, "Accurate models for
/// microstrip computer-aided design", 1980.
fn hammerstad_jensen(u: f64, er: f64) -> (f64, f64) {
    use std::f64::consts::PI;
    const ETA0: f64 = 376.730_313;
    let f = 6.0 + (2.0 * PI - 6.0) * (-(30.666 / u).powf(0.7528)).exp();
    let z_air = ETA0 / (2.0 * PI) * (f / u + (1.0 + (2.0 / u).powi(2)).sqrt()).ln();
    let a = 1.0
        + ((u.powi(4) + (u / 52.0).powi(2)) / (u.powi(4) + 0.432)).ln() / 49.0
        + (1.0 + (u / 18.1).powi(3)).ln() / 18.7;
    let b = 0.564 * ((er - 0.9) / (er + 3.0)).powf(0.053);
    let eeff = (er + 1.0) / 2.0 + (er - 1.0) / 2.0 * (1.0 + 10.0 / u).powf(-a * b);
    (z_air / eeff.sqrt(), eeff)
}
