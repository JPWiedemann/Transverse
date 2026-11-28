//! Field lines: streamlines of E traced from seeds on a coarse occupancy mask, so lines are
//! spread evenly over the view (same idea as matplotlib's `streamplot`).

/// Region and density of the streamline set.
#[derive(Clone, Copy, Debug)]
pub struct StreamOptions {
    /// (x0, x1, y0, y1) of the view [mm].
    pub view: [f64; 4],
    /// Mask cells per axis = 30 * density.
    pub density: (f64, f64),
}

struct Field<'a> {
    x: &'a [f64],
    y: &'a [f64],
    ex: &'a [f64],
    ey: &'a [f64],
    fixed: &'a [bool],
}

impl Field<'_> {
    fn cell(c: &[f64], v: f64) -> usize {
        (c.partition_point(|&k| k <= v).max(1) - 1).min(c.len() - 2)
    }

    fn in_conductor(&self, px: f64, py: f64) -> bool {
        let nearest = |c: &[f64], v: f64| {
            let k = Self::cell(c, v);
            if (v - c[k]).abs() <= (c[k + 1] - v).abs() { k } else { k + 1 }
        };
        self.fixed[nearest(self.y, py) * self.x.len() + nearest(self.x, px)]
    }

    /// Bilinear interpolation of E (NaN in conductors counts as 0).
    fn e_at(&self, px: f64, py: f64) -> (f64, f64) {
        let nx = self.x.len();
        let (i, j) = (Self::cell(self.x, px), Self::cell(self.y, py));
        let tx = ((px - self.x[i]) / (self.x[i + 1] - self.x[i])).clamp(0.0, 1.0);
        let ty = ((py - self.y[j]) / (self.y[j + 1] - self.y[j])).clamp(0.0, 1.0);
        let lerp = |f: &[f64]| {
            let g = |a: usize, b: usize| {
                let v = f[b * nx + a];
                if v.is_nan() { 0.0 } else { v }
            };
            (1.0 - ty) * ((1.0 - tx) * g(i, j) + tx * g(i + 1, j))
                + ty * ((1.0 - tx) * g(i, j + 1) + tx * g(i + 1, j + 1))
        };
        (lerp(self.ex), lerp(self.ey))
    }
}

/// Streamlines of (ex, ey) on the tensor grid inside `opt.view`, oriented along the field. Lines end at
/// conductors, at the view edge, or where they would enter a region already covered by another line.
pub fn field_lines(
    x: &[f64],
    y: &[f64],
    ex: &[f64],
    ey: &[f64],
    fixed: &[bool],
    opt: &StreamOptions,
) -> Vec<Vec<[f64; 2]>> {
    let f = Field { x, y, ex, ey, fixed };
    field_lines_with(
        [x[0], x[x.len() - 1], y[0], y[y.len() - 1]],
        &|p| f.e_at(p[0], p[1]),
        &|p| f.in_conductor(p[0], p[1]),
        opt,
    )
}

