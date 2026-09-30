//! The read/edit push: rows about `files`, ranked so the most actionable comes
//! first. Moved out of select.rs (file-size ratchet) — no logic of its own.

use super::matching::{lenient, same_dir, zone};
use crate::{Aliases, Log, Row, is_alias_row, is_carrier_row};
use std::collections::HashSet;

/// The read/edit push: rows about `files`, ranked so the most actionable comes
/// first — urgent, then exact file, same directory, rows sharing a key with an
/// exact hit. Open `issue` before `decision` before the rest, freshest first
/// by row-or-mtime inside each. Closed and superseded rows never push. Each
/// query expands through `al` first, so a row filed under a path that was
/// renamed since still pushes at the new path — except through a file that was
/// split into several (the child a row is about is unknowable: fael never
/// guesses, `find --files <child>` still reaches them). The read/edit path never
/// computes reader identity (no git spawn there), so `to` does not reorder
/// the push — session start is where routing lists. Deterministic: the
/// same log and query give the same order on any machine. The hook push cuts
/// the tiered result to the row cap with `select`, then to the push budget
/// with `render`.
///
/// `no_same_dir` is the read/edit split (PLAN-fael-row-hygiene chunk 2): reads
/// pass true to drop the same-directory tier — the noisiest one, rows about
/// neighbouring files — and keep exact file, zone/glob and shared-key hits;
/// edits pass false to keep it, because a module-level decision matters most
/// while changing that module.
///
/// `push_tiered` is `push` plus L1's match tier per row (0 exact file/zone,
/// 1 same-dir, 2 shared key) — what L3 `bucket` ranks on. Same gather and
/// order as `push`; chunk 1 exposes the tier so `select` can keep same-dir
/// and shared-key rows in Background even under the row cap.
pub fn push_tiered<'a>(
    log: &'a Log,
    files: &[String],
    al: &Aliases,
    no_same_dir: bool,
) -> Vec<(&'a Row, usize)> {
    let hide: HashSet<&str> = super::closed(log)
        .union(&super::superseded(log))
        .copied()
        .collect();
    let queries: Vec<String> = al.expand_all_unsplit(
        &files
            .iter()
            .map(|q| lenient(q).trim_end_matches('/').to_string())
            .collect::<Vec<_>>(),
    );
    if queries.is_empty() {
        return vec![];
    }
    // keys of the exact hits — tier 2 shares one of these
    let mut hit_keys: HashSet<&str> = HashSet::new();
    for r in &log.rows {
        if hide.contains(r.id.as_str()) {
            continue;
        }
        let rf: Vec<String> = r.files.iter().map(|f| lenient(f)).collect();
        if queries.iter().any(|q| rf.iter().any(|f| zone(q, f))) {
            hit_keys.extend(r.key.as_deref());
        }
    }
    let tier = |r: &Row| {
        let rf: Vec<String> = r.files.iter().map(|f| lenient(f)).collect();
        if queries.iter().any(|q| rf.iter().any(|f| zone(q, f))) {
            return 0;
        }
        if !no_same_dir && queries.iter().any(|q| rf.iter().any(|f| same_dir(q, f))) {
            return 1;
        }
        if r.key.as_deref().is_some_and(|k| hit_keys.contains(k)) {
            return 2;
        }
        3
    };
    let out: Vec<&Row> = log
        .rows
        .iter()
        .filter(|r| {
            !hide.contains(r.id.as_str()) && !is_alias_row(r) && !is_carrier_row(r) && tier(r) < 3
        })
        .collect();
    let mut keyed: Vec<(&Row, usize, i64)> = out
        .into_iter()
        .map(|r| (r, tier(r), super::fresh_ts(r)))
        .collect();
    keyed.sort_by(|a, b| super::cmp_rows(a.0, b.0, None, a.1, b.1, a.2, b.2));
    keyed.into_iter().map(|(r, t, _)| (r, t)).collect()
}

/// The read/edit push: same rows as `push_tiered`, tiers dropped. Kept for
/// callers that only render (find-shaped paths); the hook push uses
/// `push_tiered` + `select` so the row cap applies.
pub fn push<'a>(log: &'a Log, files: &[String], al: &Aliases, no_same_dir: bool) -> Vec<&'a Row> {
    push_tiered(log, files, al, no_same_dir)
        .into_iter()
        .map(|(r, _)| r)
        .collect()
}
