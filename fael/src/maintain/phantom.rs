//! `[Phantom]` (PLAN-fael-id-refs chunk 3): open rows — and close reasons —
//! citing an id-shaped token with no row behind it. Split out of `rows.rs`
//! next to `fat.rs`/`orphan.rs` so `files_notes` stays under the 100-line
//! function cap. Union-scope candidates come from core (`phantom_refs`);
//! the one branch escalation lives here, never in core (core spawns no
//! processes) — `doctor` already spawns `gh`, so one git spawn is fine.
//!
//! Only close *texts* and open rows are scanned: a closed row's own text
//! went quiet with the row, but its close reason still speaks for it.

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

/// The `[Phantom]` doctor problem, if any open row or close text cites an
/// id with no row behind it — kept here (not in `rows.rs`) so `files_notes`
/// stays under the 100-line function cap.
pub(super) fn problem(log: &core::Log, root: &Path) -> Option<core::Problem> {
    // `(citing id, phantom token)` in scan order: open rows, then every
    // close text — unfolded closes, then the `closed.text` folded into
    // compacted rows. A closed row's own text is never scanned.
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
    if cands.is_empty() {
        return None;
    }
    // one branch escalation for all candidates; anything resolving on an
    // unmerged branch exists, so it is dropped, never reported
    let (wide, _) = with_branches(root, log.clone());
    cands.retain(|(_, tok)| matches!(core::ref_state(&wide, tok), core::Ref::Missing));
    if cands.is_empty() {
        return None;
    }
    let w = core::abbrev(log);
    let eg: Vec<String> = cands
        .iter()
        .take(5)
        .map(|(id, tok)| format!("{} → {tok}", w.short(id)))
        .collect();
    let mut ids: Vec<String> = vec![];
    for (id, _) in &cands {
        if !ids.contains(id) {
            ids.push(id.clone());
        }
    }
    Some(
        core::Problem::info(
            core::ProblemKind::Phantom,
            format!(
                "{} reference(s) to ids with no row — check the citation, then re-file \
                 with --supersedes (e.g. {}) · compact --prune can remove closed rows, \
                 so an old citation may point at a pruned row",
                cands.len(),
                eg.join("; ")
            ),
        )
        .with_ids(ids),
    )
}
