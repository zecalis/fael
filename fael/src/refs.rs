//! Id-reference escalation (PLAN-fael-id-refs §2): the union log (tree +
//! journal, what `crate::read` returns) answers first; unmerged branches from
//! other clones (`with_branches`) run only on a union miss — at most one
//! spawn per call, and never on the read/edit push path (callers are the
//! write path and `doctor`).
//!
//! Chunk-0 wires this with no callers — `fael` behaves identically; chunk-2
//! wires `phantoms` from the write path. The `allow(dead_code)` on
//! `resolve_wide` goes away when chunk-1 wires its caller.

use super::Repo;
use super::find::branches::{BranchMap, with_branches};
use fael_core::{self as core};

/// Owned union-then-branches answer. Owned rows — the merged `Log` moves
/// with the answer, so nothing borrows it (the plan's `Result<…>` sketch
/// cannot borrow from a moved log).
pub(crate) enum Wide {
    One(Box<core::Row>),
    Many(Vec<core::Row>),
    Missing,
}

/// `One`/`Many` straight from the union; only a union `Missing` pays for
/// one `with_branches` call and is re-checked against the merged log. The
/// branch map tags escalated rows on display (empty on a union hit).
pub(crate) fn resolve_wide(r: &Repo, log: core::Log, tok: &str) -> (core::Log, Wide, BranchMap) {
    if !matches!(core::ref_state(&log, tok), core::Ref::Missing) {
        let wide = wide_of(&log, tok);
        return (log, wide, BranchMap::new());
    }
    let (log, btags) = with_branches(&r.root, log);
    let wide = wide_of(&log, tok);
    (log, wide, btags)
}

fn wide_of(log: &core::Log, tok: &str) -> Wide {
    match core::ref_state(log, tok) {
        core::Ref::One(row) => Wide::One(Box::new(row.clone())),
        core::Ref::Many(rows) => Wide::Many(rows.into_iter().cloned().collect()),
        core::Ref::Missing => Wide::Missing,
    }
}

/// Id-shaped tokens in `text` that are `Missing` in the union AND in
/// unmerged branches — the phantom citations. `skip` holds full ids the
/// caller already resolved (the row being written, its supersede target):
/// a token they prefix-match is not a citation. One `with_branches` call
/// per call, only when the union pass found a `Missing`.
pub(crate) fn phantoms(r: &Repo, log: &core::Log, text: &str, skip: &[&str]) -> Vec<String> {
    let mut missing: Vec<&str> = vec![];
    for tok in core::id_tokens(text) {
        let skipped = skip.iter().any(|s| {
            s.get(..tok.len())
                .is_some_and(|p| p.eq_ignore_ascii_case(tok))
        });
        if skipped {
            continue;
        }
        if matches!(core::ref_state(log, tok), core::Ref::Missing) {
            missing.push(tok);
        }
    }
    if missing.is_empty() {
        return vec![];
    }
    let (wide, _) = with_branches(&r.root, log.clone());
    missing
        .into_iter()
        .filter(|tok| matches!(core::ref_state(&wide, tok), core::Ref::Missing))
        .map(String::from)
        .collect()
}
