//! Session resolution for the write path — which hook session is filing,
//! and what it edited. Split out of write.rs (file-size ratchet): no row
//! logic here, only evidence about the caller.

use crate::{core, hook};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Chunk 6e: the hook session string behind this call — the same key the push
/// reads. Resolved like `derive()`: the recorded session equal to
/// `$CLAUDE_CODE_SESSION_ID` or its stem; else the raw value (clients that key
/// by it directly); empty = outside any hook session, seen-ids stay off.
pub(crate) fn hook_session(root: &Path) -> String {
    let env = std::env::var("CLAUDE_CODE_SESSION_ID").unwrap_or_default();
    if env.is_empty() {
        return String::new();
    }
    for s in active_sessions(root).into_iter().flatten() {
        if let Some(rec) = s.3
            && (rec == env || Path::new(&rec).file_stem().is_some_and(|f| *f == *env))
        {
            return rec;
        }
    }
    env
}

/// A session stays usable for deriving files while its edit file was written
/// recently — the plan's guess is 2 h.
const ACTIVE_SECS: u64 = 2 * 60 * 60;

/// Each active session file in this worktree with its edits, oldest file
/// first. Lines without a worktree predate it and are kept (they age out with
/// the 2 h window); lines naming another worktree are dropped — without this
/// a row filed in repo A would inherit files touched in repo B.
fn active_sessions(root: &Path) -> Vec<Vec<hook::Edit>> {
    let dir = hook::state_dir().join("sessions");
    let Ok(rd) = std::fs::read_dir(&dir) else {
        return vec![];
    };
    let here = root.to_string_lossy();
    let mut files: Vec<(u64, PathBuf)> = vec![];
    for e in rd.flatten() {
        let p = e.path();
        if p.extension().is_none_or(|x| x != "jsonl") {
            continue; // `.seen`, `.tmp`, …
        }
        let age = e
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok().map(|d| d.as_secs()));
        // an unreadable mtime fails toward inclusion — an old file only
        // contributes edits newer than the last row anyway
        if age.is_none_or(|s| s < ACTIVE_SECS) {
            files.push((age.unwrap_or(0), p));
        }
    }
    // oldest session file first (largest age), so the union reads in edit order
    files.sort_by_key(|f| std::cmp::Reverse(f.0));
    files
        .into_iter()
        .filter_map(|(_, p)| {
            let edits: Vec<_> = hook::session_edits(&p)
                .into_iter()
                .filter(|(_, _, w, _)| w.as_deref().is_none_or(|w| w == here))
                .collect();
            (!edits.is_empty()).then_some(edits)
        })
        .collect()
}

/// Every edit any active session recorded in this worktree — evidence for
/// `check`, where another session's edit only widens what passes.
pub(crate) fn active_edits(root: &Path) -> Vec<(String, i64)> {
    active_sessions(root)
        .into_iter()
        .flatten()
        .map(|(path, at, ..)| (path, at))
        .collect()
}

/// Files for a row filed now: the caller's own session edits newer than the
/// newest row, order kept, deduped. The caller's session is
/// `CLAUDE_CODE_SESSION_ID` — the hook keys Claude by transcript path, so an
/// edit line matches on that file's stem too; without it, only a single
/// active session counts — two agents in one checkout must never file rows on
/// each other's files. Empty = the caller keeps the old "files is required"
/// error, so behaviour without a hook session is unchanged.
// ponytail: the cutoff is the newest row by anyone (as the stop hook does) —
// another agent's row can hide older edits, which fails toward "files is
// required", never toward wrong files. Rows would need a session to do better.
pub(crate) fn derive(root: &Path, log: &core::Log) -> Vec<String> {
    let mut sessions = active_sessions(root);
    let mine = match std::env::var("CLAUDE_CODE_SESSION_ID") {
        Ok(id) if !id.is_empty() => sessions.into_iter().find(|e| {
            e.iter().any(|(.., s)| {
                s.as_deref()
                    .is_some_and(|s| s == id || Path::new(s).file_stem().is_some_and(|f| *f == *id))
            })
        }),
        _ if sessions.len() == 1 => sessions.pop(),
        _ => None,
    };
    let last = core::last_row_ms(log, 0);
    let mut seen = HashSet::new();
    let mut out = vec![];
    for (path, at, ..) in mine.unwrap_or_default() {
        if last.is_none_or(|r| at > r) && seen.insert(path.clone()) {
            out.push(path);
        }
    }
    out
}
