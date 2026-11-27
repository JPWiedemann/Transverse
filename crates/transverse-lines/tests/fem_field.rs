//! Field data of the FEM backend against the grid reference (final stack-up) and basic sanity of the
//! equipotentials, field lines, raster and probe on a triangle mesh.

use serde::Deserialize;
use transverse_core::Ctl;
use transverse_core::units::UnitSystem;
use transverse_lines::cpwg_inner::{CpwgParams, FemOptions, analyze, analyze_fem, field_data_fem};
use transverse_lines::mesh_field::MeshFieldData;
use transverse_lines::render::mesh_raster::{probe_mesh, render_mesh};
use transverse_lines::render::raster::{FieldMode, RasterSpec, render};

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
    nodes: Vec<NodeRef>,
}
#[derive(Deserialize)]
struct NodeRef {
    i: usize,
    j: usize,
    v: f64,
    v0: f64,
    e: Option<f64>,
}

fn reference_nodes() -> Vec<NodeRef> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/reference/cases.json");
    let all: Cases = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    all.cases.into_iter().find(|c| c.name == "final_stackup").unwrap().field.unwrap().nodes
}

/// The default stack-up (the FD "final stack-up" case) solved with FEM at a tight tolerance.
fn stackup() -> (CpwgParams, MeshFieldData) {
    let p = CpwgParams { tol: 0.2, ..CpwgParams::default() };
    let sol = analyze_fem(
        &p,
        &FemOptions {
            start_scale: std::env::var("FEM_START")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(p.fem_options().start_scale),
            ..p.fem_options()
        },
        &Ctl::new(),
    )
    .unwrap();
    let f = field_data_fem(&p, &sol).unwrap();
    (p, f)
}

fn fd_field(p: &CpwgParams) -> (transverse_lines::model::FieldData, transverse_core::postproc::EField) {
    let sol = analyze(p, &Ctl::new()).unwrap();
    let f = transverse_lines::cpwg_inner::field_data(p, &sol);
    let e = f.e_field(false);
    (f, e)
}

#[test]
fn fem_potentials_and_e_field_match_fd_at_the_reference_nodes() {
    // measured: potentials within 1.4e-4 V, |E| within 4.2 % of the grid solution at the sample nodes
    let (p, f) = stackup();
    let fd = analyze(&p, &Ctl::new()).unwrap();
    let (mut compared, mut e_compared) = (0, 0);
    let nodes = reference_nodes();
    // |E| is compared where the images show it: within 60 dB of the largest value of the samples
    let e_top = nodes.iter().filter_map(|n| n.e).fold(0.0, f64::max);
    for n in nodes {
        let pos = [fd.grid.x[n.i], fd.grid.y[n.j]];
        let Some(e) = n.e else { continue }; // conductor node of the grid
        let (v, v0) = (f.potential(pos, false).unwrap(), f.potential(pos, true).unwrap());
        assert!((v - n.v).abs() < 3e-4 && (v0 - n.v0).abs() < 3e-4, "potential at {pos:?}: {v} vs {}", n.v);
        compared += 1;
        if e < 1e-3 * e_top {
            continue;
        }
        let em = f.e_mag(pos).unwrap();
        assert!((em / e - 1.0).abs() < 3e-2, "|E| at {pos:?}: FEM {em} vs FD {e}");
        e_compared += 1;
    }
    assert!(compared > 15 && e_compared > 8, "only {compared} nodes, {e_compared} with |E| compared");
}

#[test]
fn equipotentials_lie_on_their_potential() {
    let (_, f) = stackup();
    let levels = [0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9];
    let lines = f.equipotentials(&levels);
    assert_eq!(lines.len(), levels.len());
    for (level, segs) in &lines {
        assert!(segs.len() > 50, "level {level}: only {} segments", segs.len());
        for s in segs.iter().step_by(7) {
            for pnt in s {
                // the marching triangles are linear on a quarter of an element: the potential there is the level
                // up to the difference between the linear and the quadratic interpolant (largest next to the strip edge)
                let v = f
                    .potential(*pnt, false)
                    .unwrap_or_else(|| panic!("segment end {pnt:?} outside the mesh"));
                assert!((v - level).abs() < 6e-3, "level {level}: V = {v} at {pnt:?}");
            }
        }
    }
    // a level outside the range has no line
    assert!(f.equipotentials(&[1.5])[0].1.is_empty());
}

