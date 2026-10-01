//! Capture metrics (PLAN-fael-dev-adoption chunk 1): how much memory arrives
//! through the reply's `fael <kind>:` lines against the manual `add`, what
//! the Stop hook still costs in rounds, and how many sessions edited files
//! without leaving a row. Pure: kept usage rows and loaded logs in, numbers
//! out. Estimates stay estimates — a session's "no row" is a signal for a
//! human to look at, not a verdict.

use super::parse::Parsed;
use crate::{Log, Row, rfc3339, ts_ms};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// A row filed this long after a session's last event still belongs to it —
/// the closing `add` follows the final edit by a moment, not by a usage event.
const SLACK_MS: i64 = 10 * 60 * 1000;

/// How many silent sessions `no_row_sessions` lists — the checkpoint's ten
/// for a human to judge, newest first.
const SAMPLE: usize = 10;

/// One session that edited files and filed no row — where to look.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Silent {
    pub repo: String,
    pub session: String,
    pub from: String,
    pub to: String,
}

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
    /// …of which no row was filed during the session (+10 min). Worktrees
    /// share a journal, so a row counts only when it is this session's: its
    /// writer session when the row has one, else its branch against the
    /// session's edits (either side unknown = it counts, as before).
    pub sessions_with_edits_no_row: usize,
    /// The newest ≤10 of those, for a human to judge.
    pub no_row_sessions: Vec<Silent>,
}

pub(super) fn capture(parsed: &Parsed, logs: &HashMap<String, Log>) -> Capture {
    let (mut stored, mut rejected) = (0usize, 0usize);
    let mut stored_ids: HashSet<&str> = HashSet::new();
    // (repo, session) → (first event, last event, edited?, edit branch)
    type Span<'a> = (i64, i64, bool, Option<&'a str>);
    let mut sessions: HashMap<(&str, &str), Span> = HashMap::new();
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
        let s = sessions
            .entry((repo, session))
            .or_insert((ms, ms, false, None));
        s.0 = s.0.min(ms);
        s.1 = s.1.max(ms);
        if v["event"] == "edit" {
            s.2 = true;
            s.3 = v["branch"].as_str().or(s.3);
        }
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
    let mut no_row: Vec<_> = edited
        .iter()
        .filter(|((repo, session), (first, last, _, branch))| {
            !logs.get(*repo).is_some_and(|l| {
                l.rows.iter().any(|r| {
                    !r.kind.is_empty()
                        && ts_ms(&r.ts).is_some_and(|t| t >= *first && t <= last + SLACK_MS)
                        && mine(r, session, *branch)
                })
            })
        })
        .collect();
    no_row.sort_by_key(|(k, s)| (std::cmp::Reverse(s.1), *k));
    let at = |ms: i64| rfc3339(ms.max(0) as u64);
    Capture {
        post_stop_rounds: parsed.blocks.len(),
        reply_lines: stored + rejected,
        reply_stored: stored,
        reply_rejected: rejected,
        manual_adds: manual,
        sessions_with_edits: edited.len(),
        sessions_with_edits_no_row: no_row.len(),
        no_row_sessions: no_row
            .iter()
            .take(SAMPLE)
            .map(|((repo, session), s)| Silent {
                repo: repo.to_string(),
                session: session.to_string(),
                from: at(s.0),
                to: at(s.1),
            })
            .collect(),
    }
}

/// Whether row `r` came from this session: rows carry the writer's session
/// as the transcript's file stem (`Row::session`), usage carries the hook's
/// key (a path for Claude), so compare stems. No writer session → branch.
fn mine(r: &Row, session: &str, branch: Option<&str>) -> bool {
    match r.session() {
        Some(w) => Path::new(session).file_stem().is_some_and(|s| *s == *w),
        None => branch.is_none() || r.branch().is_none() || r.branch() == branch,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

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
            // two worktrees, one journal: s4's window holds only s5's row
            r#"{"ts":"2026-09-30T03:00:00.000Z","repo":"/w/r","client":"claude","event":"edit","branch":"feat/a","session":"s4","ids":[]}"#,
            r#"{"ts":"2026-09-30T03:00:00.000Z","repo":"/w/r","client":"claude","event":"edit","branch":"feat/b","session":"s5","ids":[]}"#,
        ]);
        let mut on_b = row("B1", "2026-09-30T03:01:00.000Z", "note");
        on_b.extra.insert("branch".into(), "feat/b".into());
        // filed by another session on s4's own branch: still not s4's row
        let mut other = row("B2", "2026-09-30T03:02:00.000Z", "note");
        other.extra.insert("branch".into(), "feat/a".into());
        other.extra.insert("session".into(), "s9".into());
        let log = Log {
            rows: vec![
                row("R1", "2026-09-30T00:01:00.000Z", "note"), // via reply
                row("M1", "2026-09-30T00:00:20.000Z", "decision"), // manual
                row("C1", "2026-09-30T00:00:25.000Z", ""),     // a close: not an add
                row("OLD", "2026-09-29T00:00:00.000Z", "note"), // before first usage
                on_b,
                other,
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
                manual_adds: 3,
                sessions_with_edits: 4,
                sessions_with_edits_no_row: 2, // s2: no row near it · s4: only feat/b's
                no_row_sessions: ["s4", "s2"]
                    .iter()
                    .zip(["2026-09-30T03:00:00.000Z", "2026-09-30T01:00:00.000Z"])
                    .map(|(s, t)| Silent {
                        repo: "/w/r".into(),
                        session: s.to_string(),
                        from: t.into(),
                        to: t.into(),
                    })
                    .collect(),
            }
        );
    }

    #[test]
    fn a_row_is_the_session_whose_transcript_stem_wrote_it() {
        let mut r = row("R", "2026-09-30T00:00:00.000Z", "note");
        r.extra.insert("session".into(), "abc".into());
        r.extra.insert("branch".into(), "feat/other".into());
        assert!(mine(&r, "/h/.claude/projects/x/abc.jsonl", Some("feat/z")));
        assert!(!mine(
            &r,
            "/h/.claude/projects/x/def.jsonl",
            Some("feat/other")
        ));
    }
}
