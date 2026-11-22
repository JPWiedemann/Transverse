//! Field views as pixel images: colour-mapped |E|, potential, permittivity and mesh, plus a
//! probe for the cursor readout. Pure functions on [`FieldData`], so they are testable without a window.

use transverse_core::postproc::{EField, percentile};
use transverse_lines::model::{Bounds, FieldData};

use crate::colormap::Cmap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldMode {
    /// |E| in dB with field lines.
    EDbLines,
    EDb,
    ELinear,
    Potential,
    Permittivity,
    Mesh,
}

impl FieldMode {
    pub const ALL: [FieldMode; 6] =
        [Self::EDbLines, Self::EDb, Self::ELinear, Self::Potential, Self::Permittivity, Self::Mesh];

    pub fn label(self) -> &'static str {
        match self {
            Self::EDbLines => "|E| [dB] + field lines",
            Self::EDb => "|E| [dB]",
            Self::ELinear => "|E| (linear)",
            Self::Potential => "Potential",
            Self::Permittivity => "Permittivity er",
            Self::Mesh => "Mesh",
        }
    }

    /// Stable name for the settings file.
    pub fn key(self) -> &'static str {
        match self {
            Self::EDbLines => "e_db_lines",
            Self::EDb => "e_db",
            Self::ELinear => "e_linear",
            Self::Potential => "potential",
            Self::Permittivity => "permittivity",
            Self::Mesh => "mesh",
        }
    }

    pub fn from_key(k: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.key() == k)
    }

    pub fn has_field_lines(self) -> bool {
        self == Self::EDbLines
    }

    pub fn is_db(self) -> bool {
        matches!(self, Self::EDb | Self::EDbLines)
    }

    /// Equipotentials are drawn on the field views, not on permittivity / mesh.
    pub fn allows_equipotentials(self) -> bool {
        !matches!(self, Self::Permittivity | Self::Mesh)
    }
}

/// Colour bar description.
#[derive(Clone, Debug)]
pub struct Legend {
    pub label: String,
    pub vmin: f64,
    pub vmax: f64,
    pub cmap: Cmap,
}

pub struct Raster {
    pub width: usize,
    pub height: usize,
    /// RGBA8, row-major, first row = top of the view (largest y).
    pub rgba: Vec<u8>,
    pub legend: Option<Legend>,
}

#[derive(Clone, Copy, Debug)]
pub struct RasterSpec {
    pub mode: FieldMode,
    /// Region to draw.
    pub view: Bounds,
    /// Region that sets the colour scale (99.7 % percentile of |E|); keeps colours stable while the
    /// drawn region changes with zoom and pan.
    pub scale_view: Bounds,
    pub db_range: f64,
    /// Image width in pixels; the height follows the aspect ratio of the view.
    pub width: usize,
}

/// Cell index and position inside the cell (0..1) of coordinate `v` on grid lines `c`.
fn locate(c: &[f64], v: f64) -> (usize, f64) {
    let i = (c.partition_point(|&k| k <= v).max(1) - 1).min(c.len() - 2);
    (i, ((v - c[i]) / (c[i + 1] - c[i])).clamp(0.0, 1.0))
}

fn bilerp(s: &[f64], nx: usize, i: usize, j: usize, tx: f64, ty: f64) -> f64 {
    let g = |a: usize, b: usize| s[b * nx + a];
    (1.0 - ty) * ((1.0 - tx) * g(i, j) + tx * g(i + 1, j))
        + ty * ((1.0 - tx) * g(i, j + 1) + tx * g(i + 1, j + 1))
}

