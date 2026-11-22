//! Field-view data of a CPWG solution: potentials, conductor drawing and view limits.

use transverse_core::quasistatic::Solution;

use super::CpwgParams;
use crate::model::{DrawRect, FieldData, ShapeKind};

pub fn field_data(p: &CpwgParams, sol: &Solution) -> FieldData {
    let g = p.geometry(sol.accuracy.margin);
    let hmax = p.h1.max(p.h2);

    // conductors, drawn thick enough to see even for t = 0
    let th = p.t.max((p.h1 + p.h2) / 300.0);
    let y0c = if p.t > 0.0 { 0.0 } else { -th / 2.0 };
    let rect = |kind, x0, y0, x1, y1| DrawRect { kind, x0, y0, x1, y1 };
    let (xe, xd) = (g.xe, g.xd);
    let mut conductors = vec![
        rect(ShapeKind::Signal, -p.w / 2.0, y0c, p.w / 2.0, y0c + th),
        rect(ShapeKind::Ground, -xd, y0c, -p.w / 2.0 - p.s, y0c + th),
        rect(ShapeKind::Ground, p.w / 2.0 + p.s, y0c, xd, y0c + th),
    ];
    let pl = (p.h1 + p.h2) / 60.0;
    let mut dashed_y = Vec::new();
    if p.plane1 {
        conductors.push(rect(ShapeKind::Ground, -xd, g.ytop, xd, g.ytop + pl));
    } else {
        dashed_y.push(g.ytop);
    }
    if p.plane2 {
        conductors.push(rect(ShapeKind::Ground, -xd, g.ybot - pl, xd, g.ybot));
    } else {
        dashed_y.push(g.ybot);
    }
    let fw = (p.h1 + p.h2) / 150.0;
    for sx in [-1.0, 1.0] {
        if p.fence1 {
            conductors.push(rect(ShapeKind::Ground, sx * xe - fw / 2.0, p.t, sx * xe + fw / 2.0, p.t + p.h1));
        }
        if p.fence2 {
            conductors.push(rect(
                ShapeKind::Ground,
                sx * xe - fw / 2.0,
                g.ybot,
                sx * xe + fw / 2.0,
                g.ybot + p.h2,
            ));
        }
    }

    // view limits
    let xr = (p.w / 2.0 + p.s + 1.2 * (p.w / 2.0 + p.s)).min(xd);
    let zy0 = g.ybot - if p.plane2 { hmax / 30.0 } else { 0.5 * hmax };
    let zy1 = g.ytop + if p.plane1 { hmax / 30.0 } else { 0.5 * hmax };
    let pad = hmax / 30.0;

    FieldData {
        x: sol.grid.x.clone(),
        y: sol.grid.y.clone(),
        fixed: sol.grid.cat.iter().map(|&c| c > 0).collect(),
        er: sol.grid.er.clone(),
        v: sol.v.clone(),
        v0: sol.v0.clone(),
        conductors,
        dashed_y,
        dotted_y: vec![0.0, p.t],
        view_zoom: [-xr, xr, zy0, zy1],
        view_full: [-xd, xd, g.ymin - pad, g.ymax + pad],
        z0: sol.z0,
    }
}
