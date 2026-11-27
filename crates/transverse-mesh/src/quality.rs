//! Mesh statistics (used by the tests and by diagnostics).

use crate::Mesh;

/// Smallest angle of any triangle [deg].
pub fn min_angle_deg(m: &Mesh) -> f64 {
    let mut min = 180.0f64;
    for t in &m.tris {
        let p = [m.pts[t[0] as usize], m.pts[t[1] as usize], m.pts[t[2] as usize]];
        for k in 0..3 {
            let (a, b, c) = (p[k], p[(k + 1) % 3], p[(k + 2) % 3]);
            let (u, v) = ([b[0] - a[0], b[1] - a[1]], [c[0] - a[0], c[1] - a[1]]);
            let cos = (u[0] * v[0] + u[1] * v[1]) / (u[0].hypot(u[1]) * v[0].hypot(v[1]));
            min = min.min(cos.clamp(-1.0, 1.0).acos().to_degrees());
        }
    }
    min
}

/// Area of triangle `k`.
pub fn tri_area(m: &Mesh, k: usize) -> f64 {
    let t = m.tris[k];
    let (a, b, c) = (m.pts[t[0] as usize], m.pts[t[1] as usize], m.pts[t[2] as usize]);
    0.5 * ((b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1]))
}

/// Total area of all triangles.
pub fn total_area(m: &Mesh) -> f64 {
    (0..m.tris.len()).map(|k| tri_area(m, k)).sum()
}

/// Largest ratio of the longest edges of two triangles that share an edge (grading quality).
pub fn max_neighbour_edge_ratio(m: &Mesh) -> f64 {
    use std::collections::HashMap;
    let longest = |t: &[u32; 3]| {
        (0..3)
            .map(|k| {
                let (a, b) = (m.pts[t[k] as usize], m.pts[t[(k + 1) % 3] as usize]);
                (a[0] - b[0]).hypot(a[1] - b[1])
            })
            .fold(0.0, f64::max)
    };
    let mut by_edge: HashMap<(u32, u32), Vec<usize>> = HashMap::new();
    for (k, t) in m.tris.iter().enumerate() {
        for j in 0..3 {
            let (a, b) = (t[j], t[(j + 1) % 3]);
            by_edge.entry((a.min(b), a.max(b))).or_default().push(k);
        }
    }
    let mut worst = 1.0f64;
    for tris in by_edge.values().filter(|v| v.len() == 2) {
        let (l0, l1) = (longest(&m.tris[tris[0]]), longest(&m.tris[tris[1]]));
        worst = worst.max(l0.max(l1) / l0.min(l1));
    }
    worst
}