#[test]
fn field_lines_follow_e_and_stay_in_the_dielectric() {
    let (_, f) = stackup();
    let lines = f.field_lines(f.view_zoom, (1.0, 1.0));
    assert!(lines.len() > 20, "{} lines", lines.len());
    let mut falling = 0;
    for l in &lines {
        assert!(l.len() > 3);
        assert!(l.iter().all(|p| f.locate(*p).is_some()), "a line leaves the dielectric");
        let (a, b) = (f.potential(l[0], false).unwrap(), f.potential(*l.last().unwrap(), false).unwrap());
        if a > b {
            falling += 1;
        }
    }
    assert!(
        falling as f64 > 0.95 * lines.len() as f64,
        "lines are oriented along E: {falling} of {}",
        lines.len()
    );
}

fn spec(mode: FieldMode, f: &MeshFieldData) -> RasterSpec {
    RasterSpec {
        mode,
        view: f.view_zoom,
        scale_view: f.view_zoom,
        system: UnitSystem::Metric,
        db_range: 40.0,
        width: 400,
    }
}

#[test]
fn mesh_rasters_look_like_the_grid_rasters() {
    let (p, f) = stackup();
    let (fd, e) = fd_field(&p);
    for mode in [FieldMode::Potential, FieldMode::EDb, FieldMode::ELinear, FieldMode::Permittivity] {
        let s = spec(mode, &f);
        let (a, b) = (render(&fd, &e, &s), render_mesh(&f, &s));
        assert_eq!((a.width, a.height), (b.width, b.height), "{mode:?}");
        let (la, lb) = (a.legend.unwrap(), b.legend.unwrap());
        assert_eq!(la.label, lb.label);
        // the colour scales agree exactly for the potential and permittivity. For |E| the top of the scale is a
        // percentile of the field in the view: the grid takes it over its nodes (dense at the edges), the mesh
        // weights nodes by their area, so they differ by a few dB or a third
        let potential_like = matches!(mode, FieldMode::Potential | FieldMode::Permittivity);
        let tol = if potential_like { 1e-6 } else { 0.5 };
        assert!(
            (la.vmax - lb.vmax).abs() <= tol * la.vmax.abs().max(1.0) + 1e-9,
            "{mode:?} vmax {} vs {}",
            la.vmax,
            lb.vmax
        );
        // pixels: the same ones are transparent (conductors) except along their edges, and the colours are close
        let (mut alpha_diff, mut colour_diff, mut both) = (0usize, 0.0f64, 0usize);
        for k in 0..a.width * a.height {
            let (pa, pb) = (&a.rgba[4 * k..4 * k + 4], &b.rgba[4 * k..4 * k + 4]);
            if (pa[3] == 0) != (pb[3] == 0) {
                alpha_diff += 1;
            } else if pa[3] != 0 {
                both += 1;
                colour_diff += (0..3).map(|c| (f64::from(pa[c]) - f64::from(pb[c])).abs()).sum::<f64>() / 3.0;
            }
        }
        let n = a.width * a.height;
        // the grid paints potential and permittivity inside the conductors too; the mesh has no elements there
        if !matches!(mode, FieldMode::Potential | FieldMode::Permittivity) {
            assert!(
                (alpha_diff as f64) < 0.02 * n as f64,
                "{mode:?}: {alpha_diff} of {n} pixels differ in transparency"
            );
        }
        assert!(both > n / 3, "{mode:?}: only {both} opaque pixels");
        let mean = colour_diff / both as f64;
        if !potential_like {
            continue; // the colours of the |E| images follow the (different) scales; |E| itself is compared above
        }
        assert!(mean < 6.0, "{mode:?}: mean colour difference {mean:.2} of 255");
    }
}

#[test]
fn the_probe_tells_conductors_from_dielectric() {
    let (p, f) = stackup();
    // inside the signal strip
    let c = probe_mesh(&f, 0.0, p.t / 2.0).unwrap();
    assert!(c.in_conductor && c.v.is_nan());
    // in the gap: between the potentials of the conductors, with the field and the permittivity of the gap material
    let g = probe_mesh(&f, p.w / 2.0 + p.s / 2.0, p.t / 2.0).unwrap();
    assert!(!g.in_conductor && g.v > 0.0 && g.v < 1.0 && g.e.is_finite() && g.e > 0.0);
    assert_eq!(g.er, p.er1);
    // under the signal: in the lower dielectric, strong field
    let d = probe_mesh(&f, 0.0, -p.h2 / 2.0).unwrap();
    assert_eq!(d.er, p.er2);
    assert!(d.v > 0.0 && d.v < 1.0);
    assert!(probe_mesh(&f, 10.0, 0.0).is_none());
    // the same value from the grid probe
    let (fd, e) = fd_field(&p);
    let r = transverse_lines::render::raster::probe(&fd, &e, 0.0, -p.h2 / 2.0).unwrap();
    assert!((r.v - d.v).abs() < 3e-4 && (r.e / d.e - 1.0).abs() < 3e-2, "{} {} vs {} {}", r.v, r.e, d.v, d.e);
}
