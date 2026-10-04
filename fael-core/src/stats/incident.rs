//! Incidents a human filed (01M40ARQ step 3): rows keyed
//! `incident:<kind>[:<slug>]` — duplicate work, a contradicted decision —
//! counted per week. The one outcome fael cannot see in usage, so it is never
//! inferred: no row, no incident. Pure: parsed usage (each repo's first use)
//! and loaded logs in.

use super::parse::Parsed;
use crate::{Log, rfc3339, superseded, ts_ms};
use std::collections::{BTreeMap, HashMap, HashSet};

/// First key segment that marks a row as an incident.
pub const INCIDENT_KEY: &str = "incident";

/// Week (its Monday, `YYYY-MM-DD`, UTC) → kind → incidents filed that week.
pub type Incidents = BTreeMap<String, BTreeMap<String, usize>>;

/// Rows keyed `incident:<kind>…` filed at or after their repo's first usage
/// (so `--since` cuts them too), deduped by id across repos — worktrees share
/// one journal. A superseded row is a rewrite of the same incident and is left
/// out; a closed one still happened and counts.
pub(super) fn incidents(parsed: &Parsed, logs: &HashMap<String, Log>) -> Incidents {
    let mut out = Incidents::new();
    let mut seen = HashSet::new();
    for (repo, first) in &parsed.first_seen {
        let Some(log) = logs.get(repo) else { continue };
        let gone = superseded(log);
        for r in &log.rows {
            let mut key = r.key.as_deref().unwrap_or("").split(':');
            let (Some(INCIDENT_KEY), Some(kind)) = (key.next(), key.next()) else {
                continue;
            };
            let Some(ms) = ts_ms(&r.ts).filter(|t| t >= first) else {
                continue;
            };
            if kind.is_empty() || gone.contains(r.id.as_str()) || !seen.insert(r.id.as_str()) {
                continue;
            }
            *out.entry(monday(ms))
                .or_default()
                .entry(kind.to_string())
                .or_default() += 1;
        }
    }
    out
}

/// The Monday (UTC) of the week `ms` falls in, `YYYY-MM-DD`.
fn monday(ms: i64) -> String {
    const DAY: i64 = 86_400_000;
    let days = ms.div_euclid(DAY);
    // 1970-01-01 was a Thursday: +3 makes Monday 0
    let start = (days - (days + 3).rem_euclid(7)) * DAY;
    rfc3339(start.max(0) as u64)[..10].to_string()
}

#[cfg(test)]
mod tests {
    use crate::Log;
    use std::collections::HashMap;

    fn row(id: &str, ts: &str, key: &str, extra: &str) -> String {
        format!(
            "{{\"v\":1,\"id\":\"{id}\",\"ts\":\"{ts}\",\"by\":\"w\",\"kind\":\"note\",\"text\":\"t\",\"files\":[\"a.rs\"],\"key\":\"{key}\"{extra}}}\n"
        )
    }

    #[test]
    fn counts_incident_rows_per_week_and_kind() {
        let jsonl = row("A", "2026-09-28T00:00:00Z", "incident:duplicate-work:a", "")
            // Sunday: still the week of Monday 09-28
            + &row("B", "2026-10-04T23:59:59Z", "incident:duplicate-work", "")
            + &row("C", "2026-10-05T00:00:00Z", "incident:contradicted-decision", "")
            // a rewrite of C: C drops out, D counts
            + &row("D", "2026-10-05T01:00:00Z", "incident:contradicted-decision", ",\"supersedes\":\"C\"")
            // before the repo's first usage, no kind, not an incident
            + &row("E", "2026-09-01T00:00:00Z", "incident:duplicate-work", "")
            + &row("F", "2026-10-01T00:00:00Z", "incident:", "")
            + &row("G", "2026-10-01T00:00:00Z", "incidents:duplicate-work", "");
        let (mut rows, mut w) = (vec![], vec![]);
        crate::log::parse(jsonl.as_bytes(), "t.jsonl", &mut rows, &mut w);
        let log = Log {
            rows,
            ..Default::default()
        };
        let usage = "{\"ts\":\"2026-09-20T00:00:00.000Z\",\"repo\":\"/w\",\"client\":\"claude\",\"event\":\"read\",\"bytes\":1,\"est_tokens\":1,\"ids\":[]}\n\
                     {\"ts\":\"2026-09-20T00:00:00.000Z\",\"repo\":\"/wt\",\"client\":\"claude\",\"event\":\"read\",\"bytes\":1,\"est_tokens\":1,\"ids\":[]}\n";
        let parsed = super::super::parse::parse(usage, std::path::Path::new("/s/usage.jsonl"), &[]);
        // a worktree reads the same journal: each row once
        let logs = HashMap::from([("/w".to_string(), log.clone()), ("/wt".to_string(), log)]);
        let got = super::incidents(&parsed, &logs);
        let week = |pairs: &[(&str, usize)]| {
            pairs
                .iter()
                .map(|(k, n)| (k.to_string(), *n))
                .collect::<std::collections::BTreeMap<_, _>>()
        };
        assert_eq!(got.len(), 2);
        assert_eq!(got["2026-09-28"], week(&[("duplicate-work", 2)]));
        assert_eq!(got["2026-10-05"], week(&[("contradicted-decision", 1)]));
    }
}
