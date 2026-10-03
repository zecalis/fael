//! Session resolution for the write path — which hook session is filing,
//! and what it edited. Split out of write.rs (file-size ratchet): no row
//! logic here, only evidence about the caller.

use crate::{core, hook};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Chunk 6e: the hook session string behind this call — the same key the push
/// reads. Resolved like `derive()`: the recorded session equal to the env
/// session or its stem; else the raw value (clients that key
/// by it directly); empty = outside any hook session, seen-ids stay off.
///
/// The env is `$CLAUDE_CODE_SESSION_ID` first, then `$FAEL_SESSION` — the
/// OpenCode plugin's `shell.env` hook sets the latter to the same RFC 3339
/// string the usage lines use, because OpenCode exports no session id to the
/// shell itself. A local MCP server is spawned once per OpenCode instance, so
/// MCP `add` calls still land outside any session.
fn env_session() -> String {
    for k in ["CLAUDE_CODE_SESSION_ID", "FAEL_SESSION"] {
        let v = std::env::var(k).unwrap_or_default();
        if !v.is_empty() {
            return v;
        }
    }
    String::new()
}

pub(crate) fn hook_session(root: &Path) -> String {
    let env = env_session();
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

/// The id a row records as its writer session: `hook_session` cut to the
/// transcript's file stem — the UUID Claude Code puts in `$CLAUDE_CODE_SESSION_ID`,
/// so no local path ever lands in a row that syncs to the team. `None` outside
/// any hook session.
fn writer_session(root: &Path) -> Option<String> {
    let s = hook_session(root);
    let id = match s.contains(['/', '\\']) {
        true => Path::new(&s).file_stem()?.to_string_lossy().into_owned(),
        false => s,
    };
    (!id.is_empty()).then_some(id)
}

/// Record the writer session on a row about to be filed (no-op outside a session).
pub(crate) fn tag_writer(root: &Path, row: &mut core::Row) {
    let session = writer_session(root);
    row.extra
        .extend(session.map(|s| ("session".into(), s.into())));
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
/// newest row, order kept, deduped. The caller's session is the hook env
/// (`CLAUDE_CODE_SESSION_ID`, else `FAEL_SESSION` from the OpenCode
/// `shell.env` hook) — the hook keys Claude by transcript path, so an
/// edit line matches on that file's stem too; without it, only a single
/// active session counts — two agents in one checkout must never file rows on
/// each other's files. Empty = the caller keeps the old "files is required"
/// error, so behaviour without a hook session is unchanged.
// ponytail: the cutoff is the newest row by anyone (as the stop hook does) —
// another agent's row can hide older edits, which fails toward "files is
// required", never toward wrong files. Rows would need a session to do better.
pub(crate) fn derive(root: &Path, log: &core::Log) -> Vec<String> {
    let mut sessions = active_sessions(root);
    let id = env_session();
    let mine = if !id.is_empty() {
        sessions.into_iter().find(|e| {
            e.iter().any(|(.., s)| {
                s.as_deref()
                    .is_some_and(|s| s == id || Path::new(s).file_stem().is_some_and(|f| *f == *id))
            })
        })
    } else if sessions.len() == 1 {
        sessions.pop()
    } else {
        None
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
