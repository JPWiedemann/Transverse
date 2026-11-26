//! The generic line-model interface: parameter form, conversion, analysis, synthesis, field data.

use serde::Deserialize;
use transverse_core::Ctl;
use transverse_core::postproc::percentile;
use transverse_lines::cpwg_inner::{CpwgInner, CpwgParams};
use transverse_lines::model::{LineModel, ParamKind, ParamSet, ParamValue, ShapeKind, is_enabled};

fn rel(a: f64, b: f64) -> f64 {
    (a / b - 1.0).abs()
}

#[test]
fn defaults_equal_the_params_defaults_and_round_trip() {
    let m = CpwgInner;
    let set = m.defaults();
    assert_eq!(CpwgParams::from_set(&set).unwrap(), CpwgParams::default());
    assert_eq!(CpwgParams::default().to_set(), set);
}

#[test]
fn spec_keys_match_the_parameter_set_exactly() {
    let m = CpwgInner;
    let set = CpwgParams::default().to_set();
    let mut spec_keys: Vec<&str> = m.params().iter().map(|p| p.key).collect();
    let mut set_keys: Vec<&str> = set.0.keys().map(String::as_str).collect();
    spec_keys.sort_unstable();
    set_keys.sort_unstable();
    assert_eq!(spec_keys, set_keys);
    // dependencies point at existing choice/bool keys
    for p in m.params() {
        if let Some(dep) = p.enable_when {
            assert!(set.get(dep.key).is_some(), "{} depends on unknown {}", p.key, dep.key);
        }
        if let ParamKind::Choice(opts, def) = p.kind {
            assert!(def < opts.len());
        }
    }
}

#[test]
fn enable_when_follows_choices() {
    let m = CpwgInner;
    let mut set = m.defaults();
    let find = |k: &str| *m.params().iter().find(|p| p.key == k).unwrap();
    assert!(!is_enabled(&find("dmin"), &set) && !is_enabled(&find("richardson"), &set));
    assert!(!is_enabled(&find("er_gap"), &set));
    set.set("mesh_mode", ParamValue::Num(1.0));
    set.set("gap_mode", ParamValue::Num(2.0));
    assert!(is_enabled(&find("dmin"), &set) && is_enabled(&find("dmax"), &set));
    assert!(is_enabled(&find("richardson"), &set) && is_enabled(&find("tand_gap"), &set));
    assert!(is_enabled(&find("w"), &set));
}

#[test]
fn invalid_sets_are_rejected_and_partial_sets_use_defaults() {
    let mut set = ParamSet::default();
    assert_eq!(CpwgParams::from_set(&set).unwrap(), CpwgParams::default());
    set.set("w", ParamValue::Num(0.5));
    assert_eq!(CpwgParams::from_set(&set).unwrap().w, 0.5);
    for (k, v) in [
        ("gap_mode", ParamValue::Num(5.0)),
        ("gap_mode", ParamValue::Num(0.5)),
        ("plane1", ParamValue::Num(1.0)),
        ("w", ParamValue::Bool(true)),
        ("max_nodes", ParamValue::Num(0.0)),
    ] {
        let mut s = ParamSet::default();
        s.set(k, v);
        assert!(CpwgParams::from_set(&s).is_err(), "{k} = {v:?} accepted");
    }
}

#[test]
fn param_set_json_round_trip() {
    let set = CpwgInner.defaults();
    let text = serde_json::to_string(&set).unwrap();
    assert!(text.contains("\"plane1\":true") && text.contains("\"w\":0.3605"));
    assert_eq!(serde_json::from_str::<ParamSet>(&text).unwrap(), set);
}

#[test]
fn wavelength_follows_frequency_and_permittivity() {
    let m = CpwgInner;
    let l1 = m.wavelength_mm(&m.defaults()).unwrap();
    let mut s = m.defaults();
    s.set("f", ParamValue::Num(2e9));
    assert!(rel(m.wavelength_mm(&s).unwrap(), l1 / 2.0) < 1e-12);
    s.set("f", ParamValue::Num(0.0));
    assert!(m.wavelength_mm(&s).is_none());
}

