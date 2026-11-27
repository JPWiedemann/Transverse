//! The first mesh of CPWG cross-sections, over many geometries: no element is far smaller than the size field
//! asks for. (A point placed almost on an interface or a conductor edge would leave a knot of tiny elements.)

use transverse_core::Ctl;
use transverse_lines::cpwg_inner::{CpwgParams, GapMode};
use transverse_mesh::planar::build;
use transverse_mesh::sizefield::SizeField;

/// Smallest edge length relative to the size wanted there, and where it is.
fn smallest_edge(p: &CpwgParams, sizes: Option<(f64, f64)>, scale: f64) -> (f64, [f64; 2]) {
    let margin = if p.closed() { 0.0 } else { 1.0 };
    let cs = p.cross_section(margin);
    let spec = match sizes {
        Some((dmin, dmax)) => p.size_spec_for(margin, dmin, dmax),
        None => p.size_spec(margin),
    }
    .scaled(scale);
    let mesh = transverse_mesh::mesh(&cs, &spec, 2_000_000, &Ctl::new()).unwrap();
    let field = SizeField::new(&build(&cs).unwrap(), spec);
    let mut worst = (f64::INFINITY, [0.0; 2]);
    for t in &mesh.tris {
        for k in 0..3 {
            let (a, b) = (mesh.pts[t[k] as usize], mesh.pts[t[(k + 1) % 3] as usize]);
            let mid = [0.5 * (a[0] + b[0]), 0.5 * (a[1] + b[1])];
            let ratio = (a[0] - b[0]).hypot(a[1] - b[1]) / field.at(mid);
            if ratio < worst.0 {
                worst = (ratio, mid);
            }
        }
    }
    worst
}

#[test]
fn no_knots_of_tiny_elements_anywhere() {
    let base = CpwgParams::default();
    // the geometry and manual sizes of a reported knot in the gap, at 61.9 um from the strip on the copper surface
    let reported = CpwgParams { w: 0.359971, g: 0.2, ..base.clone() };
    let mut cases = vec![
        ("reported, manual".to_string(), reported.clone(), Some((0.002, 0.2)), 1.0),
        ("default, adaptive start".to_string(), base.clone(), None, 4.0),
    ];
    for (i, (w, s, g)) in
        [(0.3605, 0.11, 1.0), (0.359971, 0.11, 0.2), (0.2, 0.1, 0.5), (0.15, 0.075, 0.3), (0.5, 0.2, 1.5)]
            .into_iter()
            .enumerate()
    {
        let p = CpwgParams { w, s, g, ..base.clone() };
        for scale in [1.0, 2.0, 4.0] {
            cases.push((format!("closed {i} scale {scale}"), p.clone(), None, scale));
        }
        let zero = CpwgParams { t: 0.0, ..p.clone() };
        cases.push((format!("zero thickness {i}"), zero, None, 2.0));
        let open = CpwgParams { plane1: false, fence1: false, gap_mode: GapMode::Lower, ..p };
        cases.push((format!("open {i}"), open, None, 2.0));
    }
    let mut bad = Vec::new();
    let mut lowest = f64::INFINITY;
    for (name, p, sizes, scale) in &cases {
        let (ratio, at) = smallest_edge(p, *sizes, *scale);
        lowest = lowest.min(ratio);
        // edges between 0.15 and 0.5 of the wanted size are ordinary in a Delaunay mesh; the knots that this guards
        // against reached 0.004 (reported) and 0.07
        if ratio < 0.12 {
            bad.push(format!(
                "{name}: edge at ({:.4}, {:.4}) mm is {ratio:.4} of the wanted size",
                at[0], at[1]
            ));
        }
    }
    println!("smallest edge relative to the size field: {lowest:.3}");
    assert!(bad.is_empty(), "{} of {} meshes:\n{}", bad.len(), cases.len(), bad.join("\n"));
}
