//! The interface between line types and frontends.
//!
//! A frontend never talks to a solver directly: it builds a parameter form from [`ParamSpec`]s,
//! hands a [`ParamSet`] to a [`LineModel`] and gets an [`Analysis`] back. Everything in here is
//! plain data (serde) except the frequency-response trait object.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use transverse_core::losses::{FreqResponse, LossPoint};
use transverse_core::postproc::{self, EField, StreamOptions};
use transverse_core::quasistatic::Accuracy;

use crate::mesh_field::MeshFieldData;
use transverse_core::{Ctl, TvError, TvResult};

// ---------------------------------------------------------------------------------------------
// Parameters
// ---------------------------------------------------------------------------------------------

/// A parameter value. Choices are stored as their index; booleans as booleans.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ParamValue {
    Bool(bool),
    Num(f64),
}

/// All parameters of a line type by key. Lengths in mm, frequencies in Hz (internal units).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ParamSet(pub BTreeMap<String, ParamValue>);

impl ParamSet {
    pub fn set(&mut self, key: &str, v: ParamValue) {
        self.0.insert(key.to_string(), v);
    }

    pub fn get(&self, key: &str) -> Option<ParamValue> {
        self.0.get(key).copied()
    }

    pub fn num(&self, key: &str) -> TvResult<f64> {
        match self.get(key) {
            Some(ParamValue::Num(v)) => Ok(v),
            _ => Err(TvError::InvalidParam(format!("{key}: number expected"))),
        }
    }

    pub fn flag(&self, key: &str) -> TvResult<bool> {
        match self.get(key) {
            Some(ParamValue::Bool(v)) => Ok(v),
            _ => Err(TvError::InvalidParam(format!("{key}: boolean expected"))),
        }
    }

    pub fn choice(&self, key: &str) -> TvResult<usize> {
        let v = self.num(key)?;
        if v >= 0.0 && v.fract() == 0.0 {
            Ok(v as usize)
        } else {
            Err(TvError::InvalidParam(format!("{key}: choice index expected")))
        }
    }
}

/// What kind of input a parameter is, with its default and display hints.
#[derive(Clone, Copy, Debug, Serialize)]
pub enum ParamKind {
    /// Length in mm; `unit` is the initial display unit, `lambda` allows "relative to wavelength".
    Length {
        default_mm: f64,
        unit: &'static str,
        lambda: bool,
    },
    /// Frequency in Hz.
    Frequency {
        default_hz: f64,
        unit: &'static str,
    },
    /// Plain number shown as `value / scale` followed by `unit` (e.g. S/m shown in MS/m).
    Number {
        default: f64,
        unit: &'static str,
        scale: f64,
    },
    Bool(bool),
    /// Options and the default index.
    Choice(&'static [&'static str], usize),
}

/// Another parameter has a given value (choice index, or 0/1 for a bool), or with `not` any other value. A
/// condition on a quantity can only ask for zero: frontends pass on the number of a quantity only if it is 0.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct Condition {
    pub key: &'static str,
    pub value: usize,
    pub not: bool,
}

/// When a parameter or option applies, and is shown: all conditions of one of the alternatives hold. Without
/// alternatives it always applies.
pub type Visibility = &'static [&'static [Condition]];

#[derive(Clone, Copy, Debug, Serialize)]
pub struct ParamSpec {
    pub key: &'static str,
    pub label: &'static str,
    pub group: &'static str,
    pub kind: ParamKind,
    pub visible_when: Visibility,
}

impl ParamSpec {
    pub fn default_value(&self) -> ParamValue {
        match self.kind {
            ParamKind::Length { default_mm, .. } => ParamValue::Num(default_mm),
            ParamKind::Frequency { default_hz, .. } => ParamValue::Num(default_hz),
            ParamKind::Number { default, .. } => ParamValue::Num(default),
            ParamKind::Bool(b) => ParamValue::Bool(b),
            ParamKind::Choice(_, i) => ParamValue::Num(i as f64),
        }
    }
}

