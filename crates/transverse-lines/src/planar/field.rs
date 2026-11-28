//! Field-view data of a grid solution: potentials, conductor drawing and view limits.

use transverse_core::quasistatic::Solution;

use super::PlanarParams;
use crate::model::{DrawRect, FieldData, ShapeKind};

/// Conductor drawing, guide lines and view limits, for a domain extension `margin`.
pub(super) struct Drawing {
    pub conductors: Vec<DrawRect>,
    pub dashed_y: Vec<f64>,
    pub dotted_y: Vec<f64>,
    pub view_zoom: [f64; 4],
    pub view_full: [f64; 4],
}

pub(super) fn drawing(p: &PlanarParams, margin: f64) -> Drawing {
    let p = &p.effective();
    let stack = p.stackup();
    let g = p.geometry(margin);
    let hmax = p.h1.max(p.h2);

    // conductors, drawn thick enough to see even for t = 0
    let th = p.t.max((p.h1 + p.h2) / 300.0);
    let y0c = if p.t > 0.0 { 0.0 } else { -th / 2.0 };
    let rect = |kind, x0, y0, x1, y1| DrawRect { kind, x0, y0, x1, y1, top: None };
    let xd = g.xd;
    // Via fences and side walls are walls without thickness in the solution. They are drawn with a width, outwards
    // from the wall like the planes, so that the inner face is where the wall is and nothing inside it is covered.
    // The planes and the coplanar ground reach to the outer face of a wall at the domain edge.
    let fw = (p.h1 + p.h2) / 150.0;
    let outer = if stack.walls.is_empty() { xd } else { xd.max(stack.wall_x + fw) };
    let mut conductors = Vec::new();
    for layer in &stack.layers {
        let [ib, it] = layer.inset;
        // a piece of the copper layer; the sides that are etched flanks are set back by the insets
        let piece = |kind, x0: f64, x1: f64, flank0: bool, flank1: bool| {
            let at = |flank: bool, i: f64| if flank { i } else { 0.0 };
            let face = |i: f64| [x0 + at(flank0, i), x1 - at(flank1, i)];
            let ([b0, b1], top) = (face(ib), face(it));
            DrawRect { top: (ib != it).then_some(top), ..rect(kind, b0, y0c, b1, y0c + th) }
        };
        for t in &layer.traces {
            conductors.push(piece(ShapeKind::Signal, t.x0, t.x1, true, true));
        }
        if let Some([left, right]) = layer.ground {
            conductors.push(piece(ShapeKind::Ground, -outer, left, false, true));
            conductors.push(piece(ShapeKind::Ground, right, outer, true, false));
        }
    }
    let pl = (p.h1 + p.h2) / 60.0;
    if let Some(y) = stack.top_plane {
        conductors.push(rect(ShapeKind::Ground, -outer, y, outer, y + pl));
    }
    if let Some(y) = stack.bottom_plane {
        conductors.push(rect(ShapeKind::Ground, -outer, y - pl, outer, y));
    }
    for side in [-1.0, 1.0] {
        let (wall, face) = (side * stack.wall_x, side * (stack.wall_x + fw));
        for span in &stack.walls {
            conductors.push(rect(ShapeKind::Ground, wall.min(face), span.y0, wall.max(face), span.y1));
        }
    }
    // dashed: the board surface where a plane is missing (without an upper half there is only air above)
    let mut dashed_y = Vec::new();
    if p.upper && !p.plane1 {
        dashed_y.push(g.ytop);
    }
    if !p.plane2 {
        dashed_y.push(g.ybot);
    }

    // view limits
    let xr = if p.coplanar {
        let inner = p.inner_half_width();
        inner + 1.2 * inner
    } else {
        p.w / 2.0 + p.w.max(1.5 * hmax)
    }
    .min(xd);
    let zy0 = g.ybot - if p.plane2 { hmax / 30.0 } else { 0.5 * hmax };
    let zy1 = match (p.plane1, p.upper) {
        (true, _) => g.ytop + hmax / 30.0,
        (false, true) => g.ytop + 0.5 * hmax,
        (false, false) => p.t + 0.8 * p.h2,
    };
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