#[test]
fn analysis_through_the_trait_matches_the_reference() {
    let a = CpwgInner.analyze(&CpwgInner.defaults(), &Ctl::new()).unwrap();
    assert_eq!(a.modes.len(), 1);
    let m = &a.modes[0];
    assert!(rel(m.z0, 35.001) < 5e-4 && rel(m.eps_eff, 3.58) < 5e-3);
    assert!(a.report.contains("Z0              =   35.001 Ohm"), "{}", a.report);
    let s = m.split.as_ref().unwrap();
    assert!(s.rows.iter().any(|r| r.key == "plane1" && (r.current_pct - 39.2).abs() < 0.1));
    assert!(s.profiles.iter().any(|p| p.key == "cop_top") && !s.x90.is_empty());
    // frequency response is the same model the mode result was taken from
    let at = a.freq.at(1e9);
    assert!(rel(at.alpha_c, m.at_f.alpha_c) < 1e-12 && rel(at.alpha_d, m.at_f.alpha_d) < 1e-12);
    assert_eq!(a.freq.sample(&[1e9, 2e9]).len(), 2);
    assert!(a.accuracy.unwrap().nodes > 1000);
}

#[test]
fn synthesis_through_the_trait_keeps_other_parameters() {
    let m = CpwgInner;
    let mut set = m.defaults();
    set.set("plane1", ParamValue::Bool(false));
    set.set("fence1", ParamValue::Bool(false));
    let (out, a) = m.synthesize(&set, "w", 50.0, &Ctl::new()).unwrap();
    assert!(rel(a.modes[0].z0, 50.0) < 5e-4);
    assert!(a.report.starts_with("=== Synthesis: Z0 = 50 Ohm, vary w (s fixed) ==="));
    // only w changed; in particular the automatically found margin is not written back
    let mut expect = set.clone();
    expect.set("w", out.get("w").unwrap());
    assert_eq!(out, expect);
    assert!((out.num("w").unwrap() - 0.2467).abs() < 1e-3);
    assert!(m.synthesize(&set, "nonsense", 50.0, &Ctl::new()).is_err());
}

#[test]
fn analysis_reports_wavelength_and_phase_of_the_entered_length() {
    let m = CpwgInner;
    let mut set = m.defaults();
    set.set("length", ParamValue::Num(50.0));
    let a = m.analyze(&set, &Ctl::new()).unwrap();
    let mode = &a.modes[0];
    let lambda = transverse_core::C_LIGHT / (1e9 * mode.eps_eff.sqrt()) * 1e3;
    assert!(rel(mode.wavelength_mm, lambda) < 1e-12);
    assert!(rel(mode.phase_deg, 360.0 * 50.0 / lambda) < 1e-12);
    // the report shows both, and the free-space wavelength for reference
    assert!(
        a.report.contains(&format!("Wavelength      = {lambda:8.3} mm  (free space 299.792 mm)")),
        "{}",
        a.report
    );
    assert!(a.report.contains(&format!("phase {:.2} deg", mode.phase_deg)), "{}", a.report);
}

#[test]
fn synthesis_target_metadata_says_what_value_to_enter() {
    let m = CpwgInner;
    let t = m.synth_targets();
    let goals: Vec<_> = t.iter().map(|t| (t.id, t.goal, t.unit)).collect();
    assert_eq!(
        goals,
        [("w", "Z0", "Ohm"), ("s", "Z0", "Ohm"), ("w_total", "Z0", "Ohm"), ("length", "phase", "deg")]
    );
    assert_eq!(t[3].default, 90.0);
}

#[test]
fn length_synthesis_gives_the_requested_phase_and_changes_only_the_length() {
    let m = CpwgInner;
    let mut set = m.defaults();
    set.set("f", ParamValue::Num(2.4e9));
    let (out, a) = m.synthesize(&set, "length", 90.0, &Ctl::new()).unwrap();
    let mode = &a.modes[0];
    // a quarter wavelength of the guided wave at 2.4 GHz
    let lambda = transverse_core::C_LIGHT / (2.4e9 * mode.eps_eff.sqrt()) * 1e3;
    assert!(rel(out.num("length").unwrap(), lambda / 4.0) < 1e-12);
    assert!(rel(mode.phase_deg, 90.0) < 1e-12);
    let mut expect = set.clone();
    expect.set("length", out.get("length").unwrap());
    assert_eq!(out, expect);
    assert!(
        a.report.starts_with("=== Synthesis: phase = 90 deg at 2.4 GHz, vary line length (for a phase) ===")
    );
    // more phase needs more length, linearly
    let (out2, _) = m.synthesize(&set, "length", 360.0, &Ctl::new()).unwrap();
    assert!(rel(out2.num("length").unwrap(), lambda) < 1e-12);
    // nonsense targets are rejected
    for bad in [0.0, -10.0, f64::NAN] {
        assert!(m.synthesize(&set, "length", bad, &Ctl::new()).is_err(), "{bad}");
    }
}