/// Bilinear interpolation that ignores NaN corners (conductor nodes): the remaining corners are
/// re-weighted, so the colours continue smoothly up to a conductor surface. NaN if no corner is valid.
fn bilerp_valid(s: &[f64], nx: usize, i: usize, j: usize, tx: f64, ty: f64) -> f64 {
    let corners = [
        (s[j * nx + i], (1.0 - tx) * (1.0 - ty)),
        (s[j * nx + i + 1], tx * (1.0 - ty)),
        (s[(j + 1) * nx + i], (1.0 - tx) * ty),
        (s[(j + 1) * nx + i + 1], tx * ty),
    ];
    let (mut sum, mut weight, mut n, mut plain) = (0.0, 0.0, 0, 0.0);
    for (v, w) in corners {
        if !v.is_nan() {
            sum += v * w;
            weight += w;
            n += 1;
            plain += v;
        }
    }
    match n {
        0 => f64::NAN,
        _ if weight > 1e-12 => sum / weight,
        _ => plain / n as f64, // exactly on an invalid corner's opposite node: weights vanish
    }
}

enum Source {
    /// Nodal values, interpolated; cells without any valid corner are transparent.
    Nodes(Vec<f64>),
    /// One value per cell.
    Cells,
}

pub fn render(f: &FieldData, e: &EField, spec: &RasterSpec) -> Raster {
    let (nx, ny) = (f.nx(), f.ny());
    let [vx0, vx1, vy0, vy1] = spec.view;
    let (x0, x1) = (vx0.max(f.x[0]), vx1.min(f.x[nx - 1]));
    let (y0, y1) = (vy0.max(f.y[0]), vy1.min(f.y[ny - 1]));
    if x1 <= x0 || y1 <= y0 {
        return Raster { width: 1, height: 1, rgba: vec![0; 4], legend: None };
    }
    let width = spec.width.max(8);
    let height = ((width as f64 * (y1 - y0) / (x1 - x0)).round() as usize).clamp(8, 4000);

    // quantity and colour scale
    let [sx0, sx1, sy0, sy1] = spec.scale_view;
    let in_view = |k: usize| {
        let (x, y) = (f.x[k % nx], f.y[k / nx]);
        x >= sx0 && x <= sx1 && y >= sy0 && y <= sy1
    };
    let e_hi = || {
        let view = percentile((0..e.e.len()).filter(|&k| in_view(k)).map(|k| e.e[k]), 99.7);
        view.or_else(|| percentile(e.e.iter().copied(), 99.7)).unwrap_or(1.0).max(1e-30)
    };
    let (source, vmin, vmax, cmap, label) = match spec.mode {
        FieldMode::EDb | FieldMode::EDbLines => {
            let top = 20.0 * e_hi().log10();
            let lo = top - spec.db_range;
            // NaN (conductor nodes) must stay NaN: `f64::max` would turn it into the minimum
            let s = e
                .e
                .iter()
                .map(|&v| if v.is_nan() { f64::NAN } else { (20.0 * v.max(1e-30).log10()).clamp(lo, top) })
                .collect();
            (Source::Nodes(s), lo, top, Cmap::Inferno, "|E| [dB re 1 V/mm]".to_string())
        }
        FieldMode::ELinear => {
            let hi = e_hi();
            let s = e.e.iter().map(|&v| v.clamp(0.0, hi)).collect();
            (Source::Nodes(s), 0.0, hi, Cmap::Inferno, "|E| [V/mm]".to_string())
        }
        FieldMode::Potential => {
            let (lo, hi) =
                f.v.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |a, &v| (a.0.min(v), a.1.max(v)));
            (Source::Nodes(f.v.clone()), lo, hi, Cmap::Viridis, "V [V]".to_string())
        }
        FieldMode::Permittivity => {
            let (lo, hi) =
                f.er.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |a, &v| (a.0.min(v), a.1.max(v)));
            (Source::Cells, lo, if hi > lo { hi } else { lo + 1.0 }, Cmap::Viridis, "er".to_string())
        }
        FieldMode::Mesh => {
            let hi = f.er.iter().fold(0.0f64, |a, &v| a.max(v));
            (Source::Cells, 0.0, 3.0 * hi, Cmap::Greys, String::new())
        }
    };
    let span = (vmax - vmin).max(f64::MIN_POSITIVE);

    let cols: Vec<(usize, f64)> =
        (0..width).map(|i| locate(&f.x, x0 + (i as f64 + 0.5) / width as f64 * (x1 - x0))).collect();
    let mut rgba = vec![0u8; width * height * 4];
    for j in 0..height {
        let (cj, ty) = locate(&f.y, y1 - (j as f64 + 0.5) / height as f64 * (y1 - y0));
        for (i, &(ci, tx)) in cols.iter().enumerate() {
            let v = match &source {
                Source::Nodes(s) => bilerp_valid(s, nx, ci, cj, tx, ty), // NaN only inside conductors
                Source::Cells => f.er[cj * (nx - 1) + ci],
            };
            if v.is_nan() {
                continue;
            }
            let c = cmap.sample((v - vmin) / span);
            let o = (j * width + i) * 4;
            rgba[o..o + 4].copy_from_slice(&[c[0], c[1], c[2], 255]);
        }
    }
    let legend = (spec.mode != FieldMode::Mesh).then_some(Legend { label, vmin, vmax, cmap });
    Raster { width, height, rgba, legend }
}

