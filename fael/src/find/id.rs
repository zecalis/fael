//! Identity lookup for `find <id>` (F-IDENT, c2): one input → one resolver →
//! one canonical row id or a typed miss. Split out of `find.rs` at the
//! 400-line ratchet — no logic of its own beyond the `resolve_identity`
//! match, so CLI and MCP share the one lookup without a fallback.

use super::{owner, reject_many, reject_missing, show_found};
use crate::{Args, core};

/// `Some` = answered (hit or typed miss); `None` = non-id-shaped `Missing`,
/// the caller tries key/text next. Legacy non-ULID ids resolve here too —
/// no `looks_like_id` gate on the lookup itself, the gate only picks the
/// miss wording.
pub(crate) fn lookup(
    a: &Args,
    r: &crate::Repo,
    log: &core::Log,
    branch_of: &crate::journal::BranchMap,
    t: &str,
) -> Option<Result<(), String>> {
    match core::resolve_identity(log, t) {
        core::Ref::One(row) => {
            // the body was just printed — the next push must not say it again
            crate::hook::note_seen(
                &crate::session::hook_session(&r.root),
                &r.root,
                &[row.id.as_str()],
            );
            Some(show_found(a, &r.root, log, row, branch_of))
        }
        core::Ref::Many(rows) => {
            let owned: Vec<core::Row> = rows.into_iter().cloned().collect();
            Some(Err(reject_many(t, &owned)))
        }
        // --json is the machine shape: a chunk there stays the reject
        core::Ref::Missing if core::looks_like_id(t) => {
            match (!a.has("json")).then(|| owner::chunk_here(r, t)).flatten() {
                Some(c) => {
                    print!("{c}");
                    Some(Ok(()))
                }
                None => Some(Err(reject_missing(r, log, t))),
            }
        }
        core::Ref::Missing => None,
    }
}
