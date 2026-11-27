//! Shapes of a cross-section.

use serde::{Deserialize, Serialize};

pub type P2 = [f64; 2];

/// A region (`Rect`, `Polygon`) or a zero-width line (`Segment`: a zero-thickness conductor such as a
/// ground plane, a via fence or a trace of thickness 0).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Shape {
    Rect {
        x0: f64,
        y0: f64,
        x1: f64,
        y1: f64,
    },
    /// Simple polygon, vertices in either orientation, implicitly closed.
    Polygon(Vec<P2>),
    Segment {
        a: P2,
        b: P2,
    },
}

impl Shape {
    /// Rectangle from two opposite corners in any order.
    pub fn rect(xa: f64, ya: f64, xb: f64, yb: f64) -> Self {
        Self::Rect { x0: xa.min(xb), y0: ya.min(yb), x1: xa.max(xb), y1: ya.max(yb) }
    }

    pub fn segment(a: P2, b: P2) -> Self {
        Self::Segment { a, b }
    }

    /// Boundary pieces: the sides of a rectangle or polygon, or the segment itself.
    pub fn edges(&self) -> Vec<(P2, P2)> {
        match self {
            Self::Rect { x0, y0, x1, y1 } => {
                let c = [[*x0, *y0], [*x1, *y0], [*x1, *y1], [*x0, *y1]];
                (0..4).map(|k| (c[k], c[(k + 1) % 4])).collect()
            }
            Self::Polygon(p) => (0..p.len()).map(|k| (p[k], p[(k + 1) % p.len()])).collect(),
            Self::Segment { a, b } => vec![(*a, *b)],
        }
    }

    /// Whether the point is strictly inside a region. A segment has no inside.
    pub fn contains(&self, p: P2) -> bool {
        match self {
            Self::Rect { x0, y0, x1, y1 } => p[0] > *x0 && p[0] < *x1 && p[1] > *y0 && p[1] < *y1,
            Self::Polygon(v) => {
                // even-odd rule
                let mut inside = false;
                for k in 0..v.len() {
                    let (a, b) = (v[k], v[(k + 1) % v.len()]);
                    if (a[1] > p[1]) != (b[1] > p[1]) {
                        let x = a[0] + (p[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0]);
                        if p[0] < x {
                            inside = !inside;
                        }
                    }
                }
                inside
            }
            Self::Segment { .. } => false,
        }
    }

    /// Area (0 for a segment).
    pub fn area(&self) -> f64 {
        match self {
            Self::Rect { x0, y0, x1, y1 } => (x1 - x0) * (y1 - y0),
            Self::Polygon(v) => {
                let twice: f64 = (0..v.len())
                    .map(|k| {
                        let (a, b) = (v[k], v[(k + 1) % v.len()]);
                        a[0] * b[1] - b[0] * a[1]
                    })
                    .sum();
                twice.abs() / 2.0
            }
            Self::Segment { .. } => 0.0,
        }
    }

    /// `[x0, x1, y0, y1]`
    pub fn bounds(&self) -> [f64; 4] {
        let pts: Vec<P2> = match self {
            Self::Rect { x0, y0, x1, y1 } => vec![[*x0, *y0], [*x1, *y1]],
            Self::Polygon(v) => v.clone(),
            Self::Segment { a, b } => vec![*a, *b],
        };
        let f = |k: usize, m: fn(f64, f64) -> f64, init: f64| pts.iter().fold(init, |s, p| m(s, p[k]));
        [
            f(0, f64::min, f64::INFINITY),
            f(0, f64::max, f64::NEG_INFINITY),
            f(1, f64::min, f64::INFINITY),
            f(1, f64::max, f64::NEG_INFINITY),
        ]
    }

    pub(crate) fn is_degenerate(&self) -> bool {
        match self {
            Self::Rect { x0, y0, x1, y1 } => !(x1 > x0 && y1 > y0),
            Self::Polygon(v) => v.len() < 3 || self.area() == 0.0,
            Self::Segment { a, b } => a == b,
        }
    }

    pub(crate) fn all_finite(&self) -> bool {
        match self {
            Self::Rect { x0, y0, x1, y1 } => [x0, y0, x1, y1].iter().all(|v| v.is_finite()),
            Self::Polygon(v) => v.iter().all(|p| p[0].is_finite() && p[1].is_finite()),
            Self::Segment { a, b } => a.iter().chain(b).all(|v| v.is_finite()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_normalises_its_corners_and_has_four_edges() {
        let r = Shape::rect(3.0, 2.0, 1.0, -1.0);
        assert_eq!(r, Shape::Rect { x0: 1.0, y0: -1.0, x1: 3.0, y1: 2.0 });
        assert_eq!(r.edges().len(), 4);
        assert_eq!(r.area(), 6.0);
        assert_eq!(r.bounds(), [1.0, 3.0, -1.0, 2.0]);
    }

    #[test]
    fn containment_is_strict_and_a_segment_contains_nothing() {
        let r = Shape::rect(0.0, 0.0, 2.0, 1.0);
        assert!(r.contains([1.0, 0.5]) && !r.contains([2.0, 0.5]) && !r.contains([3.0, 0.5]));
        assert!(!Shape::segment([0.0, 0.0], [1.0, 0.0]).contains([0.5, 0.0]));
        // a triangle, either orientation
        for t in [vec![[0.0, 0.0], [4.0, 0.0], [0.0, 4.0]], vec![[0.0, 0.0], [0.0, 4.0], [4.0, 0.0]]] {
            let p = Shape::Polygon(t);
            assert!(p.contains([1.0, 1.0]) && !p.contains([3.0, 3.0]));
            assert!((p.area() - 8.0).abs() < 1e-12);
        }
    }

    #[test]
    fn degenerate_and_non_finite_shapes_are_detected() {
        assert!(Shape::Rect { x0: 1.0, y0: 0.0, x1: 1.0, y1: 1.0 }.is_degenerate());
        assert!(Shape::segment([1.0, 1.0], [1.0, 1.0]).is_degenerate());
        assert!(Shape::Polygon(vec![[0.0, 0.0], [1.0, 1.0]]).is_degenerate());
        assert!(!Shape::rect(0.0, 0.0, 1.0, 1.0).is_degenerate());
        assert!(!Shape::segment([0.0, f64::NAN], [1.0, 0.0]).all_finite());
    }
}
