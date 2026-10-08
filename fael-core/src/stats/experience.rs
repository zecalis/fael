//! The experience loop's numbers (PLAN-fael-experience-loop chunk 4), read
//! as capture → acted → outcome beside what other blocks already count
//! (`said.finding`, `said.check`, `capture`, `context_loop`). Never inferred:
//! a fix is a close or a commit that names its evidence, a check is a
//! backticked path with a `/` in the close, a repeat is the agent's own
//! `--supersedes`. Pure: kept usage rows, loaded logs and the default
//! branch's commits (read by the caller) in.

use super::capture::mine;
use super::parse::Parsed;
use super::repeat::{closed_issues, pairs};
use crate::{Log, backtick_paths, ts_ms};
use serde::Serialize;
use std::collections::{HashMap, HashSet};

/// A commit on the repo's default branch since its first usage.
#[derive(Debug, Clone, PartialEq)]
pub struct Commit {
    pub sha: String,
    /// The whole message, subject first.
    pub message: String,
}

/// Repo path → its default branch's commits, as the caller read them.
pub type Commits = HashMap<String, Vec<Commit>>;

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Experience {
    /// Acted (H4): issues closed at or after the repo's first usage with a
    /// fix named — a sha or `(#N)` in the close, or a commit naming the id.
    pub fixed: usize,
    /// …of which the session that filed the issue was offered a review
    /// finding on one of its files before (`said.finding`).
    pub fixed_from_review: usize,
    /// Acted: issues closed at or after first usage whose close names a check.
    pub closed_with_check: usize,
    /// Outcome (H2): `context_loop` repeats / edits after close whose closed
    /// issue named a check; the rest of `context_loop` named none.
    pub repeats_with_check: usize,
    pub edits_after_close_with_check: usize,
    /// H5: `fix:`/`fix(` commits on the default branch, and of those the ones
    /// linked to fael — the message names a row id, or a close names its sha
    /// or its `(#N)`. Deduped by sha across worktrees.
    pub fix_commits: usize,
    pub fix_commits_linked: usize,
}

const SHORT: usize = 8;

/// Words of `text`: alphanumeric runs, `#` kept so `(#12)` reads `#12`.
fn words(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '#'))
        .filter(|w| !w.is_empty())
}

/// A sha as people type one: 7–40 hex digits holding a digit and a letter.
fn sha_like(w: &str) -> bool {
    (7..=40).contains(&w.len())
        && w.bytes().all(|b| b.is_ascii_hexdigit())
        && w.bytes().any(|b| b.is_ascii_digit())
        && w.bytes().any(|b| b.is_ascii_alphabetic())
}

fn pr_like(w: &str) -> bool {
    w.strip_prefix('#')
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

/// Every close text of `log`, by the issue it closes: close rows and the
/// close `fael compact` folded into the row.
fn close_texts(log: &Log) -> HashMap<&str, Vec<&str>> {
    let mut out: HashMap<&str, Vec<&str>> = HashMap::new();
    for c in &log.closes {
        if let Some(id) = c.reference.as_deref() {
            out.entry(id).or_default().push(&c.text);
        }
    }
    for r in &log.rows {
        if let Some(t) = r.extra.get("closed").and_then(|c| c["text"].as_str()) {
            out.entry(&r.id).or_default().push(t);
        }
    }
    out
}

fn names_check(texts: &[&str]) -> bool {
    texts
        .iter()
        .any(|t| backtick_paths(t).iter().any(|p| p.contains('/')))
}

/// Row ids a commit message names (a full id or a ≥ 8-char prefix).
fn cited<'a>(by_prefix: &HashMap<&str, Vec<&'a str>>, message: &str) -> Vec<&'a str> {
    words(message)
        .filter(|w| (SHORT..=26).contains(&w.len()))
        .flat_map(|w| {
            by_prefix
                .get(&w[..SHORT])
                .into_iter()
                .flatten()
                .filter(move |id| id.starts_with(w))
                .copied()
        })
        .collect()
}

