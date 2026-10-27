//! Runs that started from a brief carrying an `after` hand-off, beside the
//! runs that did not (issue 01M5Y9KQ). A description of two groups, nothing
//! more: the groups differ in many ways fael cannot see, so no number here
//! says what the hand-off did. Pure: usage rows and crew.db runs in.
//!
//! The unit is a closed run (`ended` set) with a session, started inside the
//! stats window. A run's usage is the session's lines stamped inside its own
//! span; when two runs share a session, a line counts once, for the run with
//! the latest start that still holds it.

use super::FlowRun;
use super::parse::{Parsed, session_stem};
use crate::ts_ms;
use serde::Serialize;
use std::collections::{HashMap, HashSet};

/// One group's counts: `joined` runs had at least one usage line; `finds` are
/// find and mcp-find lines, `refinds` the ones whose ids an earlier find in
/// the same run had all returned; `rejects` are call lines with outcome reject.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Side {
    pub runs: usize,
    pub joined: usize,
    pub done: usize,
    pub finds: usize,
    pub refinds: usize,
    pub rejects: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct HandoffRuns {
    pub with: Side,
    pub without: Side,
}

struct Line<'a> {
    ms: i64,
    event: &'a str,
    outcome: &'a str,
    found: Vec<&'a str>,
}

pub fn handoff_runs(parsed: &Parsed, runs: &[FlowRun]) -> HandoffRuns {
    let mut out = HandoffRuns::default();
    let Some(first) = parsed.first_seen.values().min().copied() else {
        return out;
    };
    let eligible: Vec<(&FlowRun, i64, i64)> = runs
        .iter()
        .filter_map(|r| {
            let (s, e) = (r.started_ms?, r.ended_ms?);
            (r.session.is_some() && s >= first).then_some((r, s, e))
        })
        .collect();
    let mut by_session: HashMap<&str, Vec<usize>> = HashMap::new();
    for (i, (r, ..)) in eligible.iter().enumerate() {
        let stem = r.session.as_deref().map(session_stem).unwrap_or_default();
        by_session.entry(stem).or_default().push(i);
    }
    let mut lines: Vec<Vec<Line>> = (0..eligible.len()).map(|_| vec![]).collect();
    for v in &parsed.kept {
        let (Some(s), Some(ms)) = (v["session"].as_str(), v["ts"].as_str().and_then(ts_ms)) else {
            continue;
        };
        let owner = by_session
            .get(session_stem(s))
            .into_iter()
            .flatten()
            .filter(|&&i| eligible[i].1 <= ms && ms <= eligible[i].2)
            .max_by_key(|&&i| eligible[i].1);
        if let Some(&i) = owner {
            lines[i].push(Line {
                ms,
                event: v["event"].as_str().unwrap_or(""),
                outcome: v["outcome"].as_str().unwrap_or(""),
                found: v["found"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|x| x.as_str())
                    .collect(),
            });
        }
    }
    for ((r, ..), mut ls) in eligible.into_iter().zip(lines) {
        let side = if r.after_handoff {
            &mut out.with
        } else {
            &mut out.without
        };
        side.runs += 1;
        side.done += usize::from(r.done);
        side.joined += usize::from(!ls.is_empty());
        ls.sort_by_key(|l| l.ms);
        let mut seen: HashSet<&str> = HashSet::new();
        for l in &ls {
            if l.event == "call" && l.outcome == "reject" {
                side.rejects += 1;
            }
            if matches!(l.event, "find" | "mcp-find") && !l.found.is_empty() {
                side.finds += 1;
                side.refinds += usize::from(l.found.iter().all(|i| seen.contains(i)));
                seen.extend(l.found.iter().copied());
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stats::parse::parse;
    use std::path::Path;

    fn line(ts: &str, session: &str, rest: &str) -> String {
        format!(r#"{{"ts":"{ts}","repo":"/w/r","client":"claude","session":"{session}",{rest}}}"#)
    }

    fn run(session: &str, after: bool, start: &str, end: Option<&str>) -> FlowRun {
        FlowRun {
            started_ms: ts_ms(start),
            ended_ms: end.and_then(ts_ms),
            session: Some(session.into()),
            after_handoff: after,
            done: after,
            ..Default::default()
        }
    }

    fn parsed(lines: &[String]) -> Parsed {
        parse(&lines.join("\n"), Path::new("/state"), &[])
    }

    const S: &str = "2026-10-02T00:00:00Z";
    const E: &str = "2026-10-02T02:00:00Z";

    fn lines() -> Vec<String> {
        vec![
            line(
                "2026-10-01T00:00:00.000Z",
                "x",
                r#""event":"read","bytes":1"#,
            ),
            line(
                "2026-10-02T00:10:00.000Z",
                "/p/a.jsonl",
                r#""event":"find","found":["A","B"]"#,
            ),
            line(
                "2026-10-02T00:20:00.000Z",
                "a",
                r#""event":"mcp-find","found":["B"]"#,
            ),
            line(
                "2026-10-02T00:30:00.000Z",
                "a",
                r#""event":"call","cmd":"add","outcome":"reject","reason":"x""#,
            ),
            line(
                "2026-10-02T00:40:00.000Z",
                "b",
                r#""event":"call","cmd":"add","outcome":"ok""#,
            ),
        ]
    }

    #[test]
    fn with_counts_finds_refinds_and_rejects_by_session_stem() {
        let r = [run("a", true, S, Some(E))];
        let h = handoff_runs(&parsed(&lines()), &r);
        let side = Side {
            runs: 1,
            joined: 1,
            done: 1,
            finds: 2,
            refinds: 1,
            rejects: 1,
        };
        assert_eq!((h.with, h.without), (side, Side::default()));
    }

    #[test]
    fn without_is_every_other_run_and_unjoined_runs_still_count() {
        let r = [run("b", false, S, Some(E)), run("nope", false, S, Some(E))];
        let h = handoff_runs(&parsed(&lines()), &r);
        assert_eq!(h.with, Side::default());
        assert_eq!(
            (h.without.runs, h.without.joined, h.without.rejects),
            (2, 1, 0)
        );
    }

    #[test]
    fn open_runs_and_runs_before_the_window_are_left_out() {
        let r = [
            run("a", true, S, None),
            run("a", true, "2026-09-30T00:00:00Z", Some(E)),
        ];
        assert_eq!(handoff_runs(&parsed(&lines()), &r), HandoffRuns::default());
    }

    #[test]
    fn a_line_counts_once_when_two_runs_share_a_session() {
        let r = [
            run("a", true, S, Some(E)),
            run("a", false, "2026-10-02T00:15:00Z", Some(E)),
        ];
        let h = handoff_runs(&parsed(&lines()), &r);
        assert_eq!((h.with.finds, h.without.finds), (1, 1));
    }
}
