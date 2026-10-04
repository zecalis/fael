//! The value line (PLAN-fael-visible-secretary chunk 5): what fael gave back
//! in the window, ahead of what it cost. Deterministic events only — never
//! "saved" or "prevented": fael knows what it handed over, not what the agent
//! would have done without it. Pure: kept usage rows and loaded logs in.

use super::parse::Parsed;
use crate::{Log, ts_ms};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap, HashSet};

/// `retired.at_touch` and `capture.reply_stored` join the line from their
/// own blocks — not copied here, so `--json` carries each number once.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Value {
    /// Distinct (session, row) pairs: a decision or issue an earlier push of
    /// the session handed over was still in context when the agent edited
    /// its file (`in-context` usage lines).
    pub in_context_at_edit: usize,
    /// Issues closed since the repo's first usage.
    pub issues_closed: usize,
    /// Distinct `*:handoff` rows a push handed to an agent.
    pub handoffs_picked_up: usize,
    /// Which push event earns its tokens: each (session, row) pair is
    /// attributed once, to the event of the push that first handed it over,
    /// counting only lines stamped at or after the client's first `in-context`
    /// line (see `EventValue`).
    pub by_event: BTreeMap<String, EventValue>,
    /// Rows one agent wrote and another was handed (`cross.rs`).
    pub cross_agent: super::cross::CrossAgent,
}

/// One push event's hit rate (`in_context_at_edit / pushed`), over the usage
/// lines at or after each client's first `in-context` line (earlier pushes
/// could never score, so they are left out of both numbers — the sums can be
/// lower than the plain totals). Read it with three limits: a lower bound (an `in-context` line exists only when the
/// agent edits the row's file, so a read-only session scores every row it was
/// handed as a miss); first push claims the pair, and a push never repeats a
/// row its session already holds, so a later event carries only what is new
/// and an `edit` push, made at the edit, can only hit on a later edit; and
/// a client that has never written an `in-context` line has no entry at all.
/// Cost per event is `Stats::by_event` — not repeated here.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct EventValue {
    /// Distinct (session, row) pairs this event handed over first.
    pub pushed: usize,
    /// Of those, rows still in context when the agent edited their file.
    pub in_context_at_edit: usize,
}

/// (session, row) pairs a push handed over, each with the event of the first
/// push that said it, and the pairs found in context at an edit.
type Pairs<'a> = (
    HashMap<(&'a str, &'a str, &'a str), &'a str>,
    HashSet<(&'a str, &'a str, &'a str)>,
);

/// Each client's first `in-context` line ts: before it no hook could write one.
pub(super) fn window_start(parsed: &Parsed) -> HashMap<&str, &str> {
    let mut first: HashMap<&str, &str> = HashMap::new();
    for v in parsed.kept.iter().filter(|v| v["event"] == "in-context") {
        if let (Some(c), Some(ts)) = (v["client"].as_str(), v["ts"].as_str()) {
            first
                .entry(c)
                .and_modify(|t| *t = (*t).min(ts))
                .or_insert(ts);
        }
    }
    first
}

/// A usage line stamped at or after its client's window start.
pub(super) fn in_window(first: &HashMap<&str, &str>, v: &serde_json::Value) -> bool {
    match (v["client"].as_str(), v["ts"].as_str()) {
        (Some(c), Some(ts)) => first.get(c).is_some_and(|f| ts >= *f),
        _ => false,
    }
}

/// Join `in-context` lines to the pushes before them. `windowed` keeps only
/// usage lines stamped at or after the first `in-context` line of their
/// client: before that no hook could have written one, so a push there can
/// never score — counting it would bury the hit rate under sessions the
/// metric did not exist for.
// ponytail: window start = the client's first in-context line, a little after
// the release that added them; a rate read this way is slightly low, never high
fn pairs(parsed: &Parsed, windowed: bool) -> Pairs<'_> {
    // the seen list behind an `in-context` line also holds rows the agent
    // filed or found itself: only a row a push said first counts.
    // ponytail: usage.jsonl is append-only, so file order is time order.
    let first = window_start(parsed);
    let capable = |v: &serde_json::Value| !windowed || in_window(&first, v);
    // the value is the event of the first push that said the row
    let mut pushed: HashMap<(&str, &str, &str), &str> = HashMap::new();
    let mut in_context: HashSet<(&str, &str, &str)> = HashSet::new();
    for v in parsed.kept.iter().filter(|v| capable(v)) {
        let (Some(repo), Some(session)) = (v["repo"].as_str(), v["session"].as_str()) else {
            continue;
        };
        let ids = |k: &str| {
            v[k].as_array()
                .into_iter()
                .flatten()
                .filter_map(|i| i.as_str())
        };
        if v["event"] == "in-context" {
            in_context.extend(
                ids("in_context")
                    .map(|id| (repo, session, id))
                    .filter(|k| pushed.contains_key(k)),
            );
        } else {
            let event = v["event"].as_str().unwrap_or("unknown");
            for id in ids("ids") {
                pushed.entry((repo, session, id)).or_insert(event);
            }
        }
    }
    (pushed, in_context)
}

