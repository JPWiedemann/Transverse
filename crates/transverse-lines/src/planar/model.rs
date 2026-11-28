//! CPWG inner-layer line as a [`LineModel`]: parameter form, conversion and generic results.

use transverse_core::units::format_g;
use transverse_core::{Ctl, TvError, TvResult, guided_wavelength_mm, length_for_phase_mm, phase_deg};

use super::{
    DEFAULT_RTOL, EtchMode, GapMode, GroundCat, MeshMode, ModeKind, ModeValues, PlanarFemAnalysis,
    PlanarParams, SchematicFlags, Shares, Signals, Split, SynthVar, WideSide, analysis_from_fem,
    fem_accuracy, field_data_fem, full_analysis_fem, report_fem, schematic, synthesize_fem,
    synthesize_pair_fem,
};
use crate::model::*;
use transverse_core::losses::LossModel;

const STRUCTURE: &str = "Structure";
const GEO: &str = "Geometry";
const UP: &str = "Upper half";
const LO: &str = "Lower half";
const GAP: &str = "Material beside the trace";
const COND: &str = "Conductor";
const FREQ: &str = "Frequency / length";
const MESH: &str = "Mesh";

const fn spec(key: &'static str, label: &'static str, group: &'static str, kind: ParamKind) -> ParamSpec {
    ParamSpec { key, label, group, kind, visible_when: &[] }
}

const fn when(mut s: ParamSpec, visible_when: Visibility) -> ParamSpec {
    s.visible_when = visible_when;
    s
}

const fn is(key: &'static str, value: usize) -> Condition {
    Condition { key, value, not: false }
}

/// Copper has a thickness, and with it flanks that can be etched.
const THICK: Condition = Condition { key: "t", value: 0, not: true };

const COPLANAR: Visibility = &[&[is("coplanar", 1)]];
const SINGLE: Visibility = &[&[is("signals", 0)]];
const PAIR: Visibility = &[&[is("signals", 1)]];
const PAIR_COPLANAR: Visibility = &[&[is("signals", 1), is("coplanar", 1)]];
const SINGLE_COPLANAR: Visibility = &[&[is("signals", 0), is("coplanar", 1)]];
const UPPER: Visibility = &[&[is("upper", 1)]];
const OWN_UPPER_MATERIAL: Visibility = &[&[is("upper", 1), is("upper_same", 0)]];
const UPPER_FENCE: Visibility = &[&[is("upper", 1), is("coplanar", 1)]];
const SIDE_WALLS: Visibility = &[&[is("coplanar", 0), is("upper", 1), is("plane1", 1), is("plane2", 1)]];
/// The fence distance: next to a via fence on the coplanar ground, or next to a side wall.
const FENCE_DISTANCE: Visibility = &[
    &[is("coplanar", 1), is("upper", 1), is("fence1", 1)],
    &[is("coplanar", 1), is("fence2", 1)],
    &[is("coplanar", 0), is("upper", 1), is("plane1", 1), is("plane2", 1), is("side_walls", 1)],
];
const ETCHING: Visibility = &[&[THICK]];
const ETCHED: Visibility = &[&[THICK, is("etch", 1)], &[THICK, is("etch", 2)]];
const ETCH_FACTOR: Visibility = &[&[THICK, is("etch", 1)]];
const ETCH_DELTA: Visibility = &[&[THICK, is("etch", 2)]];
const CUSTOM_GAP: Visibility = &[&[is("gap_mode", 2)]];
const MANUAL_MESH: Visibility = &[&[is("mesh_mode", 1)]];

const fn len(mm: f64, unit: &'static str) -> ParamKind {
    ParamKind::Length { default_mm: mm, unit, lambda: false }
}

const fn mesh_len(mm: f64) -> ParamKind {
    ParamKind::Length { default_mm: mm, unit: "µm", lambda: true }
}

const fn num(default: f64, unit: &'static str) -> ParamKind {
    ParamKind::Number { default, unit, scale: 1.0 }
}

