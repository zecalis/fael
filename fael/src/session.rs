//! Session resolution for the write path — which hook session is filing,
//! and what it edited. Split out of write.rs (file-size ratchet): no row
//! logic here, only evidence about the caller.

use crate::{core, hook};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// Set once for the long-lived `fael mcp` server (`mcp::serve`): its env is
/// inherited at spawn, not per call, so `FAEL_SESSION` there is no fresher
/// than a client var — it must not be trusted raw (01M47N67).
static MCP_SERVER: AtomicBool = AtomicBool::new(false);

/// Called by `mcp::serve` at startup, so every `add` the server handles knows
/// its env is inherited, never per-command.
pub(crate) fn mark_mcp_server() {
    MCP_SERVER.store(true, Ordering::Relaxed);
}

fn mcp_server() -> bool {
    MCP_SERVER.load(Ordering::Relaxed)
}

/// Chunk 6e: the hook session string behind this call — the same key the push
/// reads. Resolved like `derive()`: the recorded session equal to the env
/// session or its stem; else the raw value (clients that key
/// by it directly); empty = outside any hook session, seen-ids stay off.
///
/// The raw fallback matters before the session's first recorded edit: an `add`
/// only knows the env id while the hook keys by transcript path, and the two
/// meet through the stem. A stranger's id in a per-session file is harmless —
/// nothing real ever reads it — so this stays lenient; row stamps do not
/// (see `writer_session`).
///
/// The env is `$FAEL_SESSION` first, then `$CLAUDE_CODE_SESSION_ID`, then
/// `$CODEX_THREAD_ID` — the OpenCode plugin's `shell.env` hook sets the former
/// inside OpenCode's own shell commands only, so it is the fresher signal; the
/// Claude one can be inherited when opencode runs inside a Claude Code shell.
/// Codex exposes `CODEX_THREAD_ID` to its shell tool executions (but not to
/// stdio MCP servers: openai/codex#19937), so a `fael add` from a Codex shell
/// joins the same way.
fn env_session() -> String {
    for k in ["FAEL_SESSION", "CLAUDE_CODE_SESSION_ID", "CODEX_THREAD_ID"] {
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
    recorded(root, &env).unwrap_or(env)
}

/// The recorded session matching `env` (equal or transcript stem), if any
/// hook event ever ran under it in this worktree.
fn recorded(root: &Path, env: &str) -> Option<String> {
    active_sessions(root).into_iter().flatten().find_map(|s| {
        s.3.filter(|rec| {
            rec.as_str() == env || Path::new(rec).file_stem().is_some_and(|f| *f == *env)
        })
    })
}

/// The id a row records as its writer session: `hook_session` cut to the
/// transcript's file stem — the UUID Claude Code puts in `$CLAUDE_CODE_SESSION_ID`,
/// so no local path ever lands in a row that syncs to the team. `None` outside
/// any hook session.
///
/// Trust is split (01M47N67): outside the long-lived `fael mcp` server,
/// `FAEL_SESSION` is set per command by the OpenCode `shell.env` hook, so it
/// is the calling session and stamps raw; the client vars travel further, so
/// an id no hook event ever recorded (an edit, or session-start in this
/// worktree) stamps nothing. The MCP server's env is
/// inherited at spawn, never per call — so there *every* id must be recorded.
fn writer_session(root: &Path) -> Option<String> {
    let env = env_session();
    if env.is_empty() {
        return None;
    }
    let per_call = !mcp_server() && std::env::var("FAEL_SESSION").is_ok_and(|v| !v.is_empty());
    let s = match recorded(root, &env) {
        Some(rec) => rec,
        // session-start saw this id here: a row filed before the first edit
        None if per_call || hook::started_path(&env, root).exists() => env,
        None => return None,
    };
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
/// (`FAEL_SESSION` from the OpenCode `shell.env` hook, `CLAUDE_CODE_SESSION_ID`,
/// or `CODEX_THREAD_ID` from a Codex shell) — the hook keys Claude by transcript path, so an
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
