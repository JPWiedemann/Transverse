//! Field-view data of a CPWG solution: potentials, conductor drawing and view limits.

use transverse_core::quasistatic::Solution;

use super::PlanarParams;
use crate::model::{DrawRect, FieldData, ShapeKind};

/// Conductor drawing, guide lines and view limits of a CPWG, for a domain extension `margin`.
pub(super) struct Drawing {
    pub conductors: Vec<DrawRect>,
    pub dashed_y: Vec<f64>,
    pub dotted_y: Vec<f64>,
    pub view_zoom: [f64; 4],
    pub view_full: [f64; 4],
}

pub(super) fn drawing(p: &PlanarParams, margin: f64) -> Drawing {
    let g = p.geometry(margin);
    let hmax = p.h1.max(p.h2);

    // conductors, drawn thick enough to see even for t = 0
    let th = p.t.max((p.h1 + p.h2) / 300.0);
    let y0c = if p.t > 0.0 { 0.0 } else { -th / 2.0 };
    let rect = |kind, x0, y0, x1, y1| DrawRect { kind, x0, y0, x1, y1 };
    let (xe, xd) = (g.xe, g.xd);
    // The via fences are walls without thickness in the solution. They are drawn with a width, outwards from the
    // wall like the planes, so that the inner face is where the wall is and nothing inside it is covered. The
    // planes and the coplanar ground reach to the outer face of a fence at the domain edge.
    let fw = (p.h1 + p.h2) / 150.0;
    let outer = if p.fence1 || p.fence2 { xd.max(xe + fw) } else { xd };
    let mut conductors = vec![
        rect(ShapeKind::Signal, -p.w / 2.0, y0c, p.w / 2.0, y0c + th),
        rect(ShapeKind::Ground, -outer, y0c, -p.w / 2.0 - p.s, y0c + th),
        rect(ShapeKind::Ground, p.w / 2.0 + p.s, y0c, outer, y0c + th),
    ];
    let pl = (p.h1 + p.h2) / 60.0;
    let mut dashed_y = Vec::new();
    if p.plane1 {
        conductors.push(rect(ShapeKind::Ground, -outer, g.ytop, outer, g.ytop + pl));
    } else {
        dashed_y.push(g.ytop);
    }
    if p.plane2 {
        conductors.push(rect(ShapeKind::Ground, -outer, g.ybot - pl, outer, g.ybot));
    } else {
        dashed_y.push(g.ybot);
    }
    for side in [-1.0, 1.0] {
        let (wall, face) = (side * xe, side * (xe + fw));
        let (x0, x1) = (wall.min(face), wall.max(face));
        if p.fence1 {
            conductors.push(rect(ShapeKind::Ground, x0, p.t, x1, p.t + p.h1));
        }
        if p.fence2 {
            conductors.push(rect(ShapeKind::Ground, x0, g.ybot, x1, g.ybot + p.h2));
        }
    }

    // view limits
    let xr = (p.w / 2.0 + p.s + 1.2 * (p.w / 2.0 + p.s)).min(xd);
    let zy0 = g.ybot - if p.plane2 { hmax / 30.0 } else { 0.5 * hmax };
    let zy1 = g.ytop + if p.plane1 { hmax / 30.0 } else { 0.5 * hmax };
    let pad = hmax / 30.0;

    Drawing {
        conductors,
        dashed_y,
        dotted_y: vec![0.0, p.t],
        view_zoom: [-xr, xr, zy0, zy1],
        view_full: [-outer, outer, g.ymin - pad, g.ymax + pad],
    }
}

pub fn field_data(p: &PlanarParams, sol: &Solution) -> FieldData {
    let d = drawing(p, sol.accuracy.margin);
    FieldData {
        x: sol.grid.x.clone(),
        y: sol.grid.y.clone(),
        fixed: sol.grid.cat.iter().map(|&c| c > 0).collect(),
        er: sol.grid.er.clone(),
        v: sol.v.clone(),
        v0: sol.v0.clone(),
        conductors: d.conductors,
        dashed_y: d.dashed_y,
        dotted_y: d.dotted_y,
        view_zoom: d.view_zoom,
        view_full: d.view_full,
        z0: sol.z0,
        mesh: None,
    }
}