/// Streamlines of any field: `bounds` = `[x0, x1, y0, y1]` of the domain, `e_at(p)` the field (components) at a
/// point, `in_conductor(p)` whether the point is not in the dielectric any more (lines end there).
pub fn field_lines_with(
    bounds: [f64; 4],
    e_at: &dyn Fn([f64; 2]) -> (f64, f64),
    in_conductor: &dyn Fn([f64; 2]) -> bool,
    opt: &StreamOptions,
) -> Vec<Vec<[f64; 2]>> {
    let [vx0, vx1, vy0, vy1] = opt.view;
    let (x0, x1) = (vx0.max(bounds[0]), vx1.min(bounds[1]));
    let (y0, y1) = (vy0.max(bounds[2]), vy1.min(bounds[3]));
    if x1 <= x0 || y1 <= y0 {
        return Vec::new();
    }
    let nxm = ((30.0 * opt.density.0).round() as usize).max(2);
    let nym = ((30.0 * opt.density.1).round() as usize).max(2);
    let (cw, ch) = ((x1 - x0) / nxm as f64, (y1 - y0) / nym as f64);
    let mut occupied = vec![false; nxm * nym];
    let mask_idx = |px: f64, py: f64| -> Option<usize> {
        let (u, w) = ((px - x0) / cw, (py - y0) / ch);
        if !(0.0..nxm as f64).contains(&u) || !(0.0..nym as f64).contains(&w) {
            return None;
        }
        Some(w as usize * nxm + u as usize)
    };

    // trace one direction from (px, py); returns the points (excluding the start) and cells used
    let trace =
        |start: [f64; 2], sign: f64, occupied: &mut Vec<bool>, used: &mut Vec<usize>| -> Vec<[f64; 2]> {
            const DS: f64 = 0.2; // step in mask cells
            let max_steps = (8.0 * (nxm + nym) as f64 / DS) as usize;
            let dir = |p: [f64; 2]| -> Option<[f64; 2]> {
                let (ux, uy) = e_at(p);
                // velocity in mask units, unit speed
                let (vx, vy) = (ux / cw, uy / ch);
                let n = vx.hypot(vy);
                (n > 1e-300).then(|| [sign * vx / n, sign * vy / n])
            };
            let mut pts = Vec::new();
            let mut p = start;
            let mut last_cell = mask_idx(p[0], p[1]);
            for _ in 0..max_steps {
                let Some(d1) = dir(p) else { break };
                // midpoint (RK2) step
                let mid = [p[0] + 0.5 * DS * d1[0] * cw, p[1] + 0.5 * DS * d1[1] * ch];
                let d = dir(mid).unwrap_or(d1);
                let q = [p[0] + DS * d[0] * cw, p[1] + DS * d[1] * ch];
                let Some(cell) = mask_idx(q[0], q[1]) else { break };
                if in_conductor(q) {
                    break;
                }
                if Some(cell) != last_cell {
                    if occupied[cell] {
                        break;
                    }
                    occupied[cell] = true;
                    used.push(cell);
                    last_cell = Some(cell);
                }
                pts.push(q);
                p = q;
            }
            pts
        };

    // seeds: cell centres, outer rings first
    let mut seeds: Vec<(usize, usize)> = (0..nym).flat_map(|j| (0..nxm).map(move |i| (i, j))).collect();
    seeds.sort_by_key(|&(i, j)| (i.min(nxm - 1 - i).min(j).min(nym - 1 - j), j, i));

    let mut lines = Vec::new();
    for (i, j) in seeds {
        let k = j * nxm + i;
        if occupied[k] {
            continue;
        }
        let start = [x0 + (i as f64 + 0.5) * cw, y0 + (j as f64 + 0.5) * ch];
        if in_conductor(start) {
            continue;
        }
        occupied[k] = true;
        let mut used = vec![k];
        let fwd = trace(start, 1.0, &mut occupied, &mut used);
        let bwd = trace(start, -1.0, &mut occupied, &mut used);
        if used.len() < 4 {
            for c in used {
                occupied[c] = false;
            }
            occupied[k] = true; // do not retry this seed
            continue;
        }
        let mut line: Vec<[f64; 2]> = bwd.into_iter().rev().collect();
        line.push(start);
        line.extend(fwd);
        lines.push(line);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Uniform field between two plates: lines must be straight and follow +y.
    #[test]
    fn uniform_field_gives_straight_lines_along_e() {
        let x: Vec<f64> = (0..21).map(|i| i as f64 * 0.1).collect();
        let y: Vec<f64> = (0..11).map(|j| j as f64 * 0.1).collect();
        let n = x.len() * y.len();
        let (ex, ey) = (vec![0.0; n], vec![1.0; n]);
        let fixed = vec![false; n];
        let lines = field_lines(
            &x,
            &y,
            &ex,
            &ey,
            &fixed,
            &StreamOptions { view: [0.0, 2.0, 0.0, 1.0], density: (1.0, 1.0) },
        );
        assert!(lines.len() > 5);
        for l in &lines {
            assert!(l.len() > 3);
            let dx = (l[l.len() - 1][0] - l[0][0]).abs();
            assert!(dx < 1e-9, "line is not vertical");
            assert!(l[l.len() - 1][1] > l[0][1], "line is not oriented along E");
        }
    }

    #[test]
    fn lines_do_not_enter_conductors() {
        let x: Vec<f64> = (0..21).map(|i| i as f64 * 0.1).collect();
        let y: Vec<f64> = (0..11).map(|j| j as f64 * 0.1).collect();
        let (nx, n) = (x.len(), x.len() * y.len());
        let mut fixed = vec![false; n];
        for j in 4..7 {
            for i in 8..13 {
                fixed[j * nx + i] = true;
            }
        }
        let (ex, ey) = (vec![0.0; n], vec![1.0; n]);
        let opt = StreamOptions { view: [0.0, 2.0, 0.0, 1.0], density: (2.0, 2.0) };
        let lines = field_lines(&x, &y, &ex, &ey, &fixed, &opt);
        let f = Field { x: &x, y: &y, ex: &ex, ey: &ey, fixed: &fixed };
        assert!(!lines.is_empty());
        assert!(lines.iter().flatten().all(|p| !f.in_conductor(p[0], p[1])));
    }
}
