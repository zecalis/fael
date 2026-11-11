//! One row per session (issue 01M75KQ5): what a session was handed and what
//! it did, from the same parsed lines the aggregates read — so a stranger can
//! check a number against the session that made it. A description, never a
//! claim: the hit and acted rules are `value.rs`'s and `touch.rs`'s, called
//! here, not restated. MCP lines carry no session and stay out. Pure: parsed
//! usage, loaded logs and crew.db runs in.

use super::FlowRun;
use super::capture::{by_time, filed, spans};
use super::friction::{is_agent_call, is_gate_reject};
use super::outcome::{ListedPr, pr_for_branch};
use super::parse::{Parsed, session_stem};
use super::value::{acted_pairs, pairs};
use crate::{Log, rfc3339, ts_ms};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap, HashSet};

/// The listed PR a session's branch heads. A branch can carry several
/// sessions, and this never says the PR holds this session's edits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SessionPr {
    pub number: i64,
    /// `YYYY-MM-DD` UTC; absent while the PR is not merged.
    pub merged: Option<String>,
}

/// A session's transcript summed at its last stop (Claude only).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct SessionTokens {
    pub input: u64,
    pub cache_creation: u64,
    pub cache_read: u64,
    pub output: u64,
}

/// One session's numbers, keyed by (repo, session stem).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SessionView {
    pub session: String,
    pub repo: String,
    /// The client on the session's push lines (its call lines' when it had none).
    pub client: String,
    /// First and last usage line of the session, RFC 3339 UTC.
    pub first: String,
    pub last: String,
    /// Branch seen on most of its edit lines (ties: the later name); absent when it never edited.
    pub branch: Option<String>,
    /// The listed PR whose `headRefName` is `branch`; absent when none is listed.
    pub pr: Option<SessionPr>,
    /// `pr` was looked up: the session's repo is the clone whose PR list was
    /// read. False = unknown (another repo, or no list), which `pr: null` alone
    /// would hide.
    pub pr_checked: bool,
    pub edits: usize,
    /// Est tokens fael pushed into the session, by usage event.
    pub pushed_tokens: BTreeMap<String, usize>,
    /// Billed-side tokens from the transcript at the session's last stop, each
    /// assistant message once; `None` = unknown (Codex, OpenCode, no stop yet), never 0.
    pub tokens: Option<SessionTokens>,
    /// Distinct rows a push handed over.
    pub shown: usize,
    /// …of those, the ones pushed at or after the client's first `in-context`
    /// line: the only pushes that could score, and the denominator the next two share.
    pub scored: usize,
    /// …of which still in context when the agent edited the row's file.
    pub in_context: usize,
    /// …of which the session acted on (in context, cited, found, or touched the file).
    pub acted: usize,
    /// Agent calls to fael (shape-gate rejects are no call, as in `friction`).
    pub calls: usize,
    pub rejects: usize,
    /// Rows the session filed; `None` when its repo's log was not loaded.
    pub rows_filed: Option<usize>,
    /// A crew run's session equals this one (the join `handoff.rs` uses).
    pub in_run: bool,
}

/// One group of the split tables, over sessions with at least one edit.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Group {
    pub sessions: usize,
    pub edits: usize,
    pub calls: usize,
    pub rejects: usize,
    pub rows_filed: usize,
    /// Sessions whose branch heads a merged listed PR.
    pub merged_pr: usize,
    /// Sessions whose log was loaded and that filed no row.
    pub no_row: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Split {
    pub with: Group,
    pub without: Group,
}

/// Two descriptive splits of edited sessions: rows pushed ≥ 1 against none,
/// inside a chunk run against outside. The groups differ in task, client and
/// repo; no number here is an effect of fael.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Splits {
    pub pushed: Split,
    pub in_run: Split,
}

#[derive(Default)]
struct Acc<'a> {
    client: &'a str,
    edits: usize,
    toks: BTreeMap<String, usize>,
    calls: usize,
    rejects: usize,
    branches: HashMap<&'a str, usize>,
}

/// Pairs per (repo, session).
fn count<'a>(
    keys: impl Iterator<Item = &'a (&'a str, &'a str, &'a str)>,
) -> HashMap<(&'a str, &'a str), usize> {
    let mut m = HashMap::new();
    for &(r, s, _) in keys {
        *m.entry((r, s)).or_default() += 1;
    }
    m
}