// ---- field data ------------------------------------------------------------------------------

#[derive(Deserialize)]
struct Cases {
    cases: Vec<Case>,
}
#[derive(Deserialize)]
struct Case {
    name: String,
    field: Option<FieldRef>,
}
#[derive(Deserialize)]
struct FieldRef {
    nx: usize,
    ny: usize,
    e_p997: f64,
    nodes: Vec<NodeRef>,
}
#[derive(Deserialize)]
struct NodeRef {
    i: usize,
    j: usize,
    v: f64,
    v0: f64,
    ex: Option<f64>,
    ey: Option<f64>,
    e: Option<f64>,
}

fn final_stackup_field() -> transverse_lines::model::FieldData {
    CpwgInner.analyze(&CpwgInner.defaults(), &Ctl::new()).unwrap().field.unwrap()
}

#[test]
fn e_field_matches_python_at_sample_nodes() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/reference/cases.json");
    let all: Cases = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let r = all.cases.into_iter().find(|c| c.name == "final_stackup").unwrap().field.unwrap();
    let f = final_stackup_field();
    assert_eq!((f.nx(), f.ny()), (r.nx, r.ny));
    let e = f.e_field(false);
    for n in &r.nodes {
        let k = n.j * f.nx() + n.i;
        assert!((f.v[k] - n.v).abs() < 1e-9 && (f.v0[k] - n.v0).abs() < 1e-9, "potential at {},{}", n.i, n.j);
        match (n.e, n.ex, n.ey) {
            (Some(re), Some(rx), Some(ry)) => {
                assert!(rel(e.e[k], re) < 1e-6, "|E| at {},{}: {} vs {re}", n.i, n.j, e.e[k]);
                assert!((e.ex[k] - rx).abs() < 1e-6 * re && (e.ey[k] - ry).abs() < 1e-6 * re);
            }
            _ => assert!(e.e[k].is_nan() && f.fixed[k], "conductor node {},{}", n.i, n.j),
        }
    }
    assert!(rel(percentile(e.e.iter().copied(), 99.7).unwrap(), r.e_p997) < 1e-9);
}

#[test]
fn field_data_geometry_for_the_default_stackup() {
    let f = final_stackup_field();
    // signal + two coplanar halves + two planes + four fences
    assert_eq!(f.conductors.len(), 9);
    assert_eq!(f.conductors.iter().filter(|c| c.kind == ShapeKind::Signal).count(), 1);
    assert!(f.dashed_y.is_empty() && f.dotted_y == vec![0.0, 0.018]);
    let [zx0, zx1, zy0, zy1] = f.view_zoom;
    let [fx0, fx1, fy0, fy1] = f.view_full;
    assert!(zx0 == -zx1 && fx0 == -fx1);
    assert!(fx1 >= zx1 && fy0 <= zy0 && fy1 >= zy1);
    assert!(zy0 < -0.254 && zy1 > 0.018 + 0.204);
    // open variant: missing planes show dashed board-surface lines and the domain is extended
    let mut s = CpwgInner.defaults();
    s.set("plane1", ParamValue::Bool(false));
    s.set("fence1", ParamValue::Bool(false));
    let o = CpwgInner.analyze(&s, &Ctl::new()).unwrap().field.unwrap();
    assert_eq!(o.dashed_y.len(), 1);
    assert!(o.view_full[3] > f.view_full[3]);
}

