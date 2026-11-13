//! Hook wall time per event: p50, p95 and n over the usage rows that carry
//! `ms` (`protocol::cmd` entry → the row's write). Only hooks that put
//! something in context write a row, so silent hooks are not in here.

use super::Parsed;
use serde::Serialize;
use std::collections::BTreeMap;

/// One event's wall time in milliseconds, nearest-rank percentiles.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Latency {
    pub p50: u64,
    pub p95: u64,
    pub n: usize,
}

/// Per event (`edit`, `session-start`, …); events with no timed row are absent.
pub fn latency(parsed: &Parsed) -> BTreeMap<String, Latency> {
    let mut by: BTreeMap<String, Vec<u64>> = BTreeMap::new();
    for v in &parsed.kept {
        if let (Some(ms), Some(e)) = (v["ms"].as_u64(), v["event"].as_str()) {
            by.entry(e.to_string()).or_default().push(ms);
        }
    }
    by.into_iter()
        .map(|(e, mut xs)| {
            xs.sort_unstable();
            let at = |pct: usize| xs[(xs.len() * pct).div_ceil(100).max(1) - 1];
            let l = Latency {
                p50: at(50),
                p95: at(95),
                n: xs.len(),
            };
            (e, l)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::latency;
    use std::path::Path;

    #[test]
    fn percentiles_are_nearest_rank_per_event() {
        let rows: String = (1..=20)
            .map(|i| format!("{{\"ts\":\"2026-11-01T00:00:00Z\",\"repo\":\"/w\",\"client\":\"c\",\"event\":\"edit\",\"bytes\":1,\"ms\":{i}}}\n"))
            .chain([
                "{\"ts\":\"2026-11-01T00:00:00Z\",\"repo\":\"/w\",\"client\":\"c\",\"event\":\"read\",\"bytes\":1}\n".into(),
            ])
            .collect();
        let p = crate::stats::parse(&rows, Path::new("/s"), &[]);
        let l = latency(&p);
        assert_eq!((l["edit"].p50, l["edit"].p95, l["edit"].n), (10, 19, 20));
        assert!(!l.contains_key("read"));
    }
}