const SIGNAL_CHOICES: &[&str] = &["Single", "Differential pair (edge-coupled)"];
const GAP_CHOICES: &[&str] = &["upper (or air)", "lower", "custom"];
const ETCH_CHOICES: &[&str] = &["Rectangular", "Etch factor", "Width difference"];
const WIDE_CHOICES: &[&str] = &["bottom", "top"];
const MESH_CHOICES: &[&str] = &["Auto (target error)", "Manual"];

/// Parameter form of the planar line (defaults equal [`PlanarParams::default`]).
static SPECS: [ParamSpec; 37] = [
    spec("signals", "Signals", STRUCTURE, ParamKind::Choice(SIGNAL_CHOICES, 0)),
    spec("coplanar", "Coplanar ground", STRUCTURE, ParamKind::Bool(true)),
    spec("upper", "Upper half (else air above)", STRUCTURE, ParamKind::Bool(true)),
    spec("w", "Trace width w", GEO, len(0.3605, "mm")),
    when(spec("d", "Pair gap d (edge to edge)", GEO, len(0.2, "mm")), PAIR),
    when(spec("s", "Gap s", GEO, len(0.11, "mm")), COPLANAR),
    spec("t", "Copper thickness t", GEO, len(0.018, "µm")),
    when(spec("etch", "Etched flanks", GEO, ParamKind::Choice(ETCH_CHOICES, 0)), ETCHING),
    when(spec("etch_factor", "Etch factor (t / undercut per side)", GEO, num(2.0, "")), ETCH_FACTOR),
    when(spec("etch_delta", "Width difference (wide - narrow)", GEO, len(0.02, "µm")), ETCH_DELTA),
    when(spec("etch_wide", "Wide side (w, s, g are there)", GEO, ParamKind::Choice(WIDE_CHOICES, 0)), ETCHED),
    when(spec("side_walls", "Via fences (plane to plane)", GEO, ParamKind::Bool(false)), SIDE_WALLS),
    when(spec("g", "Via fence distance g", GEO, len(1.0, "mm")), FENCE_DISTANCE),
    when(spec("upper_same", "Same er, tan d as lower", UP, ParamKind::Bool(false)), UPPER),
    when(spec("h1", "Height h1", UP, len(0.204, "mm")), UPPER),
    when(spec("er1", "er1", UP, num(3.52, "")), OWN_UPPER_MATERIAL),
    when(spec("tand1", "tan d1", UP, num(0.02, "")), OWN_UPPER_MATERIAL),
    when(spec("plane1", "Ground plane 1 (top)", UP, ParamKind::Bool(true)), UPPER),
    when(spec("fence1", "Via fence 1 (coplanar to top)", UP, ParamKind::Bool(true)), UPPER_FENCE),
    spec("h2", "Height h2", LO, len(0.254, "mm")),
    spec("er2", "er2", LO, num(3.66, "")),
    spec("tand2", "tan d2", LO, num(0.02, "")),
    spec("plane2", "Ground plane 2 (bottom)", LO, ParamKind::Bool(true)),
    when(spec("fence2", "Via fence 2 (coplanar to bottom)", LO, ParamKind::Bool(true)), COPLANAR),
    spec("gap_mode", "Material", GAP, ParamKind::Choice(GAP_CHOICES, 0)),
    when(spec("er_gap", "er gap (custom)", GAP, num(3.52, "")), CUSTOM_GAP),
    when(spec("tand_gap", "tan d gap (custom)", GAP, num(0.02, "")), CUSTOM_GAP),
    spec("sigma", "Conductivity", COND, ParamKind::Number { default: 5.8e7, unit: "MS/m", scale: 1e6 }),
    spec("rq", "Roughness Rq", COND, len(0.0, "µm")),
    spec("f", "Frequency", FREQ, ParamKind::Frequency { default_hz: 1e9, unit: "GHz" }),
    spec("length", "Line length", FREQ, len(100.0, "mm")),
    spec("mesh_mode", "Mode", MESH, ParamKind::Choice(MESH_CHOICES, 0)),
    spec("tol", "Target error", MESH, num(0.5, "%")),
    when(spec("dmin", "Min. cell (edges)", MESH, mesh_len(0.002)), MANUAL_MESH),
    when(spec("dmax", "Max. cell (core)", MESH, mesh_len(0.02)), MANUAL_MESH),
    spec("max_nodes", "Max. nodes", MESH, num(600_000.0, "")),
    spec("margin", "Domain margin (0=auto)", MESH, len(0.0, "mm")),
];

