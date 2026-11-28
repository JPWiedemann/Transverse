//! Tensor grid of the CPWG cross-section (ported 1:1 from the Python reference).

use transverse_core::fd2d::axis::{axis, graded_out};
use transverse_core::fd2d::{Grid, TOL_GEO};
use transverse_core::quasistatic::GridSource;

use super::PlanarParams;

// node categories (0 = free and 1 = signal are fixed by the core)
pub const C_FREE: u8 = 0;
pub const C_SIG: u8 = 1;
pub const C_PL1: u8 = 2;
pub const C_PL2: u8 = 3;
pub const C_COP: u8 = 4;
pub const C_F1: u8 = 5;
pub const C_F2: u8 = 6;
pub const C_BND: u8 = 7;

/// Domain extents [mm] for a domain extension `m` of the open regions.
#[derive(Clone, Copy, Debug)]
pub struct Geometry {
    pub xe: f64,
    pub xd: f64,
    pub ytop: f64,
    pub ybot: f64,
    pub ymin: f64,
    pub ymax: f64,
}

impl PlanarParams {
    pub fn geometry(&self, m: f64) -> Geometry {
        let p = self.effective();
        let xe = p.core_half_width();
        let ytop = p.t + p.h1;
        let ybot = -p.h2;
        Geometry {
            xe,
            xd: if p.closed() { xe } else { xe + m },
            ytop,
            ybot,
            ymax: if p.plane1 { ytop } else { ytop + m },
            ymin: if p.plane2 { ybot } else { ybot - m },
        }
    }
}

impl GridSource for PlanarParams {
    fn axes(&self, dmin: f64, dmax: f64, m: f64) -> (Vec<f64>, Vec<f64>) {
        let p = &self.effective();
        let g = p.geometry(m);
        let far = (m / 6.0).max(dmax);
        let mut xbreaks = vec![-g.xe];
        xbreaks.extend(p.stackup().copper_edges_x());
        xbreaks.push(g.xe);
        let mut x = axis(&xbreaks, dmin, dmax);
        if g.xd > g.xe + TOL_GEO {
            let right = graded_out(g.xe, g.xd, dmin, far);
            let mut full: Vec<f64> = right.iter().rev().take(right.len() - 1).map(|v| -v).collect();
            full.extend(x);
            full.extend(&right[1..]);
            x = full;
        }
        let breaks: Vec<f64> =
            if p.t > 0.0 { vec![g.ybot, 0.0, p.t, g.ytop] } else { vec![g.ybot, 0.0, g.ytop] };
        let mut y = axis(&breaks, dmin, dmax);
        if !p.plane2 {
            let below = graded_out(g.ybot, g.ymin, dmin, far);
            let mut full: Vec<f64> = below.iter().rev().take(below.len() - 1).copied().collect();
            full.extend(y);
            y = full;
        }
        if !p.plane1 {
            let above = graded_out(g.ytop, g.ymax, dmin, far);
            y.extend(&above[1..]);
        }
        (x, y)
    }

    fn fill(&self, x: Vec<f64>, y: Vec<f64>, m: f64) -> Grid {
        let p = &self.effective();
        let g = p.geometry(m);
        let (nx, ny) = (x.len(), y.len());
        let tol = TOL_GEO;

        // cell properties (air by default), constant along x
        let (er_g, td_g) = p.gap_props();
        let mut er_row = vec![1.0; ny - 1];
        let mut td_row = vec![0.0; ny - 1];
        for j in 0..ny - 1 {
            let yc = 0.5 * (y[j] + y[j + 1]);
            if yc > g.ybot && yc < 0.0 {
                (er_row[j], td_row[j]) = (p.er2, p.tand2);
            } else if yc > 0.0 && yc < p.t {
                (er_row[j], td_row[j]) = (er_g, td_g);
            } else if yc > p.t && yc < g.ytop {
                (er_row[j], td_row[j]) = (p.er1, p.tand1);
            }
        }
        let er = (0..ny - 1).flat_map(|j| std::iter::repeat_n(er_row[j], nx - 1)).collect();
        let tand = (0..ny - 1).flat_map(|j| std::iter::repeat_n(td_row[j], nx - 1)).collect();

        // node categories, later assignments win
        let mut cat = vec![C_FREE; nx * ny];
        for j in 0..ny {
            for i in 0..nx {
                let (xx, yy) = (x[i], y[j]);
                let ax = xx.abs();
                let mut c = C_FREE;
                if j == 0 || j == ny - 1 || i == 0 || i == nx - 1 {
                    c = C_BND;
                }
                let on_fence = (ax - g.xe).abs() <= tol;
                // side walls run from plane to plane; their upper half includes the copper layer
                if p.side_walls && on_fence && yy >= g.ybot - tol && yy <= g.ytop + tol {
                    c = if yy >= -tol { C_F1 } else { C_F2 };
                }
                if p.fence1 && on_fence && yy >= p.t - tol && yy <= g.ytop + tol {
                    c = C_F1;
                }
                if p.fence2 && on_fence && yy >= g.ybot - tol && yy <= tol {
                    c = C_F2;
                }
                if p.plane1 && (yy - g.ytop).abs() <= tol {
                    c = C_PL1;
                }
                if p.plane2 && (yy - g.ybot).abs() <= tol {
                    c = C_PL2;
                }
                let band = yy >= -tol && yy <= p.t + tol;
                if p.coplanar && band && ax >= p.w / 2.0 + p.s - tol {
                    c = C_COP; // coplanar ground up to the grid edge
                }
                if band && ax <= p.w / 2.0 + tol {
                    c = C_SIG;
                }
                cat[j * nx + i] = c;
            }
        }
        Grid { x, y, cat, er, tand }
    }

    fn base_sizes(&self) -> (f64, f64) {
        let p = self.effective();
        let hmin = p.h1.min(p.h2);
        // of an etched trace the mean width: the corners of a narrow face need no fine mesh all around them
        // beyond what the adaptive refinement gives them, even where the face is almost a point
        let w = p.w - p.undercut();
        let mut feat = vec![w / 60.0, hmin / 60.0];
        let mut narrowest = w.min(hmin);
        // the width of the copper layer the core cells have to resolve: strip and gaps, or three strip widths
        let mut across = 3.0 * p.w;
        if p.coplanar {
            feat.push(p.s / 30.0);
            narrowest = narrowest.min(p.s);
            across = p.w + 2.0 * p.s;
        }
        if p.t > 0.0 {
            feat.push(p.t / 6.0);
        }
        let fmin = feat.into_iter().fold(f64::INFINITY, f64::min);
        let dmin = fmin.max(1e-3 * narrowest); // relative floor
        let dmax = (hmin / 20.0).min(across / 20.0).max(2.0 * dmin);
        (dmin, dmax)
    }

    fn is_closed(&self) -> bool {
        self.closed()
    }

    fn initial_margin(&self) -> f64 {
        let p = self.effective();
        let mut m = (4.0 * p.h1.max(p.h2)).max(p.w + 2.0 * p.gap_or_zero());
        if !(p.plane1 && p.plane2) && p.coplanar {
            m = m.max(p.wall_x());
        }
        m
    }
}
