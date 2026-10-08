//! The promote ask (PLAN-fael-context-loop chunk 4): an open decision that
//! was in front of agents at edits of its file in `MIN_SESSIONS` sessions is
//! asked once, at the next edit of that file, whether it should be a test or a
//! lint/check the agent writes, then closed as moved. fael only asks; it never
//! writes or runs the check.
//!
//! Counting reads `usage.jsonl`, too slow for the push path, so session start
//! counts (decision `plan:fael-context-loop:promote-source`) and leaves the ids
//! in a cache the edit push reads. The count lags by at most one session.

use super::changed::Ask;
use super::say::{Kind, Line};
use crate::{core, journal};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Frozen from the in-context edit events before this code was written
/// (decision `plan:fael-context-loop:promote-n`): the top decile of open
/// decisions. Cut, never retuned, if the kind misses the 20% yield bar.
pub(crate) const MIN_SESSIONS: usize = 10;

/// `<repo scope>/cache/promote.txt`, one id per line: the clone-shared
/// journal, else the folder's own `.fael/` (as `stage` keeps its state).
fn path(root: &Path) -> PathBuf {
    journal::root(root)
        .unwrap_or_else(|| root.join(".fael"))
        .join("cache")
        .join("promote.txt")
}

/// Session start: rewrite the cache from this and last month's usage.
/// Fail-open: a write error only loses the ask.
pub(crate) fn refresh(root: &Path, open: &[&core::Row]) {
    let ids = due(&super::usage_files::read(None), open);
    let file = path(root);
    if let Some(dir) = file.parent()
        && std::fs::create_dir_all(dir).is_ok()
    {
        let _ = std::fs::write(&file, ids.join("\n"));
    }
}

/// The open decisions (not the user's call) in context at an edit in
/// `MIN_SESSIONS` distinct sessions, minus those a promote line already named:
/// once ever, not once per session. Ids are global, so no repo filter.
fn due(usage: &str, open: &[&core::Row]) -> Vec<String> {
    let mut sessions: HashMap<String, HashSet<String>> = HashMap::new();
    let mut asked: HashSet<String> = HashSet::new();
    let strs = |v: &Value, k: &str| -> Vec<String> {
        v[k].as_array()
            .into_iter()
            .flatten()
            .filter_map(|s| s.as_str().map(String::from))
            .collect()
    };
    // cheap filter before parsing: most lines are neither
    for l in usage
        .lines()
        .filter(|l| l.contains("\"in-context\"") || l.contains("\"promote\""))
    {
        let Ok(v) = serde_json::from_str::<Value>(l) else {
            continue;
        };
        if v["event"] == "in-context" {
            let s = v["session"].as_str().unwrap_or_default().to_string();
            for id in strs(&v, "in_context") {
                sessions.entry(id).or_default().insert(s.clone());
            }
        }
        let said = v["said"].as_array().into_iter().flatten();
        asked.extend(
            said.filter(|e| e["kind"] == "promote")
                .filter_map(|e| e["key"].as_str().map(String::from)),
        );
    }
    open.iter()
        .filter(|r| r.kind == "decision" && !r.from_user() && !asked.contains(&r.id))
        .filter(|r| sessions.get(&r.id).is_some_and(|s| s.len() >= MIN_SESSIONS))
        .map(|r| r.id.clone())
        .collect()
}

/// The first cached decision on an edited file among the edit's tier-0 rows,
/// as one `Promote` line. A read has no tier-0 rows, so it never opens the cache.
pub(crate) fn promote_line(ask: &Ask, t0: &[(&core::Row, usize)]) -> Option<Line> {
    let rows: Vec<&core::Row> = t0
        .iter()
        .filter(|(r, tier)| *tier == 0 && r.kind == "decision")
        .map(|(r, _)| *r)
        .collect();
    if rows.is_empty() {
        return None;
    }
    let cached = std::fs::read_to_string(path(ask.root)).ok()?;
    let r = rows
        .into_iter()
        .find(|r| cached.lines().any(|l| l == r.id))?;
    let file = ask.files.iter().find(|f| r.files.contains(f))?;
    let id = core::abbrev(ask.log).short(&r.id).to_string();
    Some(Line {
        kind: Kind::Promote { id: r.id.clone() },
        text: format!(
            "fael: {id} on {file} was in front of agents at edits in {MIN_SESSIONS}+ sessions — should it be a test or a lint/check you write, failing when broken? then `fael close {id} \"moved to <test or check>\"` · if neither fits, it stays as the why\n"
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: &str, kind: &str, from: Option<&str>) -> core::Row {
        let mut r: core::Row = serde_json::from_value(serde_json::json!({
            "v": 1, "id": id, "ts": "2026-10-08T00:00:00Z", "by": "t",
            "kind": kind, "text": "x", "files": ["a.rs"],
        }))
        .unwrap();
        r.from = from.map(String::from);
        r
    }

    fn in_ctx(session: usize, id: &str) -> String {
        format!(r#"{{"event":"in-context","session":"s{session}","in_context":["{id}"]}}"#)
    }

    /// Counts distinct sessions, keeps open non-user decisions only, and
    /// drops a row a promote line already named.
    #[test]
    fn due_counts_sessions_and_asks_once_ever() {
        let mut lines: Vec<String> = (0..MIN_SESSIONS)
            .flat_map(|s| ["01D", "01U", "01I", "01ASKED"].map(|id| in_ctx(s, id)))
            .collect();
        // the same session twice counts once
        lines.extend((0..MIN_SESSIONS - 1).map(|s| in_ctx(s, "01SHORT")));
        lines.push(in_ctx(0, "01SHORT"));
        lines.push(r#"{"event":"edit","said":[{"kind":"promote","key":"01ASKED"}]}"#.into());
        let rows = [
            row("01D", "decision", None),
            row("01U", "decision", Some("user")),
            row("01I", "issue", None),
            row("01ASKED", "decision", None),
            row("01SHORT", "decision", None),
        ];
        let open: Vec<&core::Row> = rows.iter().collect();
        assert_eq!(due(&lines.join("\n"), &open), vec!["01D".to_string()]);
    }
}
