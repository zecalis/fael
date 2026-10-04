//! Open rows that keep being pushed and never earn it (PLAN-fael-say-gate
//! chunk 5): a decision or issue handed over `UNUSED_PUSHES`+ times, still
//! open, never in context when the agent edited its file. A report for a
//! human to close, bump or supersede — it changes nothing a push says, and a
//! row it lists was not "useless", only never seen to be used. Pure.

use super::parse::Parsed;
use super::value::{in_window, window_start};
use crate::{Log, closed, superseded};
use serde::Serialize;
use std::collections::{HashMap, HashSet};

/// Pushes (counted only where an `in-context` line could have been written)
/// before an unused row is worth a human's look.
pub const UNUSED_PUSHES: usize = 20;
/// Listed at most this many, most pushed first.
const TOP: usize = 20;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct UnusedRow {
    pub id: String,
    pub kind: String,
    pub pushes: usize,
}

pub(super) fn unused(parsed: &Parsed, logs: &HashMap<String, Log>) -> Vec<UnusedRow> {
    let first = window_start(parsed);
    let mut pushes: HashMap<(&str, &str), usize> = HashMap::new();
    let mut used: HashSet<(&str, &str)> = HashSet::new();
    for v in &parsed.kept {
        let Some(repo) = v["repo"].as_str() else {
            continue;
        };
        let ids = |k: &str| {
            v[k].as_array()
                .into_iter()
                .flatten()
                .filter_map(|i| i.as_str())
        };
        if v["event"] == "in-context" {
            used.extend(ids("in_context").map(|id| (repo, id)));
        } else if in_window(&first, v) {
            for id in ids("ids") {
                *pushes.entry((repo, id)).or_default() += 1;
            }
        }
    }
    let mut out: Vec<UnusedRow> = pushes
        .into_iter()
        .filter(|(k, n)| *n >= UNUSED_PUSHES && !used.contains(k))
        .filter_map(|((repo, id), n)| {
            let log = logs.get(repo)?;
            // only a decision or issue can score: `in-context` never names a note
            let row = log
                .rows
                .iter()
                .find(|r| r.id == id && matches!(r.kind.as_str(), "decision" | "issue"))?;
            let gone = closed(log).contains(id) || superseded(log).contains(id);
            (!gone).then(|| UnusedRow {
                id: id.to_string(),
                kind: row.kind.clone(),
                pushes: n,
            })
        })
        .collect();
    out.sort_by(|a, b| b.pushes.cmp(&a.pushes).then_with(|| a.id.cmp(&b.id)));
    out.truncate(TOP);
    out
}

#[cfg(test)]
mod tests {
    use crate::Log;
    use std::collections::HashMap;

    fn row(id: &str, kind: &str) -> String {
        format!(
            "{{\"v\":1,\"id\":\"{id}\",\"ts\":\"2026-09-20T00:00:00Z\",\"by\":\"w\",\"kind\":\"{kind}\",\"text\":\"t\",\"files\":[\"a.rs\"]}}\n"
        )
    }

    /// `ids` is a push's rows, `in_context` the rows an edit found in context.
    fn line(event: &str, ids: &str, in_context: &str) -> String {
        format!(
            "{{\"ts\":\"2026-09-26T00:00:00.000Z\",\"repo\":\"/work/r\",\"client\":\"claude\",\"event\":\"{event}\",\"bytes\":1,\"est_tokens\":1,\"ids\":[{ids}],\"in_context\":[{in_context}]}}\n"
        )
    }

    #[test]
    fn lists_only_open_measurable_rows_never_in_context() {
        let (mut rows, mut w) = (vec![], vec![]);
        let text = row("U", "decision") + &row("H", "decision") + &row("N", "note");
        crate::log::parse(text.as_bytes(), "t.jsonl", &mut rows, &mut w);
        let log = Log {
            rows,
            ..Default::default()
        };
        // a client's window opens with its first in-context line (H's hit)
        let mut usage = line("in-context", "", "\"H\"");
        for id in ["U", "H", "N"] {
            for _ in 0..20 {
                usage += &line("read", &format!("\"{id}\""), "");
            }
        }
        // 19 pushes is under the bar
        for _ in 0..19 {
            usage += &line("read", "\"L\"", "");
        }
        let parsed = super::super::parse::parse(&usage, std::path::Path::new("/s/u.jsonl"), &[]);
        let logs = HashMap::from([("/work/r".to_string(), log)]);
        let got = super::unused(&parsed, &logs);
        // U: pushed, never hit · H: hit · N: a note, not measured · L: too few
        assert_eq!(got.len(), 1, "{got:?}");
        assert_eq!((got[0].id.as_str(), got[0].pushes), ("U", 20));
    }
}
