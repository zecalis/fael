//! Where a missing id lives (issue 01M4XKM4): a chunk in this repo's crew.db, or a row or
//! chunk in another repo on this machine — the registry and every repo usage names.
//! Read only on the reject path: an agent that was told "no row" hunted repo by repo.
//!
//! Thin over `fael-core::work::elsewhere` — the scan itself lives there so
//! `fael` and `fael-crew` cannot drift; this file keeps only the message
//! wording and the chunk-here brief.

use crate::{Repo, core};
use std::path::PathBuf;

pub(crate) use core::work::Elsewhere;

/// The unique owner of `tok` outside `r`, if there is exactly one.
pub(crate) fn row_elsewhere(r: &Repo, tok: &str) -> Option<Elsewhere> {
    core::work::row_elsewhere(r, tok)
}

/// The repo root holding `tok` as a chunk, if exactly one does.
pub(crate) fn chunk_elsewhere(r: &Repo, tok: &str) -> Option<PathBuf> {
    core::work::chunk_elsewhere(r, tok)
}

/// The line naming `tok`'s owner, if it is anything but missing everywhere.
pub(crate) fn owner(r: &Repo, tok: &str) -> Option<String> {
    if is_chunk(r, tok) {
        return Some(format!(
            "{tok} is a chunk here — `fael chunk show {tok}` reads it"
        ));
    }
    // one hit is the row (ULIDs never collide); two holders of a prefix stay
    // a reject — a prefix is an address, not a name.
    if let Some(w) = row_elsewhere(r, tok) {
        let at = w.at.display();
        return Some(format!(
            "{tok} is a ledger {} in {at} — `cd {at} && fael find {tok}` (MCP: {{\"cwd\": \"{at}\"}})",
            w.kind
        ));
    }
    if let Some(at) = chunk_elsewhere(r, tok) {
        let at = at.display();
        return Some(format!(
            "{tok} is a chunk in {at} — `cd {at} && fael chunk show {tok}` (MCP: {{\"cwd\": \"{at}\"}})"
        ));
    }
    None
}

fn is_chunk(r: &Repo, tok: &str) -> bool {
    let db = core::work::db_for(r.journal.as_deref(), &r.fael);
    core::work::is_chunk(&db, tok)
}

/// A chunk uid of this repo passed to find: its brief, as MCP `chunk show` prints it. The
/// "is a chunk here" hint alone still cost agents a round (opencode, 2026-10-17).
pub(crate) fn chunk_here(r: &Repo, tok: &str) -> Option<String> {
    let db = core::work::db_for(r.journal.as_deref(), &r.fael);
    core::work::chunk_brief(&db, tok).map(|b| brief(&b))
}

/// The chunk-show text: title, uid, plan, brief.
pub(crate) fn brief(b: &core::work::Brief) -> String {
    format!(
        "# {} ({})\nplan {} — {}\n\n{}\n",
        b.title, b.uid, b.plan, b.plan_title, b.brief
    )
}
