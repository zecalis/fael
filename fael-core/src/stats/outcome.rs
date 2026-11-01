//! Outcome joins for `fael stats` (PLAN-fael-outcome-dayone chunk o-join):
//! ledger rows to PR numbers, pure.
//!
//! Three row→PR links, all durable (never a sha — a squash rewrites it):
//! close text `(#N)` (a squash subject the agent pasted into the close),
//! a main-branch commit holding `(fael:<prefix>)` plus `(#N)` in one message
//! (the prefix resolves to the row alone, `experience::resolve`), and the
//! row's branch matching the PR's `headRefName`. `FlowRun.pr → FlowMerge.at`
//! stays in `flow.rs` (`done_to_merge`); this module reuses its `Span`
//! helpers from `span.rs` and duplicates none of them.
//!
//! Dedup is by id across worktrees sharing a journal. A PR with no link
//! shrinks the join (`prs_without_row`), never imputed. Joins cover only
//! the listed PRs; `joined_since` carries the oldest listed PR so a reader
//! sees how far the 200-row list reaches.

use super::experience::{Commits, fael_cites, pr_cites, resolve};
use super::repeat::issue_closes;
use super::span::{Span, span_of};
use crate::{Log, ts_ms};
use serde::Serialize;
use std::collections::{HashMap, HashSet};

/// One listed PR from the shared `gh pr list` (`maintain::gh_prs`): its
/// number, head branch, and times as unix ms (`None` = missing/unreadable).
#[derive(Debug, Clone, Default)]
pub struct ListedPr {
    pub number: i64,
    pub branch: Option<String>,
    pub created_ms: Option<i64>,
    pub merged_ms: Option<i64>,
}

/// Outcome numbers: PRs merged/created, issues filed/closed, the
/// filed→closed span, and how many listed PRs carry a fael row.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Outcome {
    pub prs_merged: u64,
    pub prs_created: usize,
    pub prs_merged_unknown: bool,
    pub issues_filed: usize,
    pub issues_closed: usize,
    pub filed_to_close: Span,
    pub joined_since: String,
    pub prs_with_row: usize,
    pub prs_without_row: usize,
}

/// Issue adds in the window, earliest per id across worktrees.
fn adds(logs: &HashMap<String, Log>, in_window: impl Fn(i64) -> bool) -> HashMap<&str, i64> {
    let mut add: HashMap<&str, i64> = HashMap::new();
    for log in logs.values() {
        for r in log.rows.iter().filter(|r| r.kind == "issue") {
            if let Some(ms) = ts_ms(&r.ts)
                && in_window(ms)
            {
                add.entry(r.id.as_str())
                    .and_modify(|m| *m = (*m).min(ms))
                    .or_insert(ms);
            }
        }
    }
    add
}

/// Closes in the window, earliest per id, plus every close text per id
/// for the `(#N)` link (folded `fael compact` closes included).
fn closes(
    logs: &HashMap<String, Log>,
    in_window: impl Fn(i64) -> bool,
) -> (HashMap<&str, i64>, HashMap<&str, Vec<&str>>) {
    let mut close_ms: HashMap<&str, i64> = HashMap::new();
    let mut texts: HashMap<&str, Vec<&str>> = HashMap::new();
    for log in logs.values() {
        for (id, ms, text) in issue_closes(log) {
            if !in_window(ms) {
                continue;
            }
            close_ms
                .entry(id)
                .and_modify(|m| *m = (*m).min(ms))
                .or_insert(ms);
            texts.entry(id).or_default().push(text);
        }
    }
    (close_ms, texts)
}

/// Close `(#N)` → PR numbers, per row id.
fn link_close<'a>(texts: &HashMap<&'a str, Vec<&'a str>>) -> HashMap<&'a str, HashSet<i64>> {
    let mut out: HashMap<&str, HashSet<i64>> = HashMap::new();
    for (id, ts) in texts {
        for t in ts {
            for pr in pr_cites(t) {
                if let Ok(n) = pr[2..pr.len() - 1].parse::<i64>() {
                    out.entry(*id).or_default().insert(n);
                }
            }
        }
    }
    out
}

