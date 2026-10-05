//! A rate is always `x/n` with the bound a reader needs, never a bare percent.

use serde::Serialize;

/// `x` of `n`, with its Wilson 95% interval. `n == 0` has no rate: the
/// interval is `None`, and the printer shows a dash.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
pub struct Rate {
    pub x: usize,
    pub n: usize,
    pub lo: Option<f64>,
    pub hi: Option<f64>,
}

impl Rate {
    pub fn new(x: usize, n: usize) -> Rate {
        let (lo, hi) = wilson(x, n).unzip();
        Rate { x, n, lo, hi }
    }

    pub fn pct(&self) -> Option<f64> {
        (self.n > 0).then(|| 100.0 * self.x as f64 / self.n as f64)
    }
}

/// Wilson score interval at 95%: honest at small `n` and at 0/n, where the
/// plain `x/n` ± error would claim certainty.
pub fn wilson(x: usize, n: usize) -> Option<(f64, f64)> {
    if n == 0 {
        return None;
    }
    let (z, n, p) = (1.96_f64, n as f64, x as f64 / n as f64);
    let denom = 1.0 + z * z / n;
    let centre = p + z * z / (2.0 * n);
    let adj = z * (p * (1.0 - p) / n + z * z / (4.0 * n * n)).sqrt();
    Some((
        ((centre - adj) / denom).max(0.0),
        ((centre + adj) / denom).min(1.0),
    ))
}

/// Phi coefficient of two binary variables from their 2×2 counts
/// (a = both, b = only first, c = only second, d = neither).
pub fn phi(a: usize, b: usize, c: usize, d: usize) -> Option<f64> {
    let (a, b, c, d) = (a as f64, b as f64, c as f64, d as f64);
    let den = ((a + b) * (c + d) * (a + c) * (b + d)).sqrt();
    (den > 0.0).then(|| (a * d - b * c) / den)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_of_n_still_has_an_upper_bound_and_no_n_has_none() {
        let r = Rate::new(0, 20);
        assert_eq!(r.lo, Some(0.0));
        assert!((r.hi.unwrap() - 0.161).abs() < 0.002, "{r:?}");
        let r = Rate::new(3, 100);
        assert!((r.hi.unwrap() - 0.0845).abs() < 0.002, "{r:?}");
        assert_eq!(Rate::new(0, 0).hi, None);
        assert_eq!(Rate::new(0, 0).pct(), None);
        assert_eq!(Rate::new(1, 4).pct(), Some(25.0));
    }

    #[test]
    fn phi_is_signed_and_undefined_without_variation() {
        assert_eq!(phi(10, 0, 0, 10), Some(1.0));
        assert_eq!(phi(0, 10, 10, 0), Some(-1.0));
        assert_eq!(phi(5, 5, 5, 5), Some(0.0));
        assert_eq!(phi(5, 0, 5, 0), None);
    }
}
