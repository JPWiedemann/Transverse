//! CPWG inner-layer line as a [`LineModel`]: parameter form, conversion and generic results.

use transverse_core::units::format_g;
use transverse_core::{Ctl, TvError, TvResult, guided_wavelength_mm, phase_deg};

use super::{
    CpwgAnalysis, CpwgParams, DEFAULT_RTOL, GapMode, GroundCat, MeshMode, SchematicFlags, SynthVar,
    analysis_from_solution, field_data, full_analysis, report, schematic, synthesize, synthesize_length,
};
use crate::model::*;

const GEO: &str = "Geometry";
const UP: &str = "Upper dielectric / plane 1";
const LO: &str = "Lower dielectric / plane 2";
const GAP: &str = "Gap material (signal - coplanar ground)";
const COND: &str = "Conductor";
const FREQ: &str = "Frequency / length";
const MESH: &str = "Mesh";

const fn spec(key: &'static str, label: &'static str, group: &'static str, kind: ParamKind) -> ParamSpec {
    ParamSpec { key, label, group, kind, enable_when: None }
}

const fn when(mut s: ParamSpec, key: &'static str, value: usize) -> ParamSpec {
    s.enable_when = Some(EnableWhen { key, value });
    s
}

const fn len(mm: f64, unit: &'static str) -> ParamKind {
    ParamKind::Length { default_mm: mm, unit, lambda: false }
}

const fn num(default: f64, unit: &'static str) -> ParamKind {
    ParamKind::Number { default, unit, scale: 1.0 }
}

const GAP_CHOICES: &[&str] = &["upper", "lower", "custom"];
const MESH_CHOICES: &[&str] = &["Auto (target error)", "Manual"];

/// Parameter form of the CPWG line (defaults equal [`CpwgParams::default`]).
static SPECS: [ParamSpec; 28] = [
    spec("w", "Trace width w", GEO, len(0.3605, "mm")),
    spec("s", "Gap s", GEO, len(0.11, "mm")),
    spec("t", "Copper thickness t", GEO, len(0.018, "µm")),
    spec("g", "Via fence distance g", GEO, len(1.0, "mm")),
    spec("h1", "Height h1", UP, len(0.204, "mm")),
    spec("er1", "er1", UP, num(3.52, "")),
    spec("tand1", "tan d1", UP, num(0.02, "")),
    spec("plane1", "Ground plane 1 (top)", UP, ParamKind::Bool(true)),
    spec("fence1", "Via fence 1 (coplanar to top)", UP, ParamKind::Bool(true)),
    spec("h2", "Height h2", LO, len(0.254, "mm")),
    spec("er2", "er2", LO, num(3.66, "")),
    spec("tand2", "tan d2", LO, num(0.02, "")),
    spec("plane2", "Ground plane 2 (bottom)", LO, ParamKind::Bool(true)),
    spec("fence2", "Via fence 2 (coplanar to bottom)", LO, ParamKind::Bool(true)),
    spec("gap_mode", "Material", GAP, ParamKind::Choice(GAP_CHOICES, 0)),
    when(spec("er_gap", "er gap (custom)", GAP, num(3.52, "")), "gap_mode", 2),
    when(spec("tand_gap", "tan d gap (custom)", GAP, num(0.02, "")), "gap_mode", 2),
    spec("sigma", "Conductivity", COND, ParamKind::Number { default: 5.8e7, unit: "MS/m", scale: 1e6 }),
    spec("rq", "Roughness Rq", COND, len(0.0, "µm")),
    spec("f", "Frequency", FREQ, ParamKind::Frequency { default_hz: 1e9, unit: "GHz" }),
    spec("length", "Line length", FREQ, len(100.0, "mm")),
    spec("mesh_mode", "Mode", MESH, ParamKind::Choice(MESH_CHOICES, 0)),
    spec("tol", "Target error", MESH, num(0.5, "%")),
    when(
        spec(
            "dmin",
            "Min. cell (edges)",
            MESH,
            ParamKind::Length { default_mm: 0.002, unit: "µm", lambda: true },
        ),
        "mesh_mode",
        1,
    ),
    when(
        spec(
            "dmax",
            "Max. cell (core)",
            MESH,
            ParamKind::Length { default_mm: 0.02, unit: "µm", lambda: true },
        ),
        "mesh_mode",
        1,
    ),
    when(
        spec("richardson", "Richardson (2nd level, half size)", MESH, ParamKind::Bool(true)),
        "mesh_mode",
        1,
    ),
    spec("max_nodes", "Max. nodes", MESH, num(600_000.0, "")),
    spec("margin", "Domain margin (0=auto)", MESH, len(0.0, "mm")),
];

