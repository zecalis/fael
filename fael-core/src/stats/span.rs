//! Shared span helpers for the stats time views (`flow`, and the outcome
//! join landing next): one median/p90/n span plus the Monday-week calendar.
//! Pure: numbers in, numbers out — no usage, log or clock read here.

use crate::rfc3339;
use serde::Serialize;

/// One span: median + p90 in whole seconds over `n` measured durations.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Span {
    pub median: u64,
    pub p90: u64,
    pub n: usize,
}

pub(super) fn span_of(mut secs: Vec<u64>) -> Span {
    secs.sort_unstable();
    let n = secs.len();
    if n == 0 {
        return Span::default();
    }
    let median = secs[n / 2];
    let p90 = secs[(n * 90).div_ceil(100).saturating_sub(1).min(n - 1)];
    Span { median, p90, n }
}

/// Monday (UTC) of `ms` as days since the epoch.
pub(super) fn monday_days(ms: i64) -> i64 {
    let days = ms.div_euclid(86_400_000);
    days - (days + 3).rem_euclid(7)
}

pub(super) fn day_string(days: i64) -> String {
    rfc3339((days * 86_400_000).max(0) as u64)
        .get(..10)
        .unwrap_or("")
        .to_string()
}