const fn target(
    id: &'static str,
    label: &'static str,
    quantity: &'static str,
    visible_when: Visibility,
) -> SynthTarget {
    SynthTarget { id, label, quantity, second: None, visible_when }
}

static SYNTH: [SynthTarget; 7] = [
    target("w", "w", "Z0", SINGLE),
    target("s", "s (w fixed)", "Z0", SINGLE_COPLANAR),
    target("w_total", "w (2s+w fixed)", "Z0", SINGLE_COPLANAR),
    target("pair_w", "w", "Zdiff", PAIR),
    target("d", "pair gap d (w fixed)", "Zdiff", PAIR),
    target("pair_s", "s (w fixed)", "Zdiff", PAIR_COPLANAR),
    SynthTarget {
        id: "pair_wd",
        label: "w and d (Zdiff + Zcomm)",
        quantity: "Zdiff",
        second: Some("Zcomm"),
        visible_when: PAIR,
    },
];

/// The settings of a preset: all structure switches, and the upper dielectric beside the trace (which is air
/// without an upper half). Dimensions and materials stay as they are.
macro_rules! structure {
    ($coplanar:expr, $upper:expr, $plane1:expr, $plane2:expr, $fence1:expr, $fence2:expr) => {
        &[
            ("coplanar", ParamValue::Bool($coplanar)),
            ("upper", ParamValue::Bool($upper)),
            ("plane1", ParamValue::Bool($plane1)),
            ("plane2", ParamValue::Bool($plane2)),
            ("fence1", ParamValue::Bool($fence1)),
            ("fence2", ParamValue::Bool($fence2)),
            ("side_walls", ParamValue::Bool(false)),
            ("gap_mode", ParamValue::Num(0.0)),
        ]
    };
}

static PRESETS: [Preset; 5] = [
    Preset {
        id: "cpwg_inner",
        label: "CPWG, inner layer",
        values: structure!(true, true, true, true, true, true),
    },
    Preset {
        id: "cpwg_outer",
        label: "CPWG, outer layer",
        values: structure!(true, false, false, true, false, true),
    },
    Preset {
        id: "cpw",
        label: "CPW, no planes",
        values: structure!(true, false, false, false, false, false),
    },
    Preset { id: "stripline", label: "Stripline", values: structure!(false, true, true, true, false, false) },
    Preset {
        id: "microstrip",
        label: "Microstrip",
        values: structure!(false, false, false, true, false, false),
    },
];

fn get_num(s: &ParamSet, k: &str, d: f64) -> TvResult<f64> {
    if s.get(k).is_none() { Ok(d) } else { s.num(k) }
}

fn get_flag(s: &ParamSet, k: &str, d: bool) -> TvResult<bool> {
    if s.get(k).is_none() { Ok(d) } else { s.flag(k) }
}

fn get_choice(s: &ParamSet, k: &str, d: usize, n: usize) -> TvResult<usize> {
    let i = if s.get(k).is_none() { d } else { s.choice(k)? };
    if i < n { Ok(i) } else { Err(TvError::InvalidParam(format!("{k}: choice {i} out of range"))) }
}