const Z0_GOAL: (&str, &str, f64) = ("Z0", "Ohm", 50.0);

static SYNTH: [SynthTarget; 4] = [
    SynthTarget { id: "w", label: "w (s fixed)", goal: Z0_GOAL.0, unit: Z0_GOAL.1, default: Z0_GOAL.2 },
    SynthTarget { id: "s", label: "s (w fixed)", goal: Z0_GOAL.0, unit: Z0_GOAL.1, default: Z0_GOAL.2 },
    SynthTarget {
        id: "w_total",
        label: "w (2s+w fixed)",
        goal: Z0_GOAL.0,
        unit: Z0_GOAL.1,
        default: Z0_GOAL.2,
    },
    SynthTarget {
        id: "length",
        label: "line length (for a phase)",
        goal: "phase",
        unit: "deg",
        default: 90.0,
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

impl CpwgParams {
    /// Parameters from a form value set; missing keys keep their defaults.
    pub fn from_set(s: &ParamSet) -> TvResult<Self> {
        let d = Self::default();
        let gap_mode = [GapMode::Upper, GapMode::Lower, GapMode::Custom][get_choice(s, "gap_mode", 0, 3)?];
        let mesh_mode = [MeshMode::Auto, MeshMode::Manual][get_choice(s, "mesh_mode", 0, 2)?];
        let max_nodes = get_num(s, "max_nodes", d.max_nodes as f64)?;
        if max_nodes < 1.0 || max_nodes.fract() != 0.0 {
            return Err(TvError::InvalidParam("max_nodes must be a positive integer".into()));
        }
        Ok(Self {
            w: get_num(s, "w", d.w)?,
            s: get_num(s, "s", d.s)?,
            t: get_num(s, "t", d.t)?,
            g: get_num(s, "g", d.g)?,
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
            richardson: get_flag(s, "richardson", d.richardson)?,
            max_nodes: max_nodes as usize,
            margin: get_num(s, "margin", d.margin)?,
        })
    }

    pub fn to_set(&self) -> ParamSet {
        use ParamValue::{Bool, Num};
        let mut s = ParamSet::default();
        for (k, v) in [
            ("w", self.w),
            ("s", self.s),
            ("t", self.t),
            ("g", self.g),
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
            ("richardson", self.richardson),
        ] {
            s.set(k, Bool(v));
        }
        s.set("gap_mode", Num(self.gap_mode as usize as f64));
        s.set("mesh_mode", Num(self.mesh_mode as usize as f64));
        s
    }
}

fn split_summary(p: &CpwgParams, a: &CpwgAnalysis) -> SplitSummary {
    let (fd, cur) = (&a.split.field, &a.split.current);
    let row = |c: GroundCat, indent: u8| SplitRow {
        key: c.key().into(),
        label: c.label().into(),
        indent,
        field_pct: fd.get(c),
        current_pct: cur.get(c),
    };
    let mut rows = Vec::new();
    if p.plane1 {
        rows.push(row(GroundCat::Plane1, 0));
    }
    if p.plane2 {
        rows.push(row(GroundCat::Plane2, 0));
    }
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
    if p.fence1 {
        rows.push(row(GroundCat::Fence1, 0));
    }
    if p.fence2 {
        rows.push(row(GroundCat::Fence2, 0));
    }
    if !p.closed() {
        rows.push(row(GroundCat::Boundary, 0));
    }
    let mut x90 = Vec::new();
    if let Some(x) = a.split.x90_planes {
        x90.push(("90 % of plane current within |x| <=".to_string(), x));
    }
    x90.push(("90 % of coplanar current within (from the gap edge)".to_string(), a.split.x90_cop));

    let (w2, sw) = (p.w / 2.0, p.w / 2.0 + p.s);
    let plot = ProfilePlot {
        title: format!(
            "HF return-current density (coplanar gap flank {:.1} %, not shown as a curve)",
            cur.get(GroundCat::CopFlank)
        ),
        half_width: (sw + p.g.min(1.5 * sw)).min(p.geometry(a.sol.accuracy.margin).xd),
        signal: Some([-w2, w2]),
        markers: vec![-sw, -w2, w2, sw],
        labels: vec![(0.0, "signal".into()), (-(w2 + p.s / 2.0), "s".into()), (w2 + p.s / 2.0, "s".into())],
    };
    let delta_um = a.loss.at(p.f).delta * 1e6;
    let valid = p.t > 0.0 && delta_um * 1e-3 < p.t / 3.0;
    let mut info = String::new();
    if let Some(x) = a.split.x90_planes {
        info += &format!("90 % of the plane return current flows within |x| <= {x:.3} mm; ");
    }
    info += &format!(
        "90 % of the coplanar return current within {:.3} mm of the gap edge.\nE-field share = where the field lines end (solution with dielectrics). Return current = HF distribution (skin depth << t). Skin depth at {} GHz: {:.2} um -> {}",
        a.split.x90_cop,
        format_g(p.f / 1e9),
        delta_um,
        if valid { "valid." } else { "NOT well below t: the real current spreads more widely." }
    );
    if !p.closed() {
        info += "\nDomain boundary = flux / current reaching the outer edge of the computation domain (remote ground).";
    }

    let profile_label = |c: GroundCat| match c {
        GroundCat::CopTop => "Coplanar top face",
        GroundCat::CopBottom => "Coplanar bottom face",
        _ => c.label(),
    };
    let profiles = a
        .split
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

fn to_analysis(title: &str, p: &CpwgParams, a: CpwgAnalysis) -> Analysis {
    let wavelength_mm = guided_wavelength_mm(p.f, a.sol.eeff);
    let mode = ModeResult {
        name: "single".into(),
        z0: a.sol.z0,
        eps_eff: a.sol.eeff,
        l: a.sol.l,
        c: a.sol.c,
        f_hz: p.f,
        wavelength_mm,
        phase_deg: phase_deg(p.length, wavelength_mm),
        at_f: a.loss.at(p.f),
        split: Some(split_summary(p, &a)),
    };
    Analysis {
        report: report(title, p, &a),
        field: Some(field_data(p, &a.sol)),
        accuracy: Some(a.sol.accuracy.clone()),
        freq: Box::new(a.loss),
        modes: vec![mode],
    }
}

/// CPWG / stripline-type coplanar waveguide in a PCB inner layer.
pub struct CpwgInner;

impl LineModel for CpwgInner {
    fn id(&self) -> &'static str {
        "cpwg_inner"
    }

    fn name(&self) -> &'static str {
        "CPWG, inner layer"
    }

    fn params(&self) -> &'static [ParamSpec] {
        &SPECS
    }

    fn synth_targets(&self) -> &'static [SynthTarget] {
        &SYNTH
    }

    fn wavelength_mm(&self, p: &ParamSet) -> Option<f64> {
        let p = CpwgParams::from_set(p).ok()?;
        (p.f > 0.0 && p.er1 > 0.0 && p.er2 > 0.0).then(|| p.wavelength())
    }

    fn schematic(&self, p: &ParamSet) -> Option<Schematic> {
        Some(schematic(&SchematicFlags {
            plane1: get_flag(p, "plane1", true).ok()?,
            plane2: get_flag(p, "plane2", true).ok()?,
            fence1: get_flag(p, "fence1", true).ok()?,
            fence2: get_flag(p, "fence2", true).ok()?,
            gap_mode: [GapMode::Upper, GapMode::Lower, GapMode::Custom]
                [get_choice(p, "gap_mode", 0, 3).ok()?],
        }))
    }

    fn analyze(&self, p: &ParamSet, ctl: &Ctl) -> TvResult<Analysis> {
        let p = CpwgParams::from_set(p)?;
        let a = full_analysis(&p, ctl)?;
        Ok(to_analysis("Analysis", &p, a))
    }

    fn synthesize(
        &self,
        p: &ParamSet,
        target_id: &str,
        target_value: f64,
        ctl: &Ctl,
    ) -> TvResult<(ParamSet, Analysis)> {
        let t = SYNTH
            .iter()
            .find(|t| t.id == target_id)
            .ok_or_else(|| TvError::InvalidParam(format!("unknown synthesis target {target_id}")))?;
        let p = CpwgParams::from_set(p)?;
        if t.id == "length" {
            let (pp, a) = synthesize_length(&p, target_value, ctl)?;
            let title = format!(
                "Synthesis: phase = {} deg at {} GHz, vary {}",
                format_g(target_value),
                format_g(p.f / 1e9),
                t.label
            );
            return Ok((pp.to_set(), to_analysis(&title, &pp, a)));
        }
        let var = match t.id {
            "w" => SynthVar::W,
            "s" => SynthVar::S,
            _ => SynthVar::WTotal,
        };
        let (pp, sol) = synthesize(&p, target_value, var, DEFAULT_RTOL, ctl)?;
        let a = analysis_from_solution(&pp, sol, ctl)?;
        let title = format!("Synthesis: Z0 = {} Ohm, vary {}", format_g(target_value), t.label);
        let analysis = to_analysis(&title, &pp, a);
        // only the dimensions change; the domain margin found during synthesis stays internal
        let out = CpwgParams { w: pp.w, s: pp.s, ..p };
        Ok((out.to_set(), analysis))
    }
}