/// Whether something with this visibility applies to the current values.
pub fn is_visible(when: Visibility, set: &ParamSet) -> bool {
    let holds = |c: &Condition| match set.get(c.key) {
        Some(ParamValue::Bool(b)) => (usize::from(b) == c.value) != c.not,
        Some(ParamValue::Num(v)) => (v == c.value as f64) != c.not,
        None => true,
    };
    when.is_empty() || when.iter().any(|all| all.iter().all(holds))
}

/// A button that sets some parameters at once (and has no state of its own).
#[derive(Clone, Copy, Debug, Serialize)]
pub struct Preset {
    pub id: &'static str,
    pub label: &'static str,
    pub values: &'static [(&'static str, ParamValue)],
}

/// One way of solving for a dimension (synthesis).
#[derive(Clone, Copy, Debug, Serialize)]
pub struct SynthTarget {
    pub id: &'static str,
    pub label: &'static str,
    /// The impedance the target value is for ("Z0", "Zdiff").
    pub quantity: &'static str,
    /// A second impedance that is a target at the same time ("Zcomm"), for a search in two dimensions.
    pub second: Option<&'static str>,
    pub visible_when: Visibility,
}

// ---------------------------------------------------------------------------------------------
// Results
// ---------------------------------------------------------------------------------------------

/// Share of the flux / return current on one ground conductor.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SplitRow {
    pub key: String,
    pub label: String,
    /// Nesting level for display (0 = top level, 1 = part of the row above).
    pub indent: u8,
    pub field_pct: f64,
    pub current_pct: f64,
}

/// Return-current density along x on one conductor face, J_s / I [1/mm].
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CurrentProfile {
    pub key: String,
    pub label: String,
    pub share_pct: f64,
    pub x: Vec<f64>,
    pub j: Vec<f64>,
    /// Index into the frontend's colour cycle; profiles of one conductor share it.
    pub color: usize,
    pub dashed: bool,
    /// Draw the left and right halves as separate lines (the profile is not defined across the signal).
    pub break_at_zero: bool,
}

/// Hints for plotting the current profiles.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ProfilePlot {
    pub title: String,
    /// The plot shows x in -half_width..half_width [mm].
    pub half_width: f64,
    /// Shaded x ranges (the signal traces).
    pub signals: Vec<[f64; 2]>,
    /// Dotted vertical lines [mm].
    pub markers: Vec<f64>,
    /// Texts at the top of the plot: (x [mm], text).
    pub labels: Vec<(f64, String)>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SplitSummary {
    pub rows: Vec<SplitRow>,
    /// Extra statements such as "90 % of plane current within |x| <= 0.305 mm" (label, mm).
    pub x90: Vec<(String, f64)>,
    pub profiles: Vec<CurrentProfile>,
    pub plot: ProfilePlot,
    /// Explanation shown below the table.
    pub info: String,
}

/// Result for one propagation mode: one for a single trace, the odd and the even mode for a pair. Of a pair the
/// values are those of one trace while both are driven.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ModeResult {
    pub name: String,
    pub z0: f64,
    pub eps_eff: f64,
    /// Per metre.
    pub l: f64,
    pub c: f64,
    /// Analysis frequency [Hz].
    pub f_hz: f64,
    /// Guided wavelength at the analysis frequency [mm].
    pub wavelength_mm: f64,
    /// Electrical length of the line for the entered length [deg].
    pub phase_deg: f64,
    /// Values at the analysis frequency.
    pub at_f: LossPoint,
    pub split: Option<SplitSummary>,
}

/// What belongs to a differential pair as a whole.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PairResult {
    /// Between the two traces, driven against each other: twice the odd-mode impedance [Ohm].
    pub z_diff: f64,
    /// Both traces in parallel against ground: half the even-mode impedance [Ohm].
    pub z_comm: f64,
    /// `(Zeven - Zodd) / (Zeven + Zodd)`.
    pub coupling: f64,
}

