//! The generic line-model interface: parameter form, conversion, analysis, synthesis, field data.

use serde::Deserialize;
use transverse_core::Ctl;
use transverse_core::postproc::percentile;
use transverse_lines::model::{LineModel, ParamKind, ParamSet, ParamValue, ShapeKind, is_enabled};
use transverse_lines::planar::{GroundCat, PlanarLine, PlanarParams, analyze, field_data, full_analysis};

fn rel(a: f64, b: f64) -> f64 {
    (a / b - 1.0).abs()
}

#[test]
fn defaults_equal_the_params_defaults_and_round_trip() {
    let m = PlanarLine;
    let set = m.defaults();
    assert_eq!(PlanarParams::from_set(&set).unwrap(), PlanarParams::default());
    assert_eq!(PlanarParams::default().to_set(), set);
}

#[test]
fn spec_keys_match_the_parameter_set_exactly() {
    let m = PlanarLine;
    let set = PlanarParams::default().to_set();
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
    let m = PlanarLine;
    let mut set = m.defaults();
    let find = |k: &str| *m.params().iter().find(|p| p.key == k).unwrap();
    assert!(!is_enabled(&find("dmin"), &set) && !is_enabled(&find("dmax"), &set));
    assert!(!is_enabled(&find("er_gap"), &set));
    set.set("mesh_mode", ParamValue::Num(1.0));
    set.set("gap_mode", ParamValue::Num(2.0));
    assert!(is_enabled(&find("dmin"), &set) && is_enabled(&find("dmax"), &set));
    assert!(is_enabled(&find("tand_gap"), &set));
    assert!(is_enabled(&find("w"), &set));
}

#[test]
fn invalid_sets_are_rejected_and_partial_sets_use_defaults() {
    let mut set = ParamSet::default();
    assert_eq!(PlanarParams::from_set(&set).unwrap(), PlanarParams::default());
    set.set("w", ParamValue::Num(0.5));
    assert_eq!(PlanarParams::from_set(&set).unwrap().w, 0.5);
    for (k, v) in [
        ("gap_mode", ParamValue::Num(5.0)),
        ("gap_mode", ParamValue::Num(0.5)),
        ("plane1", ParamValue::Num(1.0)),
        ("w", ParamValue::Bool(true)),
        ("max_nodes", ParamValue::Num(0.0)),
    ] {
        let mut s = ParamSet::default();
        s.set(k, v);
        assert!(PlanarParams::from_set(&s).is_err(), "{k} = {v:?} accepted");
    }
}

#[test]
fn param_set_json_round_trip() {
    let set = PlanarLine.defaults();
    let text = serde_json::to_string(&set).unwrap();
    assert!(text.contains("\"plane1\":true") && text.contains("\"w\":0.3605"));
    assert_eq!(serde_json::from_str::<ParamSet>(&text).unwrap(), set);
}

#[test]
fn wavelength_follows_frequency_and_permittivity() {
    let m = PlanarLine;
    let l1 = m.wavelength_mm(&m.defaults()).unwrap();
    let mut s = m.defaults();
    s.set("f", ParamValue::Num(2e9));
    assert!(rel(m.wavelength_mm(&s).unwrap(), l1 / 2.0) < 1e-12);
    s.set("f", ParamValue::Num(0.0));
    assert!(m.wavelength_mm(&s).is_none());
}

#[test]
fn analysis_through_the_trait_matches_the_reference() {
    // the finite elements through the interface; the reference values are the converged grid results
    let a = PlanarLine.analyze(&PlanarLine.defaults(), &Ctl::new()).unwrap();
    assert_eq!(a.modes.len(), 1);
    let m = &a.modes[0];
    assert!(rel(m.z0, 35.001) < 1e-3 && rel(m.eps_eff, 3.58) < 5e-3);
    assert!(a.report.contains(&format!("Z0              = {:8.3} Ohm", m.z0)), "{}", a.report);
    assert!(a.report.contains("finite elements"), "{}", a.report);
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
    let m = PlanarLine;
    let mut set = m.defaults();
    set.set("plane1", ParamValue::Bool(false));
    set.set("fence1", ParamValue::Bool(false));
    let (out, a) = m.synthesize(&set, "w", 50.0, None, &Ctl::new()).unwrap();
    assert!(rel(a.modes[0].z0, 50.0) < 2e-3);
    assert!(a.report.starts_with("=== Synthesis: Z0 = 50 Ohm, vary w (s fixed) ==="));
    // only w changed; in particular the automatically found margin is not written back
    let mut expect = set.clone();
    expect.set("w", out.get("w").unwrap());
    assert_eq!(out, expect);
    assert!((out.num("w").unwrap() - 0.2467).abs() < 1e-3);
    assert!(m.synthesize(&set, "nonsense", 50.0, None, &Ctl::new()).is_err());
}