/// Commit `(fael:<prefix>)` + `(#N)` in one message → the resolved row.
fn link_commits<'a>(
    commits: &Commits,
    universe: &HashSet<&'a str>,
    into: &mut HashMap<&'a str, HashSet<i64>>,
) {
    let all: Vec<&str> = universe.iter().copied().collect();
    for repo_commits in commits.values() {
        for c in repo_commits {
            let cites: Vec<&str> = fael_cites(&c.message).collect();
            if cites.is_empty() {
                continue;
            }
            let numbers: Vec<i64> = pr_cites(&c.message)
                .filter_map(|pr| pr[2..pr.len() - 1].parse::<i64>().ok())
                .collect();
            if numbers.is_empty() {
                continue;
            }
            for p in cites {
                if let Some(id) = resolve(p, all.iter().copied()) {
                    into.entry(id).or_default().extend(numbers.iter());
                }
            }
        }
    }
}

/// Row branch = PR `headRefName` → the PR number, per row id.
fn link_branch<'a>(
    logs: &'a HashMap<String, Log>,
    universe: &HashSet<&'a str>,
    prs: &[ListedPr],
    into: &mut HashMap<&'a str, HashSet<i64>>,
) {
    let mut by_branch: HashMap<&str, Vec<&str>> = HashMap::new();
    for log in logs.values() {
        for r in &log.rows {
            if !universe.contains(r.id.as_str()) {
                continue;
            }
            if let Some(b) = r.branch() {
                by_branch.entry(b).or_default().push(r.id.as_str());
            }
        }
    }
    for pr in prs {
        if let Some(b) = pr.branch.as_deref()
            && let Some(ids) = by_branch.get(b)
        {
            for id in ids {
                into.entry(*id).or_default().insert(pr.number);
            }
        }
    }
}

/// Filed→closed durations in whole seconds, deduped by id.
fn filed_secs(add: &HashMap<&str, i64>, close_ms: &HashMap<&str, i64>) -> Vec<u64> {
    let mut out: Vec<u64> = vec![];
    for (id, a) in add {
        if let Some(c) = close_ms.get(id)
            && *c >= *a
        {
            out.push(((*c - *a) / 1000).max(0) as u64);
        }
    }
    out
}

/// Listed PRs with/without a row link.
fn with_without(row_prs: &HashMap<&str, HashSet<i64>>, prs: &[ListedPr]) -> (usize, usize) {
    let listed: HashSet<i64> = prs.iter().map(|p| p.number).collect();
    let with: HashSet<i64> = row_prs
        .values()
        .flatten()
        .filter(|n| listed.contains(n))
        .copied()
        .collect();
    (with.len(), listed.len().saturating_sub(with.len()))
}

/// Oldest listed PR as `YYYY-MM-DD`; `""` when none readable.
fn joined_since_str(prs: &[ListedPr]) -> String {
    prs.iter()
        .filter_map(|p| p.created_ms.or(p.merged_ms))
        .min()
        .map(|ms| {
            crate::rfc3339(ms.max(0) as u64)
                .get(..10)
                .unwrap_or("")
                .to_string()
        })
        .unwrap_or_default()
}