/// Geometry to draw over a field plot; colours are the frontend's business.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum ShapeKind {
    Signal,
    Ground,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct DrawRect {
    /// For a trapezoid (etched copper): `[x0, x1]` of the face at `y1`; `x0`, `x1` are then those at `y0`.
    pub top: Option<[f64; 2]>,
    pub kind: ShapeKind,
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

/// (x0, x1, y0, y1) in mm.
pub type Bounds = [f64; 4];

/// Potentials and geometry for the field views. Fields are for 1 V on the signal.
#[derive(Clone, Debug)]
pub struct FieldData {
    pub x: Vec<f64>,
    pub y: Vec<f64>,
    /// Conductor nodes.
    pub fixed: Vec<bool>,
    /// Relative permittivity per cell, `(nx-1) * (ny-1)`.
    pub er: Vec<f64>,
    /// Potential with dielectrics / in air.
    pub v: Vec<f64>,
    pub v0: Vec<f64>,
    pub conductors: Vec<DrawRect>,
    /// y positions of dashed guide lines (board surface where a plane is missing).
    pub dashed_y: Vec<f64>,
    /// y positions of dotted lines (copper surfaces).
    pub dotted_y: Vec<f64>,
    pub view_zoom: Bounds,
    pub view_full: Bounds,
    /// How the signals are driven in these fields, with the impedance of that mode, for the title.
    pub drive: String,
    /// Solution on a triangle mesh (finite elements). Then `x` and `y` hold only the extent of the mesh, the
    /// grid arrays above are empty, and the methods below and the raster functions work on the mesh.
    pub mesh: Option<Box<MeshFieldData>>,
}

impl FieldData {
    /// Field data of a mesh solution.
    pub fn from_mesh(m: MeshFieldData) -> Self {
        Self {
            x: vec![m.bounds[0], m.bounds[1]],
            y: vec![m.bounds[2], m.bounds[3]],
            fixed: Vec::new(),
            er: Vec::new(),
            v: Vec::new(),
            v0: Vec::new(),
            conductors: m.conductors.clone(),
            dashed_y: m.dashed_y.clone(),
            dotted_y: m.dotted_y.clone(),
            view_zoom: m.view_zoom,
            view_full: m.view_full,
            drive: m.drive.clone(),
            mesh: Some(Box::new(m)),
        }
    }

    /// Edges of the triangles that touch `view`, as `[x0, y0, x1, y1]` (`None` for a grid solution, whose lines are
    /// the coordinates `x` and `y`).
    pub fn mesh_edges(&self, view: Bounds) -> Option<Vec<[f64; 4]>> {
        self.mesh.as_ref().map(|m| m.edge_segments(view))
    }

    pub fn nx(&self) -> usize {
        self.x.len()
    }

    pub fn ny(&self) -> usize {
        self.y.len()
    }

    /// E field of the dielectric (`air = false`) or air solution.
    pub fn e_field(&self, air: bool) -> EField {
        if self.mesh.is_some() {
            return EField { ex: Vec::new(), ey: Vec::new(), e: Vec::new() };
        }
        postproc::e_field(if air { &self.v0 } else { &self.v }, &self.x, &self.y, &self.fixed)
    }

    /// The potentials of the equipotentials to draw [V]: in tenths between ground and the signal, or in fifths
    /// from -1 V to 1 V where signals of both polarities are driven.
    pub fn equipotential_levels(&self) -> Vec<f64> {
        let v = self.mesh.as_ref().map_or(&self.v, |m| &m.v);
        if v.iter().any(|&v| v < -0.5) {
            (-4..=4).map(|k| f64::from(k) / 5.0).collect()
        } else {
            (1..=9).map(|k| f64::from(k) / 10.0).collect()
        }
    }

    /// Equipotential segments for the given potentials (V).
    pub fn equipotentials(&self, levels: &[f64]) -> Vec<(f64, Vec<[[f64; 2]; 2]>)> {
        if let Some(m) = &self.mesh {
            return m.equipotentials(levels);
        }
        levels.iter().map(|&l| (l, postproc::contour_segments(&self.v, &self.x, &self.y, l))).collect()
    }

    /// Field lines of the dielectric solution inside `view`.
    pub fn field_lines(&self, view: Bounds, density: (f64, f64)) -> Vec<Vec<[f64; 2]>> {
        if let Some(m) = &self.mesh {
            return m.field_lines(view, density);
        }
        let e = self.e_field(false);
        postproc::field_lines(&self.x, &self.y, &e.ex, &e.ey, &self.fixed, &StreamOptions { view, density })
    }
}

/// Everything a frontend shows after an analysis.
pub struct Analysis {
    /// The mode a line is designed for first: the odd (differential) one of a pair.
    pub modes: Vec<ModeResult>,
    pub pair: Option<PairResult>,
    /// Formatted text of all results.
    pub report: String,
    /// Frequency-dependent values of the first mode.
    pub freq: Box<dyn FreqResponse>,
    pub field: Option<FieldData>,
    pub accuracy: Option<Accuracy>,
}

// ---------------------------------------------------------------------------------------------
// Cross-section sketch
// ---------------------------------------------------------------------------------------------

/// Semantic colours of the sketch; the frontend maps them to its palette.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SchemColor {
    UpperDielectric,
    LowerDielectric,
    CustomGap,
    Conductor,
    ConductorEdge,
    Signal,
    Text,
    Muted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Anchor {
    Center,
    Left,
    Right,
}

/// Drawing primitive in sketch coordinates (y grows downwards, like a canvas).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum SchemItem {
    Rect {
        x0: f64,
        y0: f64,
        x1: f64,
        y1: f64,
        fill: SchemColor,
        outline: Option<SchemColor>,
    },
    Line {
        x0: f64,
        y0: f64,
        x1: f64,
        y1: f64,
        color: SchemColor,
        dashed: bool,
        arrows: bool,
    },
    /// A convex filled shape.
    Polygon {
        points: Vec<[f64; 2]>,
        fill: SchemColor,
    },
    Text {
        x: f64,
        y: f64,
        text: String,
        anchor: Anchor,
        bold: bool,
        color: SchemColor,
    },
}