pub(super) fn experience(
    parsed: &Parsed,
    logs: &HashMap<String, Log>,
    commits: &Commits,
) -> Experience {
    // (repo, session, file, ms) of each finding line
    let mut findings: Vec<(&str, &str, &str, i64)> = vec![];
    for v in &parsed.kept {
        let (Some(repo), Some(s), Some(ms)) = (
            v["repo"].as_str(),
            v["session"].as_str(),
            v["ts"].as_str().and_then(ts_ms),
        ) else {
            continue;
        };
        let said = v["said"].as_array().into_iter().flatten();
        findings.extend(
            said.filter(|e| e["kind"] == "finding")
                .filter_map(|e| Some((repo, s, e["key"].as_str()?, ms))),
        );
    }
    let mut e = Experience::default();
    let (mut fixed, mut from_review, mut checked) =
        (HashSet::new(), HashSet::new(), HashSet::new());
    let (mut fix_shas, mut linked) = (HashSet::new(), HashSet::new());
    let mut with_check: HashMap<&str, HashSet<&str>> = HashMap::new();
    for (repo, first) in &parsed.first_seen {
        let Some(log) = logs.get(repo) else { continue };
        let texts = close_texts(log);
        let mut by_prefix: HashMap<&str, Vec<&str>> = HashMap::new();
        for r in log.rows.iter().filter(|r| r.id.len() >= SHORT) {
            by_prefix.entry(&r.id[..SHORT]).or_default().push(&r.id);
        }
        let repo_commits = commits.get(repo).map_or(&[][..], Vec::as_slice);
        let in_commit: HashSet<&str> = repo_commits
            .iter()
            .flat_map(|c| cited(&by_prefix, &c.message))
            .collect();
        let closed = closed_issues(log);
        with_check.insert(
            repo,
            closed
                .keys()
                .filter(|id| texts.get(*id).is_some_and(|t| names_check(t)))
                .copied()
                .collect(),
        );
        for r in log
            .rows
            .iter()
            .filter(|r| closed.get(r.id.as_str()) >= Some(first))
        {
            let id = r.id.as_str();
            let said = texts.get(id).map_or(&[][..], Vec::as_slice);
            if with_check[repo.as_str()].contains(id) {
                checked.insert(id);
            }
            let evidence = said
                .iter()
                .flat_map(|t| words(t))
                .any(|w| sha_like(w) || pr_like(w));
            if !evidence && !in_commit.contains(id) {
                continue;
            }
            fixed.insert(id);
            let at = ts_ms(&r.ts).unwrap_or(i64::MAX);
            let reviewed = r.session().is_some()
                && findings.iter().any(|(fr, s, f, ms)| {
                    fr == repo && *ms <= at && r.files.iter().any(|x| x == f) && mine(r, s, None)
                });
            if reviewed {
                from_review.insert(id);
            }
        }
        // H5: a fix commit is linked when it names a row, or a close names it
        let words_in_closes: HashSet<&str> =
            texts.values().flatten().flat_map(|t| words(t)).collect();
        for c in repo_commits {
            let subject = c.message.lines().next().unwrap_or("");
            if !(subject.starts_with("fix:") || subject.starts_with("fix(")) {
                continue;
            }
            fix_shas.insert(c.sha.as_str());
            let by_sha = words_in_closes
                .iter()
                .any(|w| sha_like(w) && c.sha.starts_with(&w.to_ascii_lowercase()));
            let by_pr = words(subject).any(|w| pr_like(w) && words_in_closes.contains(w));
            if by_sha || by_pr || !cited(&by_prefix, &c.message).is_empty() {
                linked.insert(c.sha.as_str());
            }
        }
    }
    let p = pairs(parsed, logs);
    let named = |repo: &str, id: &str| with_check.get(repo).is_some_and(|s| s.contains(id));
    e.repeats_with_check = p.repeats.values().filter(|(r, old)| named(r, old)).count();
    e.edits_after_close_with_check = p.edits.iter().filter(|((_, id), r)| named(r, id)).count();
    (e.fixed, e.fixed_from_review, e.closed_with_check) =
        (fixed.len(), from_review.len(), checked.len());
    (e.fix_commits, e.fix_commits_linked) = (fix_shas.len(), linked.len());
    e
}
