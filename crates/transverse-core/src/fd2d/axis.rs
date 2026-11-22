//! Graded 1-D point sets (ported 1:1 from the Python reference).

fn linspace(a: f64, b: f64, n: usize) -> Vec<f64> {
    // numpy semantics: a + i*step, last point exactly b
    let step = (b - a) / n as f64;
    let mut v: Vec<f64> = (0..=n).map(|i| a + i as f64 * step).collect();
    v[n] = b;
    v
}

fn sorted_unique(mut v: Vec<f64>) -> Vec<f64> {
    v.sort_by(|a, b| a.total_cmp(b));
    v.dedup();
    v
}

/// Points from `a` to `b`, spacing `dmin` at both ends growing by `r` up to `dmax`.
pub fn graded(a: f64, b: f64, dmin: f64, dmax: f64) -> Vec<f64> {
    const R: f64 = 1.2;
    let len = b - a;
    if len <= 0.0 {
        return vec![a];
    }
    if 2.0 * dmin >= len {
        let n = ((len / dmin).ceil() as usize).max(1);
        return linspace(a, b, n);
    }
    let mut steps = Vec::new();
    let (mut st, mut tot) = (dmin, 0.0);
    while tot + st < len / 2.0 {
        steps.push(st);
        tot += st;
        st = (st * R).min(dmax);
    }
    let mut cs = vec![0.0];
    let mut acc = 0.0;
    for s in &steps {
        acc += s;
        cs.push(acc);
    }
    let left: Vec<f64> = cs.iter().map(|c| a + c).collect();
    let right: Vec<f64> = cs.iter().map(|c| b - c).collect();
    let (l_last, r_last) = (*left.last().unwrap(), *right.last().unwrap());
    let mid_gap = r_last - l_last;
    let mut pts = left;
    pts.extend(right.iter().rev());
    if mid_gap > dmax {
        let n = (mid_gap / dmax).ceil() as usize;
        let mid = linspace(l_last, r_last, n);
        pts.extend(&mid[1..n]);
    }
    let pts = sorted_unique(pts);
    let mut keep = vec![pts[0]];
    for &x in &pts[1..pts.len() - 1] {
        if x - keep.last().unwrap() > 0.3 * dmin {
            keep.push(x);
        }
    }
    let last = *pts.last().unwrap();
    if last - keep.last().unwrap() < 0.3 * dmin && keep.len() > 1 {
        keep.pop();
    }
    keep.push(last);
    keep
}

/// Points from `a` towards `b` (either direction), first step `d0` growing by 1.25 up to `dcap`.
pub fn graded_out(a: f64, b: f64, d0: f64, dcap: f64) -> Vec<f64> {
    const R: f64 = 1.25;
    let sgn = if b > a { 1.0 } else { -1.0 };
    let len = (b - a).abs();
    let (mut pos, mut st) = (0.0, d0);
    let mut pts = vec![a];
    while pos + st < len {
        pos += st;
        pts.push(a + sgn * pos);
        st = (st * R).min(dcap);
    }
    if len - pos < 0.3 * st && pts.len() > 1 {
        pts.pop();
    }
    pts.push(b);
    pts
}

/// Grid lines through all `breaks` (rounded to 12 decimals), graded in between.
pub fn axis(breaks: &[f64], dmin: f64, dmax: f64) -> Vec<f64> {
    let b = sorted_unique(breaks.iter().map(|v| (v * 1e12).round_ties_even() / 1e12).collect());
    let mut out = vec![b[0]];
    for w in b.windows(2) {
        out.extend(&graded(w[0], w[1], dmin, dmax)[1..]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graded_hits_both_ends_and_is_increasing() {
        let p = graded(0.0, 1.0, 0.01, 0.1);
        assert_eq!(p[0], 0.0);
        assert_eq!(*p.last().unwrap(), 1.0);
        assert!(p.windows(2).all(|w| w[1] > w[0]));
        // first and last cell have the minimum size
        assert!((p[1] - 0.01).abs() < 1e-12 && (1.0 - p[p.len() - 2] - 0.01).abs() < 1e-12);
    }

    #[test]
    fn graded_short_interval_is_uniform() {
        let p = graded(0.0, 0.015, 0.01, 0.1);
        assert_eq!(p.len(), 3);
    }

    #[test]
    fn graded_out_grows_towards_b() {
        let p = graded_out(1.0, -2.0, 0.01, 0.5);
        assert_eq!(p[0], 1.0);
        assert_eq!(*p.last().unwrap(), -2.0);
        assert!(p.windows(2).all(|w| w[1] < w[0]));
    }
}
