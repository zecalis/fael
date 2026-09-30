//! `[Phantom]` (PLAN-fael-id-refs chunk 3): open rows — and close reasons —
//! citing an id-shaped token with no row behind it, plus the same dead
//! citation written in any `*.md` under the repo (issue `ids:doctor-plans`).
//! Split out of `rows.rs` next to `fat.rs`/`orphan.rs` so `files_notes` stays
//! under the 100-line function cap. Union-scope candidates come from core
//! (`phantom_refs`, `phantom_md_refs`); the one branch escalation lives here,
//! never in core (core spawns no processes) — `doctor` already spawns `gh`,
//! so one git spawn is fine.
//!
//! Only close *texts* and open rows are scanned in the log: a closed row's own
//! text went quiet with the row, but its close reason still speaks for it.
//! Markdown is read as a whole file — a doc has no row to go quiet with — and
//! fenced code blocks are skipped there (a ULID in a fence is an example).

use crate::core;
use crate::find::branches::with_branches;
use std::path::Path;

/// The prose a row shows: its title (every list) plus its text (the body).
/// A citation in either is a dead citation to the reader.
fn prose(row: &core::Row) -> String {
    match row.title.as_deref() {
        Some(t) if !t.is_empty() => format!("{t} {}", row.text),
        _ => row.text.clone(),
    }
}

/// Every `[Phantom]` problem: dead citations in the log (rows and close
/// texts) and in markdown prose. Both halves share one branch escalation, and
/// both need candidates before any git spawn happens.
pub(super) fn problems(log: &core::Log, root: &Path) -> Vec<core::Problem> {
    // no rows and no closes: fael was never adopted here, or every row has
    // been pruned away — `doctor` already says so with [NoLog], and flagging
    // every id-shaped example left in the docs would only add noise
    if log.rows.is_empty() && log.closes.is_empty() {
        return vec![];
    }
    let rows = row_cands(log);
    let docs = core::phantom_md_refs(log, root);
    if rows.is_empty() && docs.is_empty() {
        return vec![];
    }
    // one branch escalation for all candidates; anything resolving on an
    // unmerged branch exists, so it is dropped, never reported
    let (wide, _) = with_branches(root, log.clone());
    let missing = |tok: &str| matches!(core::ref_state(&wide, tok), core::Ref::Missing);
    let rows: Vec<(String, String)> = rows.into_iter().filter(|(_, t)| missing(t)).collect();
    let docs: Vec<(String, usize, String)> =
        docs.into_iter().filter(|(_, _, t)| missing(t)).collect();
    let mut out = vec![];
    if !rows.is_empty() {
        out.push(row_problem(log, &rows));
    }
    if !docs.is_empty() {
        out.push(md_problem(&docs));
    }
    out
}

/// `(citing id, phantom token)` in scan order: open rows, then every close
/// text — unfolded closes, then the `closed.text` folded into compacted rows.
/// A closed row's own text is never scanned.
fn row_cands(log: &core::Log) -> Vec<(String, String)> {
    let mut cands: Vec<(String, String)> = vec![];
    for row in core::find(log, &core::Filter::default()) {
        for tok in core::phantom_refs(log, &prose(row)) {
            cands.push((row.id.clone(), tok));
        }
    }
    for c in &log.closes {
        for tok in core::phantom_refs(log, &c.text) {
            cands.push((c.id.clone(), tok));
        }
    }
    for row in &log.rows {
        let folded = row
            .extra
            .get("closed")
            .and_then(|v| v.get("text"))
            .and_then(|v| v.as_str());
        if let Some(text) = folded {
            for tok in core::phantom_refs(log, text) {
                cands.push((row.id.clone(), tok));
            }
        }
    }
    cands
}

/// The log half: the citing row owns the citation, so `ids` carries it —
/// a cleanup agent can `fael close` it without re-deriving the detector.
fn row_problem(log: &core::Log, cands: &[(String, String)]) -> core::Problem {
    let w = core::abbrev(log);
    let eg: Vec<String> = cands
        .iter()
        .take(5)
        .map(|(id, tok)| format!("{} → {tok}", w.short(id)))
        .collect();
    core::Problem::info(
        core::ProblemKind::Phantom,
        format!(
            "{} reference(s) to ids with no row — check the citation, then re-file \
             with --supersedes (e.g. {}) · compact --prune can remove closed rows, \
             so an old citation may point at a pruned row · {CROSS_REPO}",
            cands.len(),
            eg.join("; ")
        ),
    )
    .with_ids(unique_ids(cands))
}

/// This doctor reads only this repo's log, so an id from another repo's log
/// reads as dead — and ids change on every supersede anyway. A key follows
/// the row wherever it lives.
const CROSS_REPO: &str = "an id from another repo's log? cite its key instead \
    (`fael find --key <key>` in that repo), which also survives a supersede";

/// The markdown half: no row owns the citation, so `ids` stays empty (there
/// is nothing to close) — the file and line are what to fix, and they are in
/// the example, exactly where the reader lands.
fn md_problem(refs: &[(String, usize, String)]) -> core::Problem {
    let eg: Vec<String> = refs
        .iter()
        .take(5)
        .map(|(p, n, t)| format!("{p}:{n} → {t}"))
        .collect();
    core::Problem::info(
        core::ProblemKind::Phantom,
        format!(
            "{} reference(s) to ids with no row in markdown — fix the citation where it is \
             written (e.g. {}) · compact --prune can remove closed rows, so an old citation \
             may point at a pruned row · {CROSS_REPO}",
            refs.len(),
            eg.join("; ")
        ),
    )
}

fn unique_ids(cands: &[(String, String)]) -> Vec<String> {
    let mut ids: Vec<String> = vec![];
    for (id, _) in cands {
        if !ids.contains(id) {
            ids.push(id.clone());
        }
    }
    ids
}