impl PlanarParams {
    /// Parameters from a form value set; missing keys keep their defaults.
    pub fn from_set(s: &ParamSet) -> TvResult<Self> {
        let d = Self::default();
        let gap_mode = [GapMode::Upper, GapMode::Lower, GapMode::Custom][get_choice(s, "gap_mode", 0, 3)?];
        let mesh_mode = [MeshMode::Auto, MeshMode::Manual][get_choice(s, "mesh_mode", 0, 2)?];
        let etch = [EtchMode::Rect, EtchMode::Factor, EtchMode::Delta][get_choice(s, "etch", 0, 3)?];
        let etch_wide = [WideSide::Bottom, WideSide::Top][get_choice(s, "etch_wide", 0, 2)?];
        let max_nodes = get_num(s, "max_nodes", d.max_nodes as f64)?;
        if max_nodes < 1.0 || max_nodes.fract() != 0.0 {
            return Err(TvError::InvalidParam("max_nodes must be a positive integer".into()));
        }
        let signals = [Signals::Single, Signals::Pair][get_choice(s, "signals", 0, 2)?];
        Ok(Self {
            signals,
            d: get_num(s, "d", d.d)?,
            w: get_num(s, "w", d.w)?,
            s: get_num(s, "s", d.s)?,
            t: get_num(s, "t", d.t)?,
            g: get_num(s, "g", d.g)?,
            etch,
            etch_factor: get_num(s, "etch_factor", d.etch_factor)?,
            etch_delta: get_num(s, "etch_delta", d.etch_delta)?,
            etch_wide,
            h1: get_num(s, "h1", d.h1)?,
            er1: get_num(s, "er1", d.er1)?,
            tand1: get_num(s, "tand1", d.tand1)?,
            plane1: get_flag(s, "plane1", d.plane1)?,
            fence1: get_flag(s, "fence1", d.fence1)?,
            h2: get_num(s, "h2", d.h2)?,
            er2: get_num(s, "er2", d.er2)?,
            tand2: get_num(s, "tand2", d.tand2)?,
            plane2: get_flag(s, "plane2", d.plane2)?,
            fence2: get_flag(s, "fence2", d.fence2)?,
            coplanar: get_flag(s, "coplanar", d.coplanar)?,
            upper: get_flag(s, "upper", d.upper)?,
            upper_same: get_flag(s, "upper_same", d.upper_same)?,
            side_walls: get_flag(s, "side_walls", d.side_walls)?,
            gap_mode,
            er_gap: get_num(s, "er_gap", d.er_gap)?,
            tand_gap: get_num(s, "tand_gap", d.tand_gap)?,
            sigma: get_num(s, "sigma", d.sigma)?,
            rq: get_num(s, "rq", d.rq)?,
            f: get_num(s, "f", d.f)?,
            length: get_num(s, "length", d.length)?,
            mesh_mode,
            tol: get_num(s, "tol", d.tol)?,
            dmin: get_num(s, "dmin", d.dmin)?,
            dmax: get_num(s, "dmax", d.dmax)?,
            // only the grid solver uses it (the reference of the tests): not a form field
            richardson: d.richardson,
            max_nodes: max_nodes as usize,
            margin: get_num(s, "margin", d.margin)?,
        })
    }

    pub fn to_set(&self) -> ParamSet {
        use ParamValue::{Bool, Num};
        let mut s = ParamSet::default();
        for (k, v) in [
            ("d", self.d),
            ("w", self.w),
            ("s", self.s),
            ("t", self.t),
            ("g", self.g),
            ("etch_factor", self.etch_factor),
            ("etch_delta", self.etch_delta),
            ("h1", self.h1),
            ("er1", self.er1),
            ("tand1", self.tand1),
            ("h2", self.h2),
            ("er2", self.er2),
            ("tand2", self.tand2),
            ("er_gap", self.er_gap),
            ("tand_gap", self.tand_gap),
            ("sigma", self.sigma),
            ("rq", self.rq),
            ("f", self.f),
            ("length", self.length),
            ("tol", self.tol),
            ("dmin", self.dmin),
            ("dmax", self.dmax),
            ("max_nodes", self.max_nodes as f64),
            ("margin", self.margin),
        ] {
            s.set(k, Num(v));
        }
        for (k, v) in [
            ("plane1", self.plane1),
            ("fence1", self.fence1),
            ("plane2", self.plane2),
            ("fence2", self.fence2),
            ("coplanar", self.coplanar),
            ("upper", self.upper),
            ("upper_same", self.upper_same),
            ("side_walls", self.side_walls),
        ] {
            s.set(k, Bool(v));
        }
        s.set("signals", Num(self.signals as usize as f64));
        s.set("gap_mode", Num(self.gap_mode as usize as f64));
        s.set("mesh_mode", Num(self.mesh_mode as usize as f64));
        s.set("etch", Num(self.etch as usize as f64));
        s.set("etch_wide", Num(self.etch_wide as usize as f64));
        s
    }
}

