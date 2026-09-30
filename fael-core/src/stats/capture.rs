//! Capture metrics (PLAN-fael-dev-adoption chunk 1): how much memory arrives
//! through the reply's `fael <kind>:` lines against the manual `add`, what
//! the Stop hook still costs in rounds, and how many sessions edited files
//! without leaving a row. Pure: kept usage rows and loaded logs in, numbers
//! out. Estimates stay estimates — a session's "no row" is a signal for a
//! human to look at, not a verdict.

use super::parse::Parsed;
use crate::{Log, ts_ms};
use serde::Serialize;
use std::collections::{HashMap, HashSet};

/// A row filed this long after a session's last event still belongs to it —
/// the closing `add` follows the final edit by a moment, not by a usage event.
const SLACK_MS: i64 = 10 * 60 * 1000;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Capture {
    /// Stop-hook blocks — each one cost the agent a round after it thought it
    /// was done. Cumulative, like `stop_blocks`; 0 by construction unless the
    /// repo opted into `[capture] block = true`.
    pub post_stop_rounds: usize,
    /// Capture lines the Stop hook saw in replies (stored + rejected).
    pub reply_lines: usize,
    pub reply_stored: usize,
    pub reply_rejected: usize,
    /// Rows added since the repo's first usage that did not arrive through a
    /// reply line — by any writer (a teammate's synced row counts too).
    pub manual_adds: usize,
    /// Sessions (per repo) that edited at least one file.
    pub sessions_with_edits: usize,
    /// …of which no row was filed during the session (+10 min).
    pub sessions_with_edits_no_row: usize,
}

pub(super) fn capture(parsed: &Parsed, logs: &HashMap<String, Log>) -> Capture {
    let (mut stored, mut rejected) = (0usize, 0usize);
    let mut stored_ids: HashSet<&str> = HashSet::new();
    // (repo, session) → (first event, last event, edited?)
    let mut sessions: HashMap<(&str, &str), (i64, i64, bool)> = HashMap::new();
    for v in &parsed.kept {
        if v["event"] == "capture" {
            match v["capture"].as_str() {
                Some("stored") => {
                    stored += 1;
                    stored_ids.extend(v["row"].as_str());
                }
                Some("rejected") => rejected += 1,
                _ => {}
            }
        }
        let (Some(repo), Some(session), Some(ms)) = (
            v["repo"].as_str(),
            v["session"].as_str().filter(|s| !s.is_empty()),
            v["ts"].as_str().and_then(ts_ms),
        ) else {
            continue;
        };
        let s = sessions.entry((repo, session)).or_insert((ms, ms, false));
        s.0 = s.0.min(ms);
        s.1 = s.1.max(ms);
        s.2 |= v["event"] == "edit";
    }
    let mut seen: HashSet<&str> = HashSet::new();
    let mut manual = 0usize;
    for (repo, first) in &parsed.first_seen {
        for r in logs.get(repo).into_iter().flat_map(|l| &l.rows) {
            if !r.kind.is_empty()
                && ts_ms(&r.ts).is_some_and(|t| t >= *first)
                && !stored_ids.contains(r.id.as_str())
                && seen.insert(r.id.as_str())
            {
                manual += 1;
            }
        }
    }
    let edited: Vec<_> = sessions.iter().filter(|(_, s)| s.2).collect();
    let no_row = edited
        .iter()
        .filter(|((repo, _), (first, last, _))| {
            !logs.get(*repo).is_some_and(|l| {
                l.rows.iter().any(|r| {
                    !r.kind.is_empty()
                        && ts_ms(&r.ts).is_some_and(|t| t >= *first && t <= last + SLACK_MS)
                })
            })
        })
        .count();
    Capture {
        post_stop_rounds: parsed.blocks.len(),
        reply_lines: stored + rejected,
        reply_stored: stored,
        reply_rejected: rejected,
        manual_adds: manual,
        sessions_with_edits: edited.len(),
        sessions_with_edits_no_row: no_row,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Row;
    use std::path::{Path, PathBuf};

    fn parsed(rows: &[&str]) -> Parsed {
        let tmp: Vec<PathBuf> = vec![PathBuf::from("/tmp")];
        super::super::parse::parse(&rows.join("\n"), Path::new("/w/usage.jsonl"), &tmp)
    }

    fn row(id: &str, ts: &str, kind: &str) -> Row {
        Row {
            id: id.into(),
            ts: ts.into(),
            kind: kind.into(),
            ..Row::default()
        }
    }

    #[test]
    fn counts_reply_manual_and_silent_sessions() {
        let p = parsed(&[
            r#"{"ts":"2026-09-30T00:00:00.000Z","repo":"/w/r","client":"claude","event":"edit","session":"s1","ids":[]}"#,
            r#"{"ts":"2026-09-30T00:01:00.000Z","repo":"/w/r","client":"claude","event":"capture","capture":"stored","row":"R1","session":"s1"}"#,
            r#"{"ts":"2026-09-30T00:01:00.000Z","repo":"/w/r","client":"claude","event":"capture","capture":"rejected","session":"s1"}"#,
            r#"{"ts":"2026-09-30T01:00:00.000Z","repo":"/w/r","client":"claude","event":"edit","session":"s2","ids":[]}"#,
            r#"{"ts":"2026-09-30T02:00:00.000Z","repo":"/w/r","client":"claude","event":"read","session":"s3","ids":[]}"#,
            r#"{"ts":"2026-09-30T00:00:30.000Z","repo":"/w/r","client":"claude","event":"stop-work","ask":"stop-block","session":"s1"}"#,
        ]);
        let log = Log {
            rows: vec![
                row("R1", "2026-09-30T00:01:00.000Z", "note"), // via reply
                row("M1", "2026-09-30T00:00:20.000Z", "decision"), // manual
                row("C1", "2026-09-30T00:00:25.000Z", ""),     // a close: not an add
                row("OLD", "2026-09-29T00:00:00.000Z", "note"), // before first usage
            ],
            ..Log::default()
        };
        let c = capture(&p, &HashMap::from([("/w/r".to_string(), log)]));
        assert_eq!(
            c,
            Capture {
                post_stop_rounds: 1,
                reply_lines: 2,
                reply_stored: 1,
                reply_rejected: 1,
                manual_adds: 1,
                sessions_with_edits: 2,
                sessions_with_edits_no_row: 1, // s2: edited at 01:00, no row near it
            }
        );
    }
}