pub(super) fn value(parsed: &Parsed, logs: &HashMap<String, Log>) -> Value {
    let (_, in_context) = pairs(parsed, false);
    let (pushed, in_window) = pairs(parsed, true);
    let cross_agent = super::cross::cross_agent(parsed, logs, &pushed, &in_window);
    let mut by_event: BTreeMap<String, EventValue> = BTreeMap::new();
    for event in pushed.values() {
        by_event.entry(event.to_string()).or_default().pushed += 1;
    }
    for k in &in_window {
        by_event
            .entry(pushed[k].to_string())
            .or_default()
            .in_context_at_edit += 1;
    }
    // repos in one clone share the journal: dedup closes and rows by id
    let mut closed: HashSet<&str> = HashSet::new();
    for (repo, first) in &parsed.first_seen {
        let Some(log) = logs.get(repo) else { continue };
        let in_window = |ts: Option<&str>| ts.and_then(ts_ms).is_some_and(|t| t >= *first);
        let issues = log.rows.iter().filter(|r| r.kind == "issue");
        let ids: HashSet<&str> = issues.clone().map(|r| r.id.as_str()).collect();
        closed.extend(log.closes.iter().filter_map(|c| {
            let id = c.reference.as_deref()?;
            (ids.contains(id) && in_window(Some(&c.ts))).then_some(id)
        }));
        // `fael compact` folds a close into its row
        closed.extend(
            issues
                .filter(|r| in_window(r.extra.get("closed").and_then(|c| c["ts"].as_str())))
                .map(|r| r.id.as_str()),
        );
    }
    let handoff_ids: HashMap<&str, HashSet<&str>> = logs
        .iter()
        .map(|(repo, log)| {
            let h = log
                .rows
                .iter()
                .filter(|r| r.key.as_deref().is_some_and(|k| k.ends_with(":handoff")));
            (repo.as_str(), h.map(|r| r.id.as_str()).collect())
        })
        .collect();
    let handoffs = parsed
        .id_repos
        .iter()
        .filter(|(id, repos)| {
            repos.iter().any(|repo| {
                handoff_ids
                    .get(repo.as_str())
                    .is_some_and(|h| h.contains(id.as_str()))
            })
        })
        .count();
    Value {
        in_context_at_edit: in_context.len(),
        issues_closed: closed.len(),
        handoffs_picked_up: handoffs,
        by_event,
        cross_agent,
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
    fn counts_in_context_closes_and_handoffs() {
        let log = Log {
            rows: rows(
                &(row("I1", "2026-09-20T00:00:00Z", "issue", "")
                    + &row("I2", "2026-09-20T00:00:00Z", "issue", "")
                    // closed in the window, then folded into the row by compact
                    + &row(
                        "I3",
                        "2026-09-20T00:00:00Z",
                        "issue",
                        ",\"closed\":{\"id\":\"C9\",\"ts\":\"2026-09-27T00:00:00Z\"}",
                    )
                    + &row("D1", "2026-09-20T00:00:00Z", "decision", "")
                    + &row("D2", "2026-09-26T00:00:00Z", "decision", "")
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
        // s1 was pushed H1, D1, I1: in context at edit D1 twice (counts once)
        // and I1; D2 the agent filed itself, never pushed — no count. s2 is
        // pushed D1 too. A session-less line never joins.
        // claude wrote its first in-context line on 09-25 (the feature exists
        // from then); codex only at 00:04, so its 00:00:10 read of D1 in s3
        // predates the feature and stays out of by_event
        let usage = r#"{"ts":"2026-09-25T00:00:00.000Z","repo":"/work/r","client":"claude","event":"in-context","bytes":0,"est_tokens":0,"ids":[],"in_context":["Z9"],"session":"s0"}
{"ts":"2026-09-26T00:00:10.000Z","repo":"/work/r","client":"codex","event":"read","bytes":9,"est_tokens":2,"ids":["D1"],"session":"s3"}
{"ts":"2026-09-26T00:00:00.000Z","repo":"/work/r","client":"claude","event":"session-start","bytes":9,"est_tokens":2,"ids":["H1","D1"],"session":"s1"}
{"ts":"2026-09-26T00:00:30.000Z","repo":"/work/r","client":"claude","event":"read","bytes":9,"est_tokens":2,"ids":["I1"],"session":"s1"}
{"ts":"2026-09-26T00:01:00.000Z","repo":"/work/r","client":"claude","event":"in-context","bytes":0,"est_tokens":0,"ids":[],"in_context":["D1","I1","D2"],"session":"s1"}
{"ts":"2026-09-26T00:02:00.000Z","repo":"/work/r","client":"claude","event":"in-context","bytes":0,"est_tokens":0,"ids":[],"in_context":["D1"],"session":"s1"}
{"ts":"2026-09-26T00:02:30.000Z","repo":"/work/r","client":"claude","event":"read","bytes":9,"est_tokens":2,"ids":["D1"],"session":"s2"}
{"ts":"2026-09-26T00:03:00.000Z","repo":"/work/r","client":"claude","event":"in-context","bytes":0,"est_tokens":0,"ids":[],"in_context":["D1"],"session":"s2"}
{"ts":"2026-09-26T00:04:00.000Z","repo":"/work/r","client":"codex","event":"in-context","bytes":0,"est_tokens":0,"ids":[],"in_context":["D1"]}
"#;
        let parsed = super::super::parse::parse(usage, std::path::Path::new("/s/usage.jsonl"), &[]);
        // in-context lines are no injection
        assert_eq!(parsed.n, 4);
        let logs = HashMap::from([("/work/r".to_string(), log)]);
        let v = super::value(&parsed, &logs);
        assert_eq!(
            (v.in_context_at_edit, v.issues_closed, v.handoffs_picked_up),
            (3, 2, 1)
        );
        // s1: H1 and D1 came from session-start, I1 from read; s2: D1 from
        // read. Hits: (s1,D1) session-start, (s1,I1) and (s2,D1) read.
        let ev = |e: &str| (v.by_event[e].pushed, v.by_event[e].in_context_at_edit);
        assert_eq!((ev("session-start"), ev("read")), ((2, 1), (2, 2)));
        // each pair counts once, and the codex pair before its window is out
        assert!(!v.by_event.contains_key("codex"));
        assert_eq!(v.by_event.values().map(|e| e.pushed).sum::<usize>(), 4);
        assert_eq!(
            v.by_event
                .values()
                .map(|e| e.in_context_at_edit)
                .sum::<usize>(),
            v.in_context_at_edit
        );
    }
}
