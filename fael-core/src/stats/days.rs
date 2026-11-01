//! Usage per calendar day (UTC) in one pass over the parsed rows: the series
//! a chart reads, so no reader loops `--since` (issue 01M5S63N).

use super::span::day_string;
use super::parse::Parsed;
use serde::Serialize;
use std::collections::BTreeMap;

/// One UTC day (`YYYY-MM-DD`): push events and the tokens fael put in front
/// of agents. New measures join this record as fields — readers skip unknown keys.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DayCount {
    pub day: String,
    pub events: usize,
    pub est_tokens: usize,
}

/// Days from the first to the last usage row of the window; days between
/// print zero, never missing. No usage = `[]`.
pub fn days(parsed: &Parsed) -> Vec<DayCount> {
    let mut per_day: BTreeMap<i64, (usize, usize)> = BTreeMap::new();
    for r in &parsed.rows {
        let e = per_day.entry(r.ms.div_euclid(86_400_000)).or_default();
        e.0 += 1;
        e.1 += r.toks;
    }
    let (Some(&first), Some(&last)) = (per_day.keys().next(), per_day.keys().next_back()) else {
        return vec![];
    };
    (first..=last)
        .map(|d| {
            let (events, est_tokens) = per_day.get(&d).copied().unwrap_or_default();
            DayCount {
                day: day_string(d),
                events,
                est_tokens,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    fn row(ts: &str, toks: usize) -> String {
        format!(
            "{{\"ts\":\"{ts}\",\"repo\":\"/w/r\",\"client\":\"claude\",\"event\":\"read\",\"bytes\":1,\"est_tokens\":{toks},\"ids\":[]}}\n"
        )
    }

    #[test]
    fn days_fill_zeros_and_sum_per_day() {
        let text = row("2026-10-01T23:59:00.000Z", 3)
            + &row("2026-10-01T01:00:00.000Z", 4)
            + &row("2026-10-03T00:00:00.000Z", 5);
        let p = super::super::parse::parse(
            &text,
            Path::new("/w/state/usage.jsonl"),
            &[PathBuf::from("/tmp")],
        );
        let d = days(&p);
        let got: Vec<_> = d
            .iter()
            .map(|x| (x.day.as_str(), x.events, x.est_tokens))
            .collect();
        assert_eq!(
            got,
            [
                ("2026-10-01", 2, 7),
                ("2026-10-02", 0, 0),
                ("2026-10-03", 1, 5)
            ]
        );
    }

    #[test]
    fn no_usage_is_empty() {
        let p = super::super::parse::parse("", Path::new("/w/s/usage.jsonl"), &[]);
        assert!(days(&p).is_empty());
    }
}
