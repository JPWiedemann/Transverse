//! Special functions.

/// Complete elliptic integral of the first kind K(m) (parameter m = k²), 0 <= m < 1,
/// via the arithmetic-geometric mean.
pub fn ellipk(m: f64) -> f64 {
    let mut a = 1.0;
    let mut b = (1.0 - m).sqrt();
    for _ in 0..60 {
        let an = 0.5 * (a + b);
        let bn = (a * b).sqrt();
        let done = (an - bn).abs() <= 1e-16 * an;
        a = an;
        b = bn;
        if done {
            break;
        }
    }
    std::f64::consts::PI / (2.0 * a)
}

/// K(k)/K(k') with k' = sqrt(1 - k²).
pub fn k_ratio(k: f64) -> f64 {
    ellipk(k * k) / ellipk(1.0 - k * k)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ellipk_known_values() {
        // K(0) = pi/2, K(0.5) = 1.8540746773013719 (parameter convention)
        assert!((ellipk(0.0) - std::f64::consts::FRAC_PI_2).abs() < 1e-15);
        assert!((ellipk(0.5) / 1.854_074_677_301_371_9 - 1.0).abs() < 1e-14);
    }

    #[test]
    fn ratio_is_one_at_k_inverse_sqrt2() {
        assert!((k_ratio(std::f64::consts::FRAC_1_SQRT_2) - 1.0).abs() < 1e-14);
    }
}
