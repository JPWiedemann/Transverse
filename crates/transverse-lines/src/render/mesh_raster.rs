//! Field views of a triangle mesh as pixel images and the probe for the cursor readout: the same modes,
//! colour scales and legends as the grid ([`super::raster`]); each pixel is looked up in the mesh.

use transverse_core::fem::p2_values;
use transverse_core::postproc::weighted_percentile;

use super::raster::{FieldMode, Probe, Raster, RasterSpec, Scale, V_PER_M};
use crate::mesh_field::MeshFieldData;

pub fn render_mesh(f: &MeshFieldData, spec: &RasterSpec) -> Raster {
    let [vx0, vx1, vy0, vy1] = spec.view;
    let b = f.bounds;
    let (x0, x1) = (vx0.max(b[0]), vx1.min(b[1]));
    let (y0, y1) = (vy0.max(b[2]), vy1.min(b[3]));
    if x1 <= x0 || y1 <= y0 {
        return Raster { width: 1, height: 1, rgba: vec![0; 4], legend: None };
    }
    let width = spec.width.max(8);
    let height = ((width as f64 * (y1 - y0) / (x1 - x0)).round() as usize).clamp(8, 4000);

    let e_nodal = f.e_mag_nodal();
    let [sx0, sx1, sy0, sy1] = spec.scale_view;
    let in_view = |k: usize| {
        let p = f.pts[k];
        p[0] >= sx0 && p[0] <= sx1 && p[1] >= sy0 && p[1] <= sy1
    };
    // each node counts with the area around it: the mesh is much denser at conductor edges than elsewhere
    let area = f.node_areas();
    let e_hi = || {
        let all = |k: usize| (e_nodal[k], area[k]);
        let view = weighted_percentile((0..e_nodal.len()).filter(|&k| in_view(k)).map(all), 99.7);
        view.or_else(|| weighted_percentile((0..e_nodal.len()).map(all), 99.7)).unwrap_or(1.0).max(1e-30)
    };
    let range = |a: &mut dyn Iterator<Item = f64>| {
        a.fold((f64::INFINITY, f64::NEG_INFINITY), |r, v| (r.0.min(v), r.1.max(v)))
    };
    let v_range = range(&mut f.v.iter().take(f.pts.len()).copied());
    let er_range = range(&mut f.tri_er.iter().copied());
    let sc = Scale::new(spec, e_hi, v_range, er_range, f.tri_er.iter().fold(0.0f64, |a, &v| a.max(v)));
    let span = (sc.vmax - sc.vmin).max(f64::MIN_POSITIVE);

    let mut rgba = vec![0u8; width * height * 4];
    for j in 0..height {
        let y = y1 - (j as f64 + 0.5) / height as f64 * (y1 - y0);
        for i in 0..width {
            let x = x0 + (i as f64 + 0.5) / width as f64 * (x1 - x0);
            // pixels outside the dielectric (inside conductors) stay transparent
            let Some(loc) = f.locate([x, y]) else { continue };
            let t = f.tris[loc.tri];
            let v = match spec.mode {
                FieldMode::EDb | FieldMode::EDbLines | FieldMode::ELinear => {
                    sc.value((0..3).map(|k| loc.l[k] * e_nodal[t[k] as usize]).sum())
                }
                FieldMode::Potential => p2_values(loc.l, f.dofs[loc.tri].map(|d| f.v[d as usize])),
                FieldMode::Permittivity | FieldMode::Mesh => f.tri_er[loc.tri],
            };
            let c = sc.cmap.sample((v - sc.vmin) / span);
            let o = (j * width + i) * 4;
            rgba[o..o + 4].copy_from_slice(&[c[0], c[1], c[2], 255]);
        }
    }
    Raster { width, height, rgba, legend: sc.legend(spec) }
}

/// Values at (`x`, `y`): `None` outside the mesh, a conductor probe (NaN values) inside a conductor.
pub fn probe_mesh(f: &MeshFieldData, x: f64, y: f64) -> Option<Probe> {
    let b = f.bounds;
    if x < b[0] || x > b[1] || y < b[2] || y > b[3] {
        return None;
    }
    let Some(loc) = f.locate([x, y]) else {
        return Some(Probe { v: f64::NAN, e: f64::NAN, er: f64::NAN, in_conductor: true });
    };
    let t = f.tris[loc.tri];
    let e_mag: f64 =
        (0..3).map(|k| loc.l[k] * f.e_nodal[t[k] as usize][0].hypot(f.e_nodal[t[k] as usize][1])).sum();
    Some(Probe {
        v: p2_values(loc.l, f.dofs[loc.tri].map(|d| f.v[d as usize])),
        e: e_mag * V_PER_M,
        er: f.tri_er[loc.tri],
        in_conductor: false,
    })
}