/// Values under the cursor.
#[derive(Clone, Copy, Debug)]
pub struct Probe {
    pub v: f64,
    /// |E| [V/mm] (NaN next to a conductor surface).
    pub e: f64,
    pub er: f64,
    pub in_conductor: bool,
}

pub fn probe(f: &FieldData, e: &EField, x: f64, y: f64) -> Option<Probe> {
    let (nx, ny) = (f.nx(), f.ny());
    if x < f.x[0] || x > f.x[nx - 1] || y < f.y[0] || y > f.y[ny - 1] {
        return None;
    }
    let (i, tx) = locate(&f.x, x);
    let (j, ty) = locate(&f.y, y);
    let near = |a: usize, t: f64| if t < 0.5 { a } else { a + 1 };
    Some(Probe {
        v: bilerp(&f.v, nx, i, j, tx, ty),
        e: bilerp_valid(&e.e, nx, i, j, tx, ty),
        er: f.er[j * (nx - 1) + i],
        in_conductor: f.fixed[near(j, ty) * nx + near(i, tx)],
    })
}

/// "Nice" tick values (1, 2, 5 × 10^k) between `lo` and `hi`, about `n` of them.
pub fn nice_ticks(lo: f64, hi: f64, n: usize) -> Vec<f64> {
    if hi.is_nan() || lo.is_nan() || hi <= lo || n == 0 {
        return vec![lo];
    }
    let raw = (hi - lo) / n as f64;
    let mag = 10f64.powf(raw.log10().floor());
    let step = [1.0, 2.0, 5.0, 10.0].into_iter().map(|m| m * mag).find(|s| *s >= raw).unwrap_or(10.0 * mag);
    let first = (lo / step).ceil();
    (0..).map(|k| (first + k as f64) * step).take_while(|t| *t <= hi + 1e-9 * step).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use transverse_core::postproc::e_field;
    use transverse_lines::model::{DrawRect, ShapeKind};

    /// 3 x 3 nodes, x = 0, 1, 3 / y = 0, 2, 3; V = x / 3 (a uniform field in x); the cell between nodes (1..2, 1..2) lies inside a conductor.
    fn tiny() -> (FieldData, EField) {
        let x = vec![0.0, 1.0, 3.0];
        let y = vec![0.0, 2.0, 3.0];
        let v: Vec<f64> = (0..9).map(|k| x[k % 3] / 3.0).collect();
        let mut fixed = vec![false; 9];
        for k in [4, 5, 7, 8] {
            fixed[k] = true;
        }
        let f = FieldData {
            x: x.clone(),
            y: y.clone(),
            fixed: fixed.clone(),
            er: vec![1.0, 4.0, 1.0, 4.0],
            v: v.clone(),
            v0: v.clone(),
            conductors: Vec::<DrawRect>::new(),
            dashed_y: vec![],
            dotted_y: vec![],
            view_zoom: [0.0, 3.0, 0.0, 3.0],
            view_full: [0.0, 3.0, 0.0, 3.0],
            z0: 50.0,
        };
        let e = e_field(&v, &x, &y, &fixed);
        let _ = ShapeKind::Ground;
        (f, e)
    }

    fn spec(mode: FieldMode) -> RasterSpec {
        RasterSpec {
            mode,
            view: [0.0, 3.0, 0.0, 3.0],
            scale_view: [0.0, 3.0, 0.0, 3.0],
            db_range: 40.0,
            width: 30,
        }
    }

    fn px(r: &Raster, i: usize, j: usize) -> [u8; 4] {
        let o = (j * r.width + i) * 4;
        [r.rgba[o], r.rgba[o + 1], r.rgba[o + 2], r.rgba[o + 3]]
    }

    #[test]
    fn image_size_follows_the_view_aspect() {
        let (f, e) = tiny();
        let r = render(&f, &e, &spec(FieldMode::Potential));
        assert_eq!((r.width, r.height), (30, 30));
        let r = render(&f, &e, &RasterSpec { view: [0.0, 3.0, 0.0, 1.5], ..spec(FieldMode::Potential) });
        assert_eq!((r.width, r.height), (30, 15));
        // view outside the grid is clipped; a view without overlap gives an empty image
        let r = render(&f, &e, &RasterSpec { view: [5.0, 6.0, 0.0, 1.0], ..spec(FieldMode::Potential) });
        assert_eq!((r.width, r.height, r.rgba[3]), (1, 1, 0));
    }

    #[test]
    fn potential_runs_through_the_colour_map_and_conductors_are_transparent_in_e_modes() {
        let (f, e) = tiny();
        let r = render(&f, &e, &spec(FieldMode::Potential));
        let (lo, hi) = (px(&r, 0, 15), px(&r, 29, 15));
        assert_eq!(lo[3], 255);
        assert!(lo[0] < 90 && hi[0] > 230, "viridis runs from purple to yellow: {lo:?} {hi:?}");
        // E modes: the cell whose four corners are conductor nodes is transparent
        let r = render(&f, &e, &spec(FieldMode::ELinear));
        assert_eq!(px(&r, 28, 1)[3], 0, "pixel in the cell inside the conductor");
        assert_eq!(px(&r, 2, 28)[3], 255, "pixel far from it");
    }

    #[test]
    fn cells_touching_a_conductor_continue_from_their_valid_corners() {
        // corners (0,0)=1, (1,0)=NaN, (0,1)=3, (1,1)=5 in a single cell
        let s = [1.0, f64::NAN, 3.0, 5.0];
        assert!((bilerp_valid(&s, 2, 0, 0, 0.5, 0.5) - 3.0).abs() < 1e-12, "mean of the valid corners");
        assert!((bilerp_valid(&s, 2, 0, 0, 0.0, 0.0) - 1.0).abs() < 1e-12, "at a valid node");
        assert!(
            (bilerp_valid(&s, 2, 0, 0, 1.0, 0.0) - 3.0).abs() < 1e-12,
            "on the invalid corner: neighbours' mean"
        );
        assert!(bilerp_valid(&[f64::NAN; 4], 2, 0, 0, 0.5, 0.5).is_nan());
        // in the image such a cell is opaque and no longer fades to the background
        let (f, e) = tiny();
        let r = render(&f, &e, &spec(FieldMode::ELinear));
        assert_eq!(px(&r, 2, 28)[3], 255);
        assert_eq!(px(&r, 8, 8)[3], 255, "cell (0,0) touches conductor node 4 but has valid corners");
    }

    #[test]
    fn conductor_nodes_do_not_darken_the_neighbouring_cells_in_db_mode() {
        let (f, e) = tiny();
        // uniform |E| everywhere outside the conductor: every opaque pixel must have the same colour
        for mode in [FieldMode::EDb, FieldMode::ELinear] {
            let r = render(&f, &e, &spec(mode));
            let reference = px(&r, 2, 28);
            let mut opaque = 0;
            for j in 0..r.height {
                for i in 0..r.width {
                    let p = px(&r, i, j);
                    if p[3] != 0 {
                        opaque += 1;
                        assert_eq!(p, reference, "{mode:?}: pixel ({i},{j}) differs near the conductor");
                    }
                }
            }
            assert!(
                opaque > 0 && opaque < r.width * r.height,
                "{mode:?}: conductor cell must be transparent"
            );
            assert_eq!(px(&r, 28, 1)[3], 0, "{mode:?}");
        }
    }

    #[test]
    fn colour_scale_comes_from_the_scale_region_not_the_drawn_region() {
        let (f, e) = tiny();
        let a = render(&f, &e, &spec(FieldMode::EDb)).legend.unwrap();
        // drawing only a corner must not change the colour scale
        let b = render(&f, &e, &RasterSpec { view: [0.0, 1.0, 0.0, 1.0], ..spec(FieldMode::EDb) })
            .legend
            .unwrap();
        assert_eq!((a.vmin, a.vmax), (b.vmin, b.vmax));
    }

    #[test]
    fn db_scale_is_clipped_to_the_range_below_the_percentile() {
        let (f, e) = tiny();
        let r = render(&f, &e, &spec(FieldMode::EDb));
        let l = r.legend.unwrap();
        assert!((l.vmax - l.vmin - 40.0).abs() < 1e-12);
        // uniform E = 1/3 V/mm up to the conductor: top of the scale is about 20 log10(1/3)
        assert!((l.vmax - 20.0 * (1.0f64 / 3.0).log10()).abs() < 0.5, "top = {}", l.vmax);
        assert_eq!(l.cmap, Cmap::Inferno);
    }

    #[test]
    fn cell_modes_show_the_permittivity() {
        let (f, e) = tiny();
        let r = render(&f, &e, &spec(FieldMode::Permittivity));
        let l = r.legend.as_ref().unwrap();
        assert_eq!((l.vmin, l.vmax), (1.0, 4.0));
        // left cell column (x < 1) has er 1 (lowest colour), second column er 4 (highest)
        let (a, b) = (px(&r, 2, 2), px(&r, 20, 2));
        assert_eq!(a[..3], Cmap::Viridis.sample(0.0));
        assert_eq!(b[..3], Cmap::Viridis.sample(1.0));
        let m = render(&f, &e, &spec(FieldMode::Mesh));
        assert!(m.legend.is_none());
        assert_eq!(px(&m, 2, 2)[..3], Cmap::Greys.sample(1.0 / 12.0));
    }

    #[test]
    fn probe_reports_values_and_conductors() {
        let (f, e) = tiny();
        let p = probe(&f, &e, 0.5, 0.5).unwrap();
        assert!((p.v - 0.5 / 3.0).abs() < 1e-12 && p.er == 1.0 && !p.in_conductor);
        assert!((p.e - 1.0 / 3.0).abs() < 1e-9);
        let c = probe(&f, &e, 2.5, 2.5).unwrap();
        assert!(c.in_conductor && c.e.is_nan() && c.er == 4.0);
        assert!(probe(&f, &e, -1.0, 0.0).is_none() && probe(&f, &e, 0.0, 3.5).is_none());
    }

    #[test]
    fn ticks_are_nice_and_inside_the_range() {
        assert_eq!(nice_ticks(0.0, 1.0, 5), vec![0.0, 0.2, 0.4, 0.6000000000000001, 0.8, 1.0]);
        let t = nice_ticks(-43.7, -3.7, 5);
        assert!(
            t.iter().all(|v| *v >= -43.7 && *v <= -3.7)
                && t.windows(2).all(|w| (w[1] - w[0] - 10.0).abs() < 1e-9)
        );
        assert_eq!(nice_ticks(2.0, 2.0, 5), vec![2.0]);
    }

    #[test]
    fn mode_keys_round_trip() {
        for m in FieldMode::ALL {
            assert_eq!(FieldMode::from_key(m.key()), Some(m));
        }
        assert_eq!(FieldMode::from_key("nonsense"), None);
        assert!(FieldMode::EDbLines.has_field_lines() && !FieldMode::EDb.has_field_lines());
        assert!(FieldMode::EDb.is_db() && !FieldMode::Potential.is_db());
        assert!(!FieldMode::Mesh.allows_equipotentials());
    }
}
