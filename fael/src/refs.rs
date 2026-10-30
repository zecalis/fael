//! Id-reference lookups (PLAN-fael-id-refs §2) over the union log (tree +
//! journal, what `crate::read` returns). No git spawn: every row a clone can
//! see is in its journal (or imported into it), never only on another branch.
//!
//! Identity itself is `core::resolve_identity` (plan fael-secretary c2) —
//! this module keeps only `phantoms`, the write-path citation scan over that
//! single lookup. The old owned `Wide`/`resolve_wide` wrapper is gone: CLI
//! find/find-many and MCP find call the canonical resolver directly.

use super::Repo;
use fael_core::{self as core};

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
        if matches!(core::resolve_identity(log, tok), core::Ref::Missing) {
            missing.push(tok);
        }
    }
    if missing.is_empty() {
        return vec![];
    }
    // a chunk uid has the same ULID shape: citing the chunk a row is about is no phantom
    let db = core::work::db_for(r.journal.as_deref(), &r.fael);
    missing
        .into_iter()
        .filter(|tok| !core::work::is_chunk(&db, tok))
        .map(String::from)
        .collect()
}