fn split_summary(
    p: &PlanarParams,
    split: &Split,
    loss: &LossModel,
    margin: f64,
    mode: ModeKind,
) -> SplitSummary {
    let e = &p.effective();
    let (fd, cur) = (&split.field, &split.current);
    let row = |c: GroundCat, indent: u8| SplitRow {
        key: c.key().into(),
        label: c.label().into(),
        indent,
        field_pct: fd.get(c),
        current_pct: cur.get(c),
    };
    let mut rows = Vec::new();
    if e.plane1 {
        rows.push(row(GroundCat::Plane1, 0));
    }
    if e.plane2 {
        rows.push(row(GroundCat::Plane2, 0));
    }
    if e.coplanar {
        rows.push(SplitRow {
            key: "coplanar".into(),
            label: "Coplanar ground (total)".into(),
            indent: 0,
            field_pct: fd.coplanar,
            current_pct: cur.coplanar,
        });
        for c in GroundCat::COPLANAR {
            if c == GroundCat::CopOuter && fd.get(c) < 0.05 && cur.get(c) < 0.05 {
                continue;
            }
            rows.push(row(c, 1));
        }
    }
    if e.fence1 {
        rows.push(row(GroundCat::Fence1, 0));
    }
    if e.fence2 {
        rows.push(row(GroundCat::Fence2, 0));
    }
    if e.side_walls {
        // one wall from plane to plane; the split keeps its two halves apart
        let sum = |s: &Shares| s.get(GroundCat::Fence1) + s.get(GroundCat::Fence2);
        rows.push(SplitRow {
            key: "walls".into(),
            label: "Via fences".into(),
            indent: 0,
            field_pct: sum(fd),
            current_pct: sum(cur),
        });
    }
    if !p.closed() {
        rows.push(row(GroundCat::Boundary, 0));
    }
    let mut x90 = Vec::new();
    if let Some(x) = split.x90_planes {
        x90.push(("90 % of plane current within |x| <=".to_string(), x));
    }
    if e.coplanar {
        x90.push(("90 % of coplanar current within (from the gap edge)".to_string(), split.x90_cop));
    }

    // the outer edge of the traces
    let w2 = p.trace_half_width();
    let xd = p.geometry(margin).xd;
    let (signals, mut markers, mut labels) = if p.pair() {
        let (inner, centre) = (p.d / 2.0, p.d / 2.0 + p.w / 2.0);
        (
            vec![[-w2, -inner], [inner, w2]],
            vec![-w2, -inner, inner, w2],
            vec![(-centre, "signal 1".to_string()), (centre, "signal 2".to_string()), (0.0, "d".to_string())],
        )
    } else {
        (vec![[-w2, w2]], vec![-w2, w2], vec![(0.0, "signal".to_string())])
    };
    let name = match mode {
        ModeKind::Single => "HF return-current density".to_string(),
        _ => format!("{}: ground return-current density", mode.label()),
    };
    let (title, half_width) = if e.coplanar {
        let sw = w2 + p.s;
        markers.insert(0, -sw);
        markers.push(sw);
        labels.extend([(-(w2 + p.s / 2.0), "s".into()), (w2 + p.s / 2.0, "s".into())]);
        let title = format!(
            "{name} (coplanar gap flank {:.1} %, not shown as a curve)",
            cur.get(GroundCat::CopFlank)
        );
        (title, (sw + p.g.min(1.5 * sw)).min(xd))
    } else {
        (name, (w2 + p.w.max(2.0 * e.h1.max(e.h2))).min(xd))
    };
    let plot = ProfilePlot { title, half_width, signals, markers, labels };
    let delta_um = loss.at(p.f).delta * 1e6;
    let valid = p.t > 0.0 && delta_um * 1e-3 < p.t / 3.0;
    let mut info = String::new();
    if let Some(x) = split.x90_planes {
        info += &format!("90 % of the plane return current flows within |x| <= {x:.3} mm; ");
    }
    if e.coplanar {
        info +=
            &format!("90 % of the coplanar return current within {:.3} mm of the gap edge.\n", split.x90_cop);
    }
    info += &format!(
        "E-field share = where the field lines end (solution with dielectrics). Return current = HF distribution (skin depth << t). Skin depth at {} GHz: {:.2} um -> {}",
        format_g(p.f / 1e9),
        delta_um,
        if valid { "valid." } else { "NOT well below t: the real current spreads more widely." }
    );
    if mode == ModeKind::Odd {
        info += "\nOdd mode: part of each trace's current returns in the other trace; the shares divide what returns in the grounds.";
    }
    if !p.closed() {
        info += "\nDomain boundary = flux / current reaching the outer edge of the computation domain (remote ground).";
    }

    let profile_label = |c: GroundCat| match c {
        GroundCat::CopTop => "Coplanar top face",
        GroundCat::CopBottom => "Coplanar bottom face",
        _ => c.label(),
    };
    let profiles = split
        .profiles
        .iter()
        .map(|pr| CurrentProfile {
            key: pr.cat.key().into(),
            label: profile_label(pr.cat).into(),
            share_pct: cur.get(pr.cat),
            x: pr.x.clone(),
            j: pr.js.clone(),
            color: match pr.cat {
                GroundCat::Plane1 => 0,
                GroundCat::Plane2 => 1,
                _ => 2,
            },
            dashed: pr.cat == GroundCat::CopBottom,
            break_at_zero: matches!(pr.cat, GroundCat::CopTop | GroundCat::CopBottom),
        })
        .collect();
    SplitSummary { rows, x90, profiles, plot, info }
}