#[test]
fn analysis_reports_wavelength_and_phase_of_the_entered_length() {
    let m = PlanarLine;
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
fn synthesis_with_a_phase_sets_the_length_from_the_solution() {
    let m = PlanarLine;
    let mut set = m.defaults();
    set.set("f", ParamValue::Num(2.4e9));
    let (out, a) = m.synthesize(&set, "w", 35.0, Some(90.0), &Ctl::new()).unwrap();
    let mode = &a.modes[0];
    assert!(rel(mode.z0, 35.0) < 2e-3);
    // a quarter wavelength of the guided wave at 2.4 GHz, with the eps_eff of the synthesised geometry
    let lambda = transverse_core::C_LIGHT / (2.4e9 * mode.eps_eff.sqrt()) * 1e3;
    assert!(rel(out.num("length").unwrap(), lambda / 4.0) < 1e-12);
    assert!(rel(mode.phase_deg, 90.0) < 1e-12);
    // only w and the length changed
    let mut expect = set.clone();
    expect.set("w", out.get("w").unwrap());
    expect.set("length", out.get("length").unwrap());
    assert_eq!(out, expect);
    assert!(
        a.report.starts_with(
            "=== Synthesis: Z0 = 35 Ohm, vary w (s fixed), phase = 90 deg at 2.4 GHz -> length ==="
        ),
        "{}",
        a.report
    );
    // without a phase the length stays as entered
    let (kept, _) = m.synthesize(&set, "w", 35.0, None, &Ctl::new()).unwrap();
    assert_eq!(kept.num("length").unwrap(), 100.0);
    // more phase needs more length, linearly
    let (out2, _) = m.synthesize(&set, "w", 35.0, Some(180.0), &Ctl::new()).unwrap();
    assert!(rel(out2.num("length").unwrap(), 2.0 * out.num("length").unwrap()) < 1e-12);
    // nonsense phases are rejected
    for bad in [0.0, -10.0, f64::NAN] {
        assert!(m.synthesize(&set, "w", 35.0, Some(bad), &Ctl::new()).is_err(), "{bad}");
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

/// The field data of the grid solver (the reference), which the stored values belong to.
fn grid_field(p: &PlanarParams) -> transverse_lines::model::FieldData {
    field_data(p, &analyze(p, &Ctl::new()).unwrap())
}

fn final_stackup_field() -> transverse_lines::model::FieldData {
    grid_field(&PlanarParams::default())
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
    let o = grid_field(&PlanarParams { plane1: false, fence1: false, ..PlanarParams::default() });
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
    let m = PlanarLine;
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
    let m = PlanarLine;
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

// ---- the model solves with finite elements; the grid solver is the reference

#[test]
fn the_model_agrees_with_the_grid_solver() {
    let grid = full_analysis(&PlanarParams::default(), &Ctl::new()).unwrap();
    let a = PlanarLine.analyze(&PlanarLine.defaults(), &Ctl::new()).unwrap();
    let m = &a.modes[0];
    assert!(rel(m.z0, grid.sol.z0) < 1e-3 && rel(m.eps_eff, grid.sol.eeff) < 1e-3, "{}", m.z0);
    let at = grid.loss.at(1e9);
    assert!(rel(m.at_f.alpha_c, at.alpha_c) < 1e-3 && rel(m.at_f.alpha_d, at.alpha_d) < 1e-3);
    for row in &m.split.as_ref().unwrap().rows {
        let Some(cat) = GroundCat::ALL.iter().find(|c| c.key() == row.key) else { continue };
        let (field, current) = (grid.split.field.get(*cat), grid.split.current.get(*cat));
        assert!(
            (row.field_pct - field).abs() < 0.5 && (row.current_pct - current).abs() < 0.5,
            "{}",
            row.key
        );
    }
    assert!(a.report.contains("Accuracy"));
    // the accuracy record of a triangle mesh has no grid size
    let acc = a.accuracy.unwrap();
    assert!(acc.nodes > 1000 && acc.nx == 0 && acc.ny == 0 && acc.dmin_actual > 0.0);
    assert!(acc.est_err.unwrap() < 5e-3);
}

#[test]
fn old_parameter_sets_with_solver_keys_still_load() {
    let mut s = PlanarLine.defaults();
    s.set("solver", ParamValue::Num(0.0));
    s.set("richardson", ParamValue::Bool(false));
    assert_eq!(PlanarParams::from_set(&s).unwrap(), PlanarParams::default());
    let json: PlanarParams = serde_json::from_str(r#"{"solver": "fd", "w": 0.3}"#).unwrap();
    assert_eq!(json.w, 0.3);
}

#[test]
fn mesh_field_data_answers_the_same_questions_as_grid_field_data() {
    let f = PlanarLine.analyze(&PlanarLine.defaults(), &Ctl::new()).unwrap().field.unwrap();
    let g = final_stackup_field();
    assert!(f.mesh.is_some() && g.mesh.is_none() && g.mesh_edges(g.view_zoom).is_none());
    assert_eq!(f.conductors.len(), g.conductors.len());
    assert_eq!(f.view_zoom, g.view_zoom);
    // the extent is in x and y, so that frontends can clip to it
    assert!(f.x[0] <= f.view_zoom[0] && f.x[1] >= f.view_zoom[1] && f.y[0] < f.y[1]);
    let grid_extent = [g.x[0], g.x[g.x.len() - 1], g.y[0], g.y[g.y.len() - 1]];
    for (a, b) in [f.x[0], f.x[1], f.y[0], f.y[1]].into_iter().zip(grid_extent) {
        assert!((a - b).abs() < 1e-12 * (g.x[g.x.len() - 1] - g.x[0]), "{a} vs {b}");
    }
    let lines = f.field_lines(f.view_zoom, (1.0, 1.0));
    assert!(lines.len() > 20);
    assert_eq!(f.equipotentials(&[0.3, 0.6]).len(), 2);
    // mesh edges: all of them for the whole view, fewer for a small window
    let all = f.mesh_edges(f.view_full).unwrap();
    let w = f.mesh_edges([-0.1, 0.1, -0.05, 0.05]).unwrap();
    assert!(all.len() > 3000 && !w.is_empty() && w.len() < all.len() / 4, "{} / {}", w.len(), all.len());
    // the raster and the probe work on the mesh through the grid interface
    use transverse_lines::render::raster::{FieldMode, RasterSpec, probe, render};
    let spec = RasterSpec {
        mode: FieldMode::EDb,
        view: f.view_zoom,
        scale_view: f.view_zoom,
        system: transverse_core::units::UnitSystem::Metric,
        db_range: 40.0,
        width: 200,
    };
    let e = f.e_field(false);
    let r = render(&f, &e, &spec);
    assert!(r.width == 200 && r.rgba.chunks(4).filter(|p| p[3] != 0).count() > r.width * r.height / 3);
    assert!(probe(&f, &e, 0.0, -0.1).is_some_and(|p| !p.in_conductor && p.v > 0.5));
}

#[test]
fn synthesis_with_the_finite_elements_finds_the_reference_width() {
    let mut s = PlanarLine.defaults();
    s.set("w", ParamValue::Num(0.2));
    let (set, a) = PlanarLine.synthesize(&s, "w", 35.0, None, &Ctl::new()).unwrap();
    let w = PlanarParams::from_set(&set).unwrap().w;
    // the reference (grid) result is w = 0.3605 mm for 35 Ohm
    assert!(rel(w, 0.3605) < 3e-3, "w = {w}");
    assert!(rel(a.modes[0].z0, 35.0) < 2e-3, "z0 = {}", a.modes[0].z0);
    assert!(a.field.unwrap().mesh.is_some());
}

/// The drawing must not cover any of the solved region: conductors are drawn where the solution has none.
#[test]
fn via_fences_are_drawn_outside_the_wall_they_stand_for() {
    let p = PlanarParams::default();
    let wall = p.w / 2.0 + p.s + p.g;
    let mesh = PlanarLine.analyze(&PlanarLine.defaults(), &Ctl::new()).unwrap().field.unwrap();
    for f in [final_stackup_field(), mesh] {
        // the fences are the narrow upright rectangles
        let fences: Vec<_> = f.conductors.iter().filter(|c| c.y1 - c.y0 > 4.0 * (c.x1 - c.x0)).collect();
        assert_eq!(fences.len(), 4);
        for c in &fences {
            let inner = c.x0.abs().min(c.x1.abs());
            assert!((inner - wall).abs() < 1e-12 * wall && c.x0 < c.x1, "{c:?}, wall at {wall}");
        }
        // the closed structure ends at the walls; planes and coplanar ground close the corners, and the full
        // view shows the fences completely
        assert!((f.x[f.x.len() - 1] - wall).abs() < 1e-12 * wall);
        let face = fences.iter().map(|c| c.x1).fold(0.0, f64::max);
        assert!(face > wall && f.view_full[1] >= face && f.view_full[0] <= -face);
        let widest = f.conductors.iter().map(|c| c.x1).fold(0.0, f64::max);
        assert_eq!(widest, face);
        assert_eq!(f.conductors.iter().filter(|c| c.x1 == face && c.x1 - c.x0 > c.y1 - c.y0).count(), 3);
    }
}
