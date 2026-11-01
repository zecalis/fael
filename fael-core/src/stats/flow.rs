//! Flow time + throughput (PLAN-fael-flow chunk f1): how long an issue
//! takes to close, a chunk to finish, a done chunk to merge — and how many
//! chunks land per week. Pure: kept usage rows (for the window) and loaded
//! logs in, run spans and merge times in, numbers out.
//!
//! Three spans, each `{median, p90, n}` in whole seconds, never a mean:
//! filed→closed from the journal add ts to the close row ts for the same id
//! (close rows live in `*.close.jsonl` with `ref` = the issue id, plus the
//! folded `fael compact` close); start→done from the run table's
//! `started`/`ended`; done→merged from `Run.pr` to the PR's merge time on the
//! default branch. A span with a missing endpoint shrinks `n`, never imputed;
//! a merge before `ended` never counts (the clock moved, not the work).
//! Matching is durable only, by PR number (`(#N)` in a squash subject, a
//! merge commit, or `gh`'s merged list) — never a sha (a squash rewrites it,
//! decision 01M580PY). PR numbers are unique, so no `(fael:<id>)` or branch
//! match is needed; runs without a PR shrink `n`, never imputed.
//! Throughput counts distinct chunk-done per calendar week (Monday, UTC) from
//! `Run.ended`; weeks with zero dones print zero, never missing.

use super::parse::Parsed;
use super::repeat::issue_closes;
use super::span::{Span, day_string, monday_days, span_of};
use crate::{Log, ts_ms};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap, HashSet};

/// The three flow spans plus the done→merge mapping coverage:
/// `runs_with_pr` of `runs_total` runs carry a PR (e.g. 105/136).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Flow {
    pub filed_to_close: Span,
    pub start_to_done: Span,
    pub done_to_merge: Span,
    pub runs_with_pr: usize,
    pub runs_total: usize,
}

/// One calendar week (its Monday, `YYYY-MM-DD`, UTC) and chunk dones in it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct WeekCount {
    pub week: String,
    pub done: usize,
}

/// One run for the flow join: times as unix ms, `None` = missing endpoint.
/// `chunk` is the run's chunk uid — throughput dedupes restarts on it, so one
/// chunk lands in exactly one week; spans still read every run (each attempt).
#[derive(Debug, Clone, Default)]
pub struct FlowRun {
    pub started_ms: Option<i64>,
    pub ended_ms: Option<i64>,
    pub pr: Option<i64>,
    pub chunk: String,
    /// For `handoff`: the session (path or id), an `after` hand-off at start, a done inside.
    pub session: Option<String>,
    pub after_handoff: bool,
    pub done: bool,
}

/// One merged PR on the default branch: its number and merge time as unix ms.
#[derive(Debug, Clone)]
pub struct FlowMerge {
    pub number: i64,
    pub at_ms: i64,
}

pub fn flow(
    parsed: &Parsed,
    logs: &HashMap<String, Log>,
    runs: &[FlowRun],
    merges: &[FlowMerge],
) -> Flow {
    // filed→closed: one duration per issue, deduped by id across worktrees
    // that share a journal; both ends at or after the repo's first usage.
    let mut seen: HashSet<&str> = HashSet::new();
    let mut filed: Vec<u64> = vec![];
    for (repo, first) in &parsed.first_seen {
        let Some(log) = logs.get(repo) else { continue };
        let mut add: HashMap<&str, i64> = HashMap::new();
        for r in log.rows.iter().filter(|r| r.kind == "issue") {
            if let Some(ms) = ts_ms(&r.ts)
                && ms >= *first
            {
                add.entry(r.id.as_str())
                    .and_modify(|m| *m = (*m).min(ms))
                    .or_insert(ms);
            }
        }
        let mut close: HashMap<&str, i64> = HashMap::new();
        for (id, close_ms, _) in issue_closes(log) {
            if close_ms >= *first {
                close
                    .entry(id)
                    .and_modify(|m| *m = (*m).min(close_ms))
                    .or_insert(close_ms);
            }
        }
        for (id, a) in &add {
            if !seen.insert(*id) {
                continue;
            }
            if let Some(c) = close.get(id)
                && *c >= *a
            {
                filed.push(((*c - *a) / 1000).max(0) as u64);
            }
        }
    }
    // runs in the usage window: `ended` at or after the earliest first use
    let window = parsed.first_seen.values().copied().min();
    let in_window = |ms: i64| window.is_none_or(|w| ms >= w);
    let mut start: Vec<u64> = vec![];
    let mut with_pr = 0usize;
    let mut total = 0usize;
    let mut by_pr: HashMap<i64, Vec<i64>> = HashMap::new();
    for m in merges {
        by_pr.entry(m.number).or_default().push(m.at_ms);
    }
    for v in by_pr.values_mut() {
        v.sort_unstable();
    }
    let mut merged: Vec<u64> = vec![];
    for r in runs {
        let (Some(s), Some(e)) = (r.started_ms, r.ended_ms) else {
            continue;
        };
        if !in_window(e) || e < s {
            continue;
        }
        total += 1;
        start.push(((e - s) / 1000).max(0) as u64);
        let Some(pr) = r.pr else { continue };
        with_pr += 1;
        if let Some(ats) = by_pr.get(&pr)
            && let Some(at) = ats.iter().find(|at| **at >= e)
        {
            merged.push(((*at - e) / 1000).max(0) as u64);
        }
    }
    Flow {
        filed_to_close: span_of(filed),
        start_to_done: span_of(start),
        done_to_merge: span_of(merged),
        runs_with_pr: with_pr,
        runs_total: total,
    }
}