/// The result of one mode.
fn mode_result(
    p: &PlanarParams,
    mode: &ModeValues,
    loss: &LossModel,
    split: &Split,
    margin: f64,
) -> ModeResult {
    let wavelength_mm = guided_wavelength_mm(p.f, mode.eeff);
    ModeResult {
        name: mode.kind.key().into(),
        z0: mode.z0,
        eps_eff: mode.eeff,
        l: mode.l,
        c: mode.c,
        f_hz: p.f,
        wavelength_mm,
        phase_deg: phase_deg(p.length, wavelength_mm),
        at_f: loss.at(p.f),
        split: Some(split_summary(p, split, loss, margin, mode.kind)),
    }
}

fn to_analysis(title: &str, p: &PlanarParams, a: PlanarFemAnalysis) -> TvResult<Analysis> {
    let s = &a.sol;
    let mut modes = vec![mode_result(p, &s.modes[0], &a.loss, &a.split, s.margin)];
    if let Some(even) = &a.even {
        modes.push(mode_result(p, &s.modes[1], &even.loss, &even.split, s.margin));
    }
    let pair = s.common_impedance().zip(s.coupling()).map(|(z_comm, coupling)| PairResult {
        z_diff: s.target_impedance(),
        z_comm,
        coupling,
    });
    Ok(Analysis {
        modes,
        pair,
        report: report_fem(title, p, &a),
        field: Some(FieldData::from_mesh(field_data_fem(p, s)?)),
        accuracy: Some(fem_accuracy(s)),
        freq: std::iter::once(a.loss)
            .chain(a.even.as_ref().map(|even| even.loss))
            .map(|loss| Box::new(loss) as _)
            .collect(),
    })
}

/// Planar transmission lines, solved with finite elements.
pub struct PlanarLine;