/// A schematic (not to scale) of the line cross-section for the cross-section tab.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Schematic {
    pub width: f64,
    pub height: f64,
    pub items: Vec<SchemItem>,
}

// ---------------------------------------------------------------------------------------------
// The line-type interface
// ---------------------------------------------------------------------------------------------

pub trait LineModel: Send + Sync {
    fn id(&self) -> &'static str;
    fn name(&self) -> &'static str;
    fn params(&self) -> &'static [ParamSpec];
    fn synth_targets(&self) -> &'static [SynthTarget];

    /// Buttons for common set-ups, if the line type has any.
    fn presets(&self) -> &'static [Preset] {
        &[]
    }

    /// Defaults of all parameters.
    fn defaults(&self) -> ParamSet {
        let mut s = ParamSet::default();
        for p in self.params() {
            s.set(p.key, p.default_value());
        }
        s
    }

    /// Wavelength [mm] used for sizes given in λ, if the line type has one.
    fn wavelength_mm(&self, _p: &ParamSet) -> Option<f64> {
        None
    }

    /// Sketch of the cross-section for the current form values (None if the line type has none).
    fn schematic(&self, _p: &ParamSet) -> Option<Schematic> {
        None
    }

    fn analyze(&self, p: &ParamSet, ctl: &Ctl) -> TvResult<Analysis>;

    /// Find the dimension `target_id` that gives `z0_target` Ohm; returns the new parameters and the
    /// analysis at that point. A target with a [`SynthTarget::second`] quantity takes its value in `second`
    /// and finds two dimensions. With `phase_deg` the line length is set as well, so that the line has
    /// that electrical length at the analysis frequency: the length follows from the solution
    /// (through eps_eff), it is never a search variable of its own.
    fn synthesize(
        &self,
        p: &ParamSet,
        target_id: &str,
        z0_target: f64,
        second: Option<f64>,
        phase_deg: Option<f64>,
        ctl: &Ctl,
    ) -> TvResult<(ParamSet, Analysis)>;
}