/// Join ledger rows to the listed PRs. `prs_merged_total` is the
/// `gh api search/issues` count for the range (`None` = offline/no `gh`,
/// reads unknown, never an error). `window` is `parse::outcome_window`:
/// row ends outside it do not count; PR links are over the listed PRs
/// whatever the window, with `joined_since` naming the list's reach.
pub fn outcome(
    logs: &HashMap<String, Log>,
    prs: &[ListedPr],
    prs_merged_total: Option<u64>,
    commits: &Commits,
    window: Option<i64>,
) -> Outcome {
    let in_window = |ms: i64| window.is_none_or(|w| ms >= w);
    let add = adds(logs, in_window);
    let (close_ms, texts) = closes(logs, in_window);
    let universe: HashSet<&str> = add
        .keys()
        .copied()
        .chain(close_ms.keys().copied())
        .collect();
    let mut row_prs = link_close(&texts);
    link_commits(commits, &universe, &mut row_prs);
    link_branch(logs, &universe, prs, &mut row_prs);
    let (with, without) = with_without(&row_prs, prs);
    Outcome {
        prs_merged: prs_merged_total.unwrap_or(0),
        prs_created: prs
            .iter()
            .filter(|p| p.created_ms.is_none_or(in_window))
            .count(),
        prs_merged_unknown: prs_merged_total.is_none(),
        issues_filed: add.len(),
        issues_closed: close_ms.len(),
        filed_to_close: span_of(filed_secs(&add, &close_ms)),
        joined_since: joined_since_str(prs),
        prs_with_row: with,
        prs_without_row: without,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    fn pr(number: i64, branch: &str, created: &str) -> ListedPr {
        ListedPr {
            number,
            branch: Some(branch.into()),
            created_ms: crate::ts_ms(created),
            merged_ms: None,
        }
    }

    #[test]
    fn squash_close_links_the_row_to_the_pr() {
        let issues = "{\"v\":1,\"id\":\"A\",\"ts\":\"2026-10-01T00:00:00Z\",\"by\":\"w\",\"kind\":\"issue\",\"text\":\"t\",\"files\":[]}\n";
        let closes = "{\"v\":1,\"id\":\"X\",\"ts\":\"2026-10-01T02:00:00Z\",\"by\":\"w\",\"kind\":\"close\",\"text\":\"fixed (#41)\",\"files\":[],\"ref\":\"A\"}\n";
        let logs = HashMap::from([("/w/r".to_string(), log_with(issues, closes))]);
        let prs = vec![pr(41, "feat/other", "2026-10-01T01:00:00Z")];
        let o = outcome(&logs, &prs, Some(1), &HashMap::new(), None);
        assert_eq!((o.prs_with_row, o.prs_without_row), (1, 0));
        assert_eq!(o.joined_since, "2026-10-01");
        assert_eq!(o.filed_to_close.n, 1);
        assert_eq!((o.issues_filed, o.issues_closed), (1, 1));
    }

    #[test]
    fn branch_name_match_links_the_row_to_the_pr() {
        let issues = "{\"v\":1,\"id\":\"B\",\"ts\":\"2026-10-02T00:00:00Z\",\"by\":\"w\",\"kind\":\"issue\",\"text\":\"t\",\"files\":[]}\n";
        // branch lives in `extra.branch`, stamped by the adapter
        let mut log = log_with(issues, "");
        log.rows[0]
            .extra
            .insert("branch".into(), "feat/work".into());
        let logs = HashMap::from([("/w/r".to_string(), log)]);
        let prs = vec![pr(42, "feat/work", "2026-10-02T01:00:00Z")];
        let o = outcome(&logs, &prs, Some(1), &HashMap::new(), None);
        assert_eq!((o.prs_with_row, o.prs_without_row), (1, 0));
        assert_eq!(o.issues_filed, 1);
        assert_eq!(o.issues_closed, 0);
        assert_eq!(o.filed_to_close.n, 0);
    }

    #[test]
    fn unmappable_prs_shrink_n_and_unknown_total_reads_unknown() {
        let logs = HashMap::from([("/w/r".to_string(), Log::default())]);
        let prs = vec![
            pr(7, "feat/a", "2026-10-03T00:00:00Z"),
            pr(8, "feat/b", "2026-10-19T00:00:00Z"),
        ];
        let o = outcome(&logs, &prs, None, &HashMap::new(), None);
        assert_eq!((o.prs_with_row, o.prs_without_row), (0, 2));
        assert!(o.prs_merged_unknown);
        assert_eq!(o.prs_merged, 0);
        assert_eq!(o.joined_since, "2026-10-03");
    }
}
