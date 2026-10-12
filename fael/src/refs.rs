//! Id-reference lookups (PLAN-fael-id-refs §2) over the union log (tree +
//! journal, what `crate::read` returns). No git spawn: every row a clone can
//! see is in its journal (or synced into it), never only on another branch.

use super::Repo;
use fael_core::{self as core};

/// Owned answer — `One`/`Many`/`Missing`, cloned out of the log.
pub(crate) enum Wide {
    One(Box<core::Row>),
    Many(Vec<core::Row>),
    Missing,
}

pub(crate) fn resolve_wide(log: &core::Log, tok: &str) -> Wide {
    match core::ref_state(log, tok) {
        core::Ref::One(row) => Wide::One(Box::new(row.clone())),
        core::Ref::Many(rows) => Wide::Many(rows.into_iter().cloned().collect()),
        core::Ref::Missing => Wide::Missing,
    }
}

/// Id-shaped tokens in `text` that are `Missing` in the union — the phantom
/// citations. `skip` holds full ids the caller already resolved (the row
/// being written, its supersede target): a token they prefix-match is not a
/// citation.
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
    // a chunk uid has the same ULID shape: citing the chunk a row is about is no phantom
    let plans = Some(crate::plan::db(r))
        .filter(|p| p.exists())
        .and_then(|p| fael_core::plan::Store::open(&p).ok());
    missing
        .into_iter()
        .filter(|tok| !plans.as_ref().is_some_and(|s| s.is_chunk(tok)))
        .map(String::from)
        .collect()
}
