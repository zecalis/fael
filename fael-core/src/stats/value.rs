//! The value line (PLAN-fael-visible-secretary chunk 5): what fael gave back
//! in the window, ahead of what it cost. Deterministic events only — never
//! "saved" or "prevented": fael knows what it handed over, not what the agent
//! would have done without it. Pure: kept usage rows and loaded logs in.

use super::capture::Capture;
use super::parse::Parsed;
use super::retire::Retired;
use crate::{Log, ts_ms};
use serde::Serialize;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Value {
    /// Distinct (session, row) pairs: a decision or issue about a file was
    /// already in the agent's context when it edited that file (`reminded`
    /// usage lines).
    pub reminded_before_edit: usize,
    /// Issues closed since the repo's first usage.
    pub issues_closed: usize,
    /// Distinct `*:handoff` rows a push handed to an agent.
    pub handoffs_picked_up: usize,
    /// Same number as `retired.at_touch`.
    pub retired_at_touch: usize,
    /// Same number as `capture.reply_stored`.
    pub filed_from_replies: usize,
}

pub(super) fn value(
    parsed: &Parsed,
    logs: &HashMap<String, Log>,
    retired: &Retired,
    capture: &Capture,
) -> Value {
    let mut reminded: HashSet<(&str, &str, &str)> = HashSet::new();
    for v in parsed.kept.iter().filter(|v| v["event"] == "reminded") {
        let (Some(repo), Some(session)) = (v["repo"].as_str(), v["session"].as_str()) else {
            continue;
        };
        for id in v["reminded"].as_array().into_iter().flatten() {
            reminded.extend(id.as_str().map(|id| (repo, session, id)));
        }
    }
    // repos in one clone share the journal: dedup closes and rows by id
    let mut closed: HashSet<&str> = HashSet::new();
    for (repo, first) in &parsed.first_seen {
        let Some(log) = logs.get(repo) else { continue };
        let issues: HashSet<&str> = log
            .rows
            .iter()
            .filter(|r| r.kind == "issue")
            .map(|r| r.id.as_str())
            .collect();
        closed.extend(log.closes.iter().filter_map(|c| {
            let id = c.reference.as_deref()?;
            (issues.contains(id) && ts_ms(&c.ts).is_some_and(|t| t >= *first)).then_some(id)
        }));
    }
    let handoffs = parsed
        .id_repos
        .iter()
        .filter(|(id, repos)| {
            repos.iter().any(|repo| {
                logs.get(repo).is_some_and(|log| {
                    log.rows.iter().any(|r| {
                        &r.id == *id && r.key.as_deref().is_some_and(|k| k.ends_with(":handoff"))
                    })
                })
            })
        })
        .count();
    Value {
        reminded_before_edit: reminded.len(),
        issues_closed: closed.len(),
        handoffs_picked_up: handoffs,
        retired_at_touch: retired.at_touch,
        filed_from_replies: capture.reply_stored,
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

    fn row(id: &str, ts: &str, kind: &str, extra: &str) -> String {
        format!(
            "{{\"v\":1,\"id\":\"{id}\",\"ts\":\"{ts}\",\"by\":\"w\",\"kind\":\"{kind}\",\"text\":\"t\",\"files\":[\"a.rs\"]{extra}}}\n"
        )
    }

    #[test]
    fn counts_reminders_closes_and_handoffs() {
        let log = Log {
            rows: rows(
                &(row("I1", "2026-09-20T00:00:00Z", "issue", "")
                    + &row("I2", "2026-09-20T00:00:00Z", "issue", "")
                    + &row("D1", "2026-09-20T00:00:00Z", "decision", "")
                    + &row(
                        "H1",
                        "2026-09-25T00:00:00Z",
                        "note",
                        ",\"key\":\"plan:x:handoff\"",
                    )),
            ),
            // I1 closed in the window, I2 before it, D1 is no issue
            closes: rows(
                &(row("C1", "2026-09-26T05:00:00Z", "close", ",\"ref\":\"I1\"")
                    + &row("C2", "2026-09-21T00:00:00Z", "close", ",\"ref\":\"I2\"")
                    + &row("C3", "2026-09-26T05:00:00Z", "close", ",\"ref\":\"D1\"")),
            ),
            ..Default::default()
        };
        // s1 reminded of D1 twice (counts once) and I1; s2 of D1 again;
        // a session-less line never joins; H1 pushed at session start
        let usage = r#"{"ts":"2026-09-26T00:00:00.000Z","repo":"/work/r","client":"claude","event":"session-start","bytes":9,"est_tokens":2,"ids":["H1"],"session":"s1"}
{"ts":"2026-09-26T00:01:00.000Z","repo":"/work/r","client":"claude","event":"reminded","bytes":0,"est_tokens":0,"ids":[],"reminded":["D1","I1"],"session":"s1"}
{"ts":"2026-09-26T00:02:00.000Z","repo":"/work/r","client":"claude","event":"reminded","bytes":0,"est_tokens":0,"ids":[],"reminded":["D1"],"session":"s1"}
{"ts":"2026-09-26T00:03:00.000Z","repo":"/work/r","client":"claude","event":"reminded","bytes":0,"est_tokens":0,"ids":[],"reminded":["D1"],"session":"s2"}
{"ts":"2026-09-26T00:04:00.000Z","repo":"/work/r","client":"codex","event":"reminded","bytes":0,"est_tokens":0,"ids":[],"reminded":["D1"]}
"#;
        let parsed = super::super::parse::parse(usage, std::path::Path::new("/s/usage.jsonl"), &[]);
        let logs = HashMap::from([("/work/r".to_string(), log)]);
        let retired = super::super::retire::retired(&parsed, &logs);
        let capture = super::super::capture::capture(&parsed, &logs);
        let v = super::value(&parsed, &logs, &retired, &capture);
        assert_eq!(
            (
                v.reminded_before_edit,
                v.issues_closed,
                v.handoffs_picked_up
            ),
            (3, 1, 1)
        );
    }
}
