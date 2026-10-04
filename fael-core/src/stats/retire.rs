//! Rows retired where they were pushed: of the distinct rows a read or edit
//! push handed an agent, how many were closed, superseded or bumped (a bump
//! event names the row) within `RETIRE_WINDOW_MS` after one of those pushes. The measure
//! of the edit-push ask — a row the code outgrew should go while the agent has
//! the code in front of it. Pure: usage rows in, logs in, counts out.

use super::parse::Parsed;
use crate::{Log, reverted, ts_ms};
use serde::Serialize;
use std::collections::{HashMap, HashSet};

/// ponytail: a fixed day stands in for "the same session" — usage lines of a
/// push carry no session on every client; a per-session join when they do.
pub const RETIRE_WINDOW_MS: i64 = 24 * 3600 * 1000;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Retired {
    /// Distinct rows handed over by a read or edit push.
    pub pushed: usize,
    /// Of those, closed or superseded within the window after a push.
    pub at_touch: usize,
}

/// When each row was handled, per repo: its close, the row that superseded
/// it (reverted edges skipped), or a bump event — a bump keeps the row open
/// but is the check the ask asked for, as the supersede it replaced was.
/// The first retirement wins.
pub(super) fn retire_times(log: &Log) -> HashMap<&str, i64> {
    let mut out: HashMap<&str, i64> = HashMap::new();
    let rev = reverted(log);
    let events = log
        .closes
        .iter()
        .filter_map(|c| Some((c.reference.as_deref()?, ts_ms(&c.ts)?)))
        .chain(log.rows.iter().filter_map(|r| {
            let t = r.supersedes.as_deref()?;
            (!rev.contains(r.id.as_str())).then_some(())?;
            Some((t, ts_ms(&r.ts)?))
        }))
        .chain(
            log.rows
                .iter()
                .filter_map(|r| Some((r.bumps.as_deref()?, ts_ms(&r.ts)?))),
        );
    for (id, ms) in events {
        out.entry(id)
            .and_modify(|m| *m = (*m).min(ms))
            .or_insert(ms);
    }
    out
}

pub(super) fn retired(parsed: &Parsed, logs: &HashMap<String, Log>) -> Retired {
    let times: HashMap<&str, HashMap<&str, i64>> = logs
        .iter()
        .map(|(repo, log)| (repo.as_str(), retire_times(log)))
        .collect();
    let (mut pushed, mut at_touch) = (HashSet::new(), HashSet::new());
    for v in &parsed.kept {
        if !matches!(v["event"].as_str(), Some("read" | "edit")) {
            continue;
        }
        let (Some(repo), Some(ms)) = (v["repo"].as_str(), v["ts"].as_str().and_then(ts_ms)) else {
            continue;
        };
        for id in v["ids"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|i| i.as_str())
        {
            pushed.insert(id);
            let gone = times.get(repo).and_then(|t| t.get(id));
            if gone.is_some_and(|g| *g >= ms && *g - ms <= RETIRE_WINDOW_MS) {
                at_touch.insert(id);
            }
        }
    }
    Retired {
        pushed: pushed.len(),
        at_touch: at_touch.len(),
    }
}

#[cfg(test)]
mod tests {
    use crate::Log;
    use std::collections::HashMap;

    fn rows(jsonl: &str) -> Vec<crate::Row> {
        let (mut out, mut w) = (vec![], vec![]);
        crate::log::parse(jsonl.as_bytes(), "t.jsonl", &mut out, &mut w);
        out
    }

    #[test]
    fn counts_rows_retired_within_a_day_of_a_push() {
        let row = |id: &str, extra: &str| {
            format!(
                "{{\"v\":1,\"id\":\"{id}\",\"ts\":\"2026-09-26T00:00:00Z\",\"by\":\"w\",\"kind\":\"decision\",\"text\":\"t\",\"files\":[\"a.rs\"]{extra}}}\n"
            )
        };
        let mut log = Log {
            // C supersedes B an hour after the push, a bump event moves E two
            // hours after it; A closes two days later
            rows: rows(
                &(row("A", "")
                    + &row("B", "")
                    + &row("D", "")
                    + &row("E", "")
                    + "{\"v\":1,\"id\":\"C\",\"ts\":\"2026-09-26T01:00:00Z\",\"by\":\"w\",\"kind\":\"decision\",\"text\":\"t\",\"files\":[\"a.rs\"],\"supersedes\":\"B\"}\n"
                    + "{\"v\":1,\"id\":\"F\",\"ts\":\"2026-09-26T02:00:00Z\",\"by\":\"w\",\"text\":\"E bumped\",\"bumps\":\"E\"}\n"),
            ),
            ..Default::default()
        };
        log.closes = rows(
            "{\"v\":1,\"id\":\"X\",\"ts\":\"2026-09-28T00:10:00Z\",\"by\":\"w\",\"kind\":\"close\",\"text\":\"done\",\"files\":[],\"ref\":\"A\"}\n",
        );
        let usage = r#"{"ts":"2026-09-26T00:05:00.000Z","repo":"/work/r","client":"claude","event":"edit","bytes":1,"est_tokens":1,"ids":["A","B","D","E"]}
{"ts":"2026-09-26T00:06:00.000Z","repo":"/work/r","client":"claude","event":"session-start","bytes":1,"est_tokens":1,"ids":["Z"]}
"#;
        let parsed = super::super::parse::parse(usage, std::path::Path::new("/s/usage.jsonl"), &[]);
        let logs = HashMap::from([("/work/r".to_string(), log)]);
        let r = super::retired(&parsed, &logs);
        // A, B, D, E pushed (session-start is no push); B and E within a day
        assert_eq!((r.pushed, r.at_touch), (4, 2));
    }
}
