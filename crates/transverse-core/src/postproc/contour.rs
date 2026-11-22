//! Equipotential lines by marching squares on the tensor grid.

/// Line segments of the iso-line `v == level`, linearly interpolated along cell edges.
pub fn contour_segments(v: &[f64], x: &[f64], y: &[f64], level: f64) -> Vec<[[f64; 2]; 2]> {
    let (nx, ny) = (x.len(), y.len());
    let mut out = Vec::new();
    let at = |i: usize, j: usize| v[j * nx + i];
    for j in 0..ny - 1 {
        for i in 0..nx - 1 {
            // corners: 0 = (i, j), 1 = (i+1, j), 2 = (i+1, j+1), 3 = (i, j+1)
            let c = [at(i, j), at(i + 1, j), at(i + 1, j + 1), at(i, j + 1)];
            let p = [[x[i], y[j]], [x[i + 1], y[j]], [x[i + 1], y[j + 1]], [x[i], y[j + 1]]];
            if c.iter().any(|a| !a.is_finite()) {
                continue;
            }
            let above = |k: usize| c[k] >= level;
            // crossing point on edge k (between corner k and k+1)
            let cross = |k: usize| {
                let (a, b) = (k, (k + 1) % 4);
                let t = (level - c[a]) / (c[b] - c[a]);
                [p[a][0] + t * (p[b][0] - p[a][0]), p[a][1] + t * (p[b][1] - p[a][1])]
            };
            let edges: Vec<usize> = (0..4).filter(|&k| above(k) != above((k + 1) % 4)).collect();
            match edges.len() {
                2 => out.push([cross(edges[0]), cross(edges[1])]),
                4 => {
                    // saddle: decide by the cell centre value
                    let centre_above = c.iter().sum::<f64>() / 4.0 >= level;
                    // the centre connects corners 0 and 2 (isolating 1 and 3) or 1 and 3 (isolating 0 and 2)
                    let pairs = if centre_above == above(0) { [(0, 1), (2, 3)] } else { [(3, 0), (1, 2)] };
                    for (a, b) in pairs {
                        out.push([cross(a), cross(b)]);
                    }
                }
                _ => {}
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contour_of_linear_field_is_a_straight_line() {
        let x = vec![0.0, 0.2, 0.5, 1.0];
        let y = vec![0.0, 0.3, 1.0];
        let v: Vec<f64> = (0..12).map(|k| x[k % 4]).collect(); // V = x
        let segs = contour_segments(&v, &x, &y, 0.35);
        assert_eq!(segs.len(), 2); // one segment per row of cells that crosses x = 0.35
        for s in &segs {
            for p in s {
                assert!((p[0] - 0.35).abs() < 1e-12);
            }
        }
        assert!(contour_segments(&v, &x, &y, 2.0).is_empty());
    }

    #[test]
    fn saddle_cell_gives_two_segments() {
        let (x, y) = (vec![0.0, 1.0], vec![0.0, 1.0]);
        let v = [1.0, 0.0, 0.0, 1.0]; // (0,0)=1, (1,0)=0, (0,1)=0, (1,1)=1
        assert_eq!(contour_segments(&v, &x, &y, 0.5).len(), 2);
    }
}