pub fn sessions(
    parsed: &Parsed,
    logs: &HashMap<String, Log>,
    runs: &[FlowRun],
) -> (Vec<SessionView>, Splits) {
    let spans = spans(parsed);
    let (all, _) = pairs(parsed, false);
    let (scored, in_ctx) = pairs(parsed, true);
    let acted = acted_pairs(parsed, logs, &scored, &in_ctx);
    let mut acc: HashMap<(&str, &str), Acc> = HashMap::new();
    for v in &parsed.kept {
        let (Some(repo), Some(s)) = (v["repo"].as_str(), v["session"].as_str()) else {
            continue;
        };
        let a = acc.entry((repo, s)).or_default();
        if a.client.is_empty() {
            a.client = v["client"].as_str().unwrap_or("");
        }
        if v["event"] == "edit" {
            a.edits += 1;
            if let Some(b) = v["branch"].as_str() {
                *a.branches.entry(b).or_default() += 1;
            }
        }
    }
    for v in parsed.calls.iter().filter(|v| is_agent_call(v)) {
        let (Some(repo), Some(s)) = (v["repo"].as_str(), v["session"].as_str()) else {
            continue;
        };
        let a = acc.entry((repo, s)).or_default();
        if a.client.is_empty() {
            a.client = v["client"].as_str().unwrap_or("");
        }
        if is_gate_reject(v)
            || v["ts"].as_str().and_then(ts_ms).is_none()
            || !matches!(v["outcome"].as_str(), Some("ok" | "reject" | "help"))
        {
            continue;
        }
        a.calls += 1;
        a.rejects += usize::from(v["outcome"] == "reject");
    }
    for r in parsed.rows.iter().filter(|r| !r.session.is_empty()) {
        *acc.entry((&r.repo, &r.session))
            .or_default()
            .toks
            .entry(r.event.clone())
            .or_default() += r.toks;
    }
    let (shown, scored_n) = (count(all.keys()), count(scored.keys()));
    let (hit, did) = (count(in_ctx.iter()), count(acted.iter()));
    let by_ms = by_time(logs);
    let in_runs: HashSet<&str> = runs
        .iter()
        .filter_map(|r| r.session.as_deref().map(session_stem))
        .collect();
    let n = |m: &HashMap<(&str, &str), usize>, k| m.get(&k).copied().unwrap_or(0);
    let at = |ms: i64| rfc3339(ms.max(0) as u64);
    let mut out: Vec<SessionView> = acc
        .into_iter()
        .filter_map(|(k, a)| {
            let span = spans.get(&k)?;
            let branch = a
                .branches
                .iter()
                .max_by_key(|&(b, n)| (*n, *b))
                .map(|(b, _)| *b);
            Some(SessionView {
                session: k.1.to_string(),
                repo: k.0.to_string(),
                client: a.client.to_string(),
                first: at(span.0),
                last: at(span.1),
                branch: branch.map(String::from),
                pr: None,
                pr_checked: false,
                edits: a.edits,
                tokens: tokens_of(parsed, k),
                pushed_tokens: a.toks,
                shown: n(&shown, k),
                scored: n(&scored_n, k),
                in_context: n(&hit, k),
                acted: n(&did, k),
                calls: a.calls,
                rejects: a.rejects,
                rows_filed: by_ms.get(k.0).map(|rows| filed(rows, k.1, span).len()),
                in_run: in_runs.contains(k.1),
            })
        })
        .collect();
    out.sort_by(|a, b| (&b.last, &a.session, &a.repo).cmp(&(&a.last, &b.session, &b.repo)));
    let splits = splits(&out);
    (out, splits)
}

fn tokens_of(parsed: &Parsed, k: (&str, &str)) -> Option<SessionTokens> {
    let &[input, cache_creation, cache_read, output] = parsed
        .session_tokens
        .get(&(k.0.to_string(), k.1.to_string()))?;
    Some(SessionTokens {
        input,
        cache_creation,
        cache_read,
        output,
    })
}

/// Join each session of `repo` to the listed PR its branch heads, then
/// recount the splits (they hold `merged_pr`). Sessions of any other repo stay
/// unchecked: the list is one clone's `gh pr list`, and a branch name alone
/// would match another repo's PR. A PR merged before the session's first line
/// cannot hold its edits (a reused branch name), so it is skipped. Pure.
pub fn link_prs(list: &mut [SessionView], prs: &[ListedPr], repo: &str) -> Splits {
    if !prs.is_empty() {
        for v in list.iter_mut().filter(|v| v.repo == repo) {
            let first = ts_ms(&v.first).unwrap_or(0);
            v.pr_checked = true;
            v.pr = v
                .branch
                .as_deref()
                .and_then(|b| pr_for_branch(prs, b, first))
                .map(|p| SessionPr {
                    number: p.number,
                    merged: p
                        .merged_ms
                        .map(|ms| rfc3339(ms.max(0) as u64)[..10].to_string()),
                });
        }
    }
    splits(list)
}

fn splits(list: &[SessionView]) -> Splits {
    let mut s = Splits::default();
    for v in list.iter().filter(|v| v.edits > 0) {
        for (split, with) in [(&mut s.pushed, v.shown > 0), (&mut s.in_run, v.in_run)] {
            let g = if with {
                &mut split.with
            } else {
                &mut split.without
            };
            g.sessions += 1;
            g.edits += v.edits;
            g.calls += v.calls;
            g.rejects += v.rejects;
            g.rows_filed += v.rows_filed.unwrap_or(0);
            g.no_row += usize::from(v.rows_filed == Some(0));
            g.merged_pr += usize::from(v.pr.as_ref().is_some_and(|p| p.merged.is_some()));
        }
    }
    s
}

/// The sessions `q` names: a full stem or a unique prefix of one. One stem in
/// several repos returns each; a prefix of several stems is an error naming them.
pub fn find_session<'a>(list: &'a [SessionView], q: &str) -> Result<Vec<&'a SessionView>, String> {
    let q = session_stem(q);
    let exact: Vec<_> = list.iter().filter(|v| v.session == q).collect();
    if !exact.is_empty() {
        return Ok(exact);
    }
    let hits: Vec<_> = list.iter().filter(|v| v.session.starts_with(q)).collect();
    let stems: HashSet<&str> = hits.iter().map(|v| v.session.as_str()).collect();
    match stems.len() {
        0 => Err(format!(
            "rejected: no session in the window starts with {q:?}"
        )),
        1 => Ok(hits),
        _ => {
            let mut s: Vec<_> = stems.into_iter().collect();
            s.sort_unstable();
            Err(format!(
                "rejected: {q:?} starts {} sessions — longer prefix: {}",
                s.len(),
                s.join(", ")
            ))
        }
    }
}