/// Chunk dones per calendar week (Monday, UTC) from `Run.ended` in the usage
/// window; weeks between the first and last done week print zero, never
/// missing. One chunk lands once: restarts (several runs, one uid) keep the
/// latest `ended`, so the series sums to the distinct done-chunk count.
/// A run with no chunk uid (never from the loader) still counts on its own.
pub fn throughput(parsed: &Parsed, runs: &[FlowRun]) -> Vec<WeekCount> {
    let window = parsed.first_seen.values().copied().min();
    let mut latest: HashMap<&str, i64> = HashMap::new();
    let mut unkeyed: Vec<i64> = vec![];
    for r in runs {
        let Some(e) = r.ended_ms else { continue };
        if window.is_some_and(|w| e < w) {
            continue;
        }
        if r.chunk.is_empty() {
            unkeyed.push(e);
        } else {
            latest
                .entry(r.chunk.as_str())
                .and_modify(|m| *m = (*m).max(e))
                .or_insert(e);
        }
    }
    let mut per_week: BTreeMap<i64, usize> = BTreeMap::new();
    for e in latest.values().copied().chain(unkeyed) {
        *per_week.entry(monday_days(e)).or_default() += 1;
    }
    let (Some(&first), Some(&last)) = (per_week.keys().next(), per_week.keys().next_back()) else {
        return vec![];
    };
    let mut out = vec![];
    let mut d = first;
    while d <= last {
        out.push(WeekCount {
            week: day_string(d),
            done: per_week.get(&d).copied().unwrap_or(0),
        });
        d += 7;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    fn parsed_of(text: &str) -> Parsed {
        super::super::parse::parse(
            text,
            Path::new("/w/state/usage.jsonl"),
            &[PathBuf::from("/tmp")],
        )
    }

    fn usage(repo: &str, ts: &str) -> String {
        format!(
            "{{\"ts\":\"{ts}\",\"repo\":\"{repo}\",\"client\":\"claude\",\"event\":\"read\",\"bytes\":1,\"est_tokens\":1,\"ids\":[]}}\n"
        )
    }

    fn rows(jsonl: &str) -> Vec<crate::Row> {
        let (mut out, mut w) = (vec![], vec![]);
        crate::log::parse(jsonl.as_bytes(), "t.jsonl", &mut out, &mut w);
        out
    }

    fn log_with(issues: &str, closes: &str) -> Log {
        Log {
            rows: rows(issues),
            closes: rows(closes),
            ..Default::default()
        }
    }

    #[test]
    fn filed_to_close_reports_median_p90_and_n() {
        let p = parsed_of(&usage("/w/r", "2026-10-01T00:00:00.000Z"));
        let issues = concat!(
            "{\"v\":1,\"id\":\"A\",\"ts\":\"2026-10-01T00:00:00Z\",\"by\":\"w\",\"kind\":\"issue\",\"text\":\"t\",\"files\":[]}\n",
            "{\"v\":1,\"id\":\"B\",\"ts\":\"2026-10-01T00:00:00Z\",\"by\":\"w\",\"kind\":\"issue\",\"text\":\"t\",\"files\":[]}\n",
            "{\"v\":1,\"id\":\"C\",\"ts\":\"2026-10-01T00:00:00Z\",\"by\":\"w\",\"kind\":\"issue\",\"text\":\"t\",\"files\":[]}\n",
        );
        let closes = concat!(
            "{\"v\":1,\"id\":\"X\",\"ts\":\"2026-10-01T01:00:00Z\",\"by\":\"w\",\"kind\":\"close\",\"text\":\"t\",\"files\":[],\"ref\":\"A\"}\n",
            "{\"v\":1,\"id\":\"Y\",\"ts\":\"2026-10-01T02:00:00Z\",\"by\":\"w\",\"kind\":\"close\",\"text\":\"t\",\"files\":[],\"ref\":\"B\"}\n",
            "{\"v\":1,\"id\":\"Z\",\"ts\":\"2026-10-01T09:00:00Z\",\"by\":\"w\",\"kind\":\"close\",\"text\":\"t\",\"files\":[],\"ref\":\"C\"}\n",
        );
        let f = flow(
            &p,
            &HashMap::from([("/w/r".to_string(), log_with(issues, closes))]),
            &[],
            &[],
        );
        assert_eq!(
            f.filed_to_close,
            Span {
                median: 7200,
                p90: 32400,
                n: 3
            },
            "{f:?}"
        );
        assert_eq!((f.runs_with_pr, f.runs_total), (0, 0));
    }

    #[test]
    fn run_spans_shrink_on_missing_ends_and_unmappable_merges() {
        let p = parsed_of(&usage("/w/r", "2026-10-01T00:00:00.000Z"));
        let t = |s: &str| crate::ts_ms(s).unwrap();
        let run = |e: Option<&str>, pr: Option<i64>| FlowRun {
            started_ms: Some(t("2026-10-02T00:00:00Z")),
            ended_ms: e.map(t),
            pr,
            chunk: format!("C{}", pr.unwrap_or(0)),
            ..Default::default()
        };
        let runs = vec![
            run(Some("2026-10-02T00:10:00Z"), Some(1)),
            run(Some("2026-10-02T00:20:00Z"), Some(2)),
            // no merge for pr 2: shrinks done→merge n, never imputed
            run(None, Some(3)),
            // merge before ended: clock moved, not work — left out
            run(Some("2026-10-02T01:00:00Z"), Some(4)),
        ];
        let merges = vec![
            FlowMerge {
                number: 1,
                at_ms: t("2026-10-02T00:15:00Z"),
            },
            FlowMerge {
                number: 4,
                at_ms: t("2026-10-02T00:30:00Z"),
            },
        ];
        let logs = HashMap::from([("/w/r".to_string(), Log::default())]);
        let f = flow(&p, &logs, &runs, &merges);
        assert_eq!(
            f.start_to_done,
            Span {
                median: 1200,
                p90: 3600,
                n: 3
            },
            "{f:?}"
        );
        assert_eq!(
            f.done_to_merge,
            Span {
                median: 300,
                p90: 300,
                n: 1
            },
            "{f:?}"
        );
        assert_eq!((f.runs_with_pr, f.runs_total), (3, 3));
    }

    #[test]
    fn throughput_weeks_fill_zeros_never_missing() {
        let p = parsed_of(&usage("/w/r", "2026-10-01T00:00:00.000Z"));
        let t = |s: &str| crate::ts_ms(s).unwrap();
        let run = |c: &str, e: &str| FlowRun {
            started_ms: Some(t(e)),
            ended_ms: Some(t(e)),
            chunk: c.into(),
            ..Default::default()
        };
        // Mon 2026-10-05 x2, Mon 2026-10-19 x1: the middle week prints zero
        let runs = vec![
            run("A", "2026-10-05T12:00:00Z"),
            run("B", "2026-10-06T12:00:00Z"),
            run("C", "2026-10-19T12:00:00Z"),
        ];
        assert_eq!(
            throughput(&p, &runs),
            vec![
                WeekCount {
                    week: "2026-10-05".into(),
                    done: 2
                },
                WeekCount {
                    week: "2026-10-12".into(),
                    done: 0
                },
                WeekCount {
                    week: "2026-10-19".into(),
                    done: 1
                },
            ]
        );
        assert!(throughput(&p, &[]).is_empty());
    }

    #[test]
    fn throughput_counts_a_restarted_chunk_once_in_its_latest_week() {
        let p = parsed_of(&usage("/w/r", "2026-10-01T00:00:00.000Z"));
        let t = |s: &str| crate::ts_ms(s).unwrap();
        let run = |c: &str, e: &str| FlowRun {
            started_ms: Some(t(e)),
            ended_ms: Some(t(e)),
            chunk: c.into(),
            ..Default::default()
        };
        // chunk A restarted: first done Mon 2026-10-05, redone Mon 2026-10-12 —
        // one chunk, latest week wins, never double-counted
        let runs = vec![
            run("A", "2026-10-05T12:00:00Z"),
            run("A", "2026-10-12T12:00:00Z"),
            run("B", "2026-10-12T13:00:00Z"),
        ];
        assert_eq!(
            throughput(&p, &runs),
            vec![WeekCount {
                week: "2026-10-12".into(),
                done: 2
            },]
        );
    }
}