impl LineModel for PlanarLine {
    fn id(&self) -> &'static str {
        "planar"
    }

    fn name(&self) -> &'static str {
        "Planar line"
    }

    fn params(&self) -> &'static [ParamSpec] {
        &SPECS
    }

    fn synth_targets(&self) -> &'static [SynthTarget] {
        &SYNTH
    }

    fn presets(&self) -> &'static [Preset] {
        &PRESETS
    }

    fn wavelength_mm(&self, p: &ParamSet) -> Option<f64> {
        let p = PlanarParams::from_set(p).ok()?;
        (p.f > 0.0 && p.er1 > 0.0 && p.er2 > 0.0).then(|| p.wavelength())
    }

    fn schematic(&self, p: &ParamSet) -> Option<Schematic> {
        let d = PlanarParams::default();
        let flag = |key, value| get_flag(p, key, value).ok();
        let etched = get_choice(p, "etch", 0, 3).ok()? != 0 && p.num("t").ok() != Some(0.0);
        let wide = [WideSide::Bottom, WideSide::Top][get_choice(p, "etch_wide", 0, 2).ok()?];
        let params = PlanarParams {
            signals: [Signals::Single, Signals::Pair][get_choice(p, "signals", 0, 2).ok()?],
            coplanar: flag("coplanar", d.coplanar)?,
            upper: flag("upper", d.upper)?,
            upper_same: flag("upper_same", d.upper_same)?,
            side_walls: flag("side_walls", d.side_walls)?,
            plane1: flag("plane1", d.plane1)?,
            plane2: flag("plane2", d.plane2)?,
            fence1: flag("fence1", d.fence1)?,
            fence2: flag("fence2", d.fence2)?,
            gap_mode: [GapMode::Upper, GapMode::Lower, GapMode::Custom]
                [get_choice(p, "gap_mode", 0, 3).ok()?],
            ..d
        };
        Some(schematic(&SchematicFlags { etched: etched.then_some(wide), ..SchematicFlags::of(&params) }))
    }

    fn analyze(&self, p: &ParamSet, ctl: &Ctl) -> TvResult<Analysis> {
        let p = PlanarParams::from_set(p)?;
        to_analysis("Analysis", &p, full_analysis_fem(&p, ctl)?)
    }

    fn synthesize(
        &self,
        p: &ParamSet,
        target_id: &str,
        z0_target: f64,
        second: Option<f64>,
        phase_deg: Option<f64>,
        ctl: &Ctl,
    ) -> TvResult<(ParamSet, Analysis)> {
        let t = SYNTH
            .iter()
            .find(|t| t.id == target_id)
            .ok_or_else(|| TvError::InvalidParam(format!("unknown synthesis target {target_id}")))?;
        if phase_deg.is_some_and(|ph| !(ph.is_finite() && ph > 0.0)) {
            return Err(TvError::Synthesis("the target phase must be a positive number of degrees".into()));
        }
        if !is_visible(t.visible_when, p) {
            return Err(TvError::Synthesis(format!("'{}' does not apply to this structure", t.label)));
        }
        let p = PlanarParams::from_set(p)?;
        let mut title = format!("Synthesis: {} = {} Ohm", t.quantity, format_g(z0_target));
        let (mut pp, sol) = if let Some(name) = t.second {
            let second = second.ok_or_else(|| TvError::Synthesis(format!("a target {name} is needed")))?;
            title += &format!(", {name} = {} Ohm", format_g(second));
            synthesize_pair_fem(&p, z0_target, second, DEFAULT_RTOL, ctl)?
        } else {
            let var = match t.id {
                "w" | "pair_w" => SynthVar::W,
                "d" => SynthVar::D,
                "s" | "pair_s" => SynthVar::S,
                _ => SynthVar::WTotal,
            };
            synthesize_fem(&p, z0_target, var, DEFAULT_RTOL, ctl)?
        };
        title += &format!(", vary {}", t.label);
        // the length follows from the solution: eps_eff does not depend on it
        if let Some(ph) = phase_deg {
            pp.length = length_for_phase_mm(pp.f, sol.eeff, ph);
            title += &format!(", phase = {} deg at {} GHz -> length", format_g(ph), format_g(pp.f / 1e9));
        }
        let analysis = to_analysis(&title, &pp, analysis_from_fem(&pp, sol, ctl)?)?;
        // only the dimensions (and the length, if a phase was asked for) change; the domain margin
        // found during synthesis stays internal
        let out = PlanarParams { w: pp.w, s: pp.s, d: pp.d, length: pp.length, ..p };
        Ok((out.to_set(), analysis))
    }
}