#[test]
fn equipotentials_and_field_lines_of_a_real_solution() {
    let f = final_stackup_field();
    let eq = f.equipotentials(&[0.1, 0.5, 0.9]);
    assert!(eq.iter().all(|(_, segs)| !segs.is_empty()));
    // every segment end lies on the requested potential (checked at the nearest cell corners' range)
    let (zx0, zx1, zy0, zy1) = (f.view_zoom[0], f.view_zoom[1], f.view_zoom[2], f.view_zoom[3]);
    let lines = f.field_lines(f.view_zoom, (2.2, 1.4));
    assert!(lines.len() > 30, "only {} lines", lines.len());
    let nx = f.nx();
    let nearest = |c: &[f64], v: f64| {
        c.iter().enumerate().min_by(|a, b| (a.1 - v).abs().total_cmp(&(b.1 - v).abs())).unwrap().0
    };
    for l in &lines {
        for p in l {
            assert!(p[0] >= zx0 && p[0] <= zx1 && p[1] >= zy0 && p[1] <= zy1);
            assert!(!f.fixed[nearest(&f.y, p[1]) * nx + nearest(&f.x, p[0])], "line enters a conductor");
        }
        // lines follow E = -grad V: the potential never rises along a line (small slack for grid noise)
        let v = |p: &[f64; 2]| f.v[nearest(&f.y, p[1]) * nx + nearest(&f.x, p[0])];
        assert!(v(&l[l.len() - 1]) <= v(&l[0]) + 0.02, "line runs against the field");
    }
}

#[test]
fn schematic_follows_the_form_values() {
    use transverse_lines::model::SchemItem;
    let m = CpwgInner;
    let mut set = m.defaults();
    let all = m.schematic(&set).unwrap();
    assert_eq!((all.width, all.height), (700.0, 340.0));
    set.set("plane1", ParamValue::Bool(false));
    set.set("gap_mode", ParamValue::Num(2.0));
    let s = m.schematic(&set).unwrap();
    assert_ne!(s, all);
    assert!(
        s.items
            .iter()
            .any(|i| matches!(i, SchemItem::Text { text, .. } if text == "no ground plane 1  (air)"))
    );
    // wrong kinds give no sketch instead of a panic
    set.set("fence2", ParamValue::Num(1.0));
    assert!(m.schematic(&set).is_none());
}

#[test]
fn split_summary_carries_what_the_return_current_tab_needs() {
    let m = CpwgInner;
    let a = m.analyze(&m.defaults(), &Ctl::new()).unwrap();
    assert_eq!(a.modes[0].f_hz, 1e9);
    let s = a.modes[0].split.as_ref().unwrap();
    // plot hints: signal span, the four dotted lines and the x range of the Python plot
    assert_eq!(s.plot.signal, Some([-0.18025, 0.18025]));
    assert_eq!(s.plot.markers.len(), 4);
    let sw = 0.3605 / 2.0 + 0.11;
    assert!((s.plot.half_width - (sw + 1.0f64.min(1.5 * sw))).abs() < 1e-12);
    assert!(s.plot.title.starts_with("HF return-current density (coplanar gap flank 8."));
    assert_eq!(s.plot.labels.len(), 3);
    // profile styling: planes keep their colours, the coplanar faces share one, bottom is dashed
    let style =
        |k: &str| s.profiles.iter().find(|p| p.key == k).map(|p| (p.color, p.dashed, p.break_at_zero));
    assert_eq!(style("plane1"), Some((0, false, false)));
    assert_eq!(style("plane2"), Some((1, false, false)));
    assert_eq!(style("cop_top"), Some((2, false, true)));
    assert_eq!(style("cop_bottom"), Some((2, true, true)));
    // info text as in the Python tool
    assert!(s.info.contains("90 % of the plane return current flows within |x| <="), "{}", s.info);
    assert!(s.info.contains("Skin depth at 1 GHz: 2.09 um -> valid."), "{}", s.info);
    assert!(!s.info.contains("Domain boundary"));
    // an open structure adds the boundary explanation and the skin-depth warning for thin copper
    let mut set = m.defaults();
    set.set("plane1", ParamValue::Bool(false));
    set.set("fence1", ParamValue::Bool(false));
    set.set("t", ParamValue::Num(0.004));
    let o = m.analyze(&set, &Ctl::new()).unwrap();
    let info = &o.modes[0].split.as_ref().unwrap().info;
    assert!(info.contains("Domain boundary") && info.contains("NOT well below t"), "{info}");
}
