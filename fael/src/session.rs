//! Session resolution for the write path — which hook session is filing,
//! and what it edited. Split out of write.rs (file-size ratchet): no row
//! logic here, only evidence about the caller.

use crate::{core, hook};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// Set once for `fael mcp`: process env belongs to the server's launcher,
/// not its callers. Even a recorded edit cannot prove who made a request.
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
    if mcp_server() {
        return String::new();
    }
    for k in ["FAEL_SESSION", "CLAUDE_CODE_SESSION_ID", "CODEX_THREAD_ID"] {
        let v = std::env::var(k).unwrap_or_default();
        if !v.is_empty() {
            return v;
        }
    }
    String::new()
}

/// The client behind a CLI call, by the same env keys `env_session` reads and
/// nothing else: `FAEL_SESSION` is OpenCode's, then Claude's, then Codex's;
/// none = a plain `cli`.
pub(crate) fn env_client() -> &'static str {
    if mcp_server() {
        return "cli";
    }
    client_of(|k| std::env::var(k).unwrap_or_default())
}

fn client_of(get: impl Fn(&str) -> String) -> &'static str {
    [
        ("FAEL_SESSION", "opencode"),
        ("CLAUDE_CODE_SESSION_ID", "claude"),
        ("CODEX_THREAD_ID", "codex"),
    ]
    .into_iter()
    .find(|(k, _)| !get(k).is_empty())
    .map_or("cli", |(_, c)| c)
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
/// so no local path ever lands in a row. `None` outside
/// any hook session.
///
/// Trust is split (01M47N67): outside the long-lived `fael mcp` server,
/// `FAEL_SESSION` is set per command by the OpenCode `shell.env` hook, so it
/// is the calling session and stamps raw; the client vars travel further, so
/// an id no hook event ever recorded (an edit, or session-start in this
/// worktree) stamps nothing. The MCP server's env is inherited at spawn,
/// never per call — so it supplies no caller identity, even after an edit.
pub(crate) fn writer_session(root: &Path) -> Option<String> {
    let env = env_session();
    if env.is_empty() {
        return None;
    }
    let per_call = !mcp_server() && std::env::var("FAEL_SESSION").is_ok_and(|v| !v.is_empty());
    let s = match recorded(root, &env) {
        Some(rec) => rec,
        None if per_call => env,
        // session-start saw this id here: a row filed before the first edit.
        // Never for the MCP server — its env may name a session it outlived,
        // and a session-start makes that id look live (01M47N67).
        None if !mcp_server() && hook::started_path(&env, root).exists() => env,
        None => return None,
    };
    let id = match s.contains(['/', '\\']) {
        true => Path::new(&s).file_stem()?.to_string_lossy().into_owned(),
        false => s,
    };
    (!id.is_empty()).then_some(id)
}

/// Record the writer session on a row about to be filed (no-op outside a session).
/// A retire — a close (`ref`), a bump (`bumps`), a superseder (`supersedes`)
/// — also stamps where it was shown (`extra.shown`): the sorted unique
/// sources from this session's shown file that named the target. Absent when
/// none. Fail open on any IO error, never blocks a write.
pub(crate) fn tag_writer(root: &Path, row: &mut core::Row) {
    let session = writer_session(root);
    row.extra
        .extend(session.clone().map(|s| ("session".into(), s.into())));
    let target = row
        .reference
        .as_deref()
        .or(row.bumps.as_deref())
        .or(row.supersedes.as_deref());
    if let (Some(s), Some(t)) = (session.as_deref(), target) {
        let shown = crate::hook::held_shown(s, root, t);
        if !shown.is_empty() {
            row.extra.insert(
                "shown".into(),
                shown.into_iter().map(serde_json::Value::from).collect(),
            );
        }
    }
}

/// Fill a `Stamp` for a retire of `target`: the writer session and its shown
/// sources for that id. No session = no stamp; no sources = session only.
/// Fail open, never blocks.
pub(crate) fn tag_stamp(root: &Path, target: &str, stamp: &mut core::Stamp) {
    let Some(s) = writer_session(root) else {
        return;
    };
    stamp.session = Some(s.clone());
    let shown = crate::hook::held_shown(&s, root, target);
    stamp.shown = (!shown.is_empty()).then_some(shown);
}

/// A retire `Stamp` for `id`: the base git stamp plus the writer session and
/// its shown sources. Unresolvable id = base stamp as is, fail open.
pub(crate) fn retire_stamp(r: &crate::Repo, log: &core::Log, id: &str) -> core::Stamp {
    let mut stamp = crate::stamp(r);
    if let Ok(full) = core::resolve_row(log, id) {
        tag_stamp(&r.root, &full.id, &mut stamp);
    }
    stamp
}

/// Stamp a superseder filed now: resolve its target, set it, tag writer +
/// shown. No-op when none — the first `tag_writer` in `prepare` ran before
/// heal knew the target.
pub(crate) fn tag_superseder(
    root: &Path,
    log: &core::Log,
    row: &mut core::Row,
    supersedes: Option<&str>,
) {
    if let Some(s) = supersedes
        && let Ok(full) = core::resolve_row(log, s)
    {
        row.supersedes = Some(full.id.clone());
        tag_writer(root, row);
    }
}

/// A session stays usable for deriving files while its edit file was written
/// recently — the plan's guess is 2 h.
const ACTIVE_SECS: u64 = 2 * 60 * 60;

/// Each active session file in this worktree with its edits, oldest file
/// first. Lines without a worktree predate it and are kept (they age out with
/// the 2 h window); lines naming another worktree are dropped — without this
/// a row filed in repo A would inherit files touched in repo B.
fn active_sessions(root: &Path) -> Vec<Vec<hook::Edit>> {
    // ponytail: memoized per process (several callers per `find`); the MCP
    // server is long-lived so it never memoizes, and `record_edits` drops it.
    let key = (root.to_path_buf(), hook::state_dir());
    if !mcp_server()
        && let Some(v) = SESSIONS_MEMO.with(|m| {
            m.borrow()
                .as_ref()
                .filter(|(k, _)| *k == key)
                .map(|(_, v)| v.clone())
        })
    {
        return v;
    }
    let v = scan_sessions(root);
    if !mcp_server() {
        SESSIONS_MEMO.with(|m| *m.borrow_mut() = Some((key, v.clone())));
    }
    v
}

type Memo = ((PathBuf, PathBuf), Vec<Vec<hook::Edit>>);

thread_local! {
    static SESSIONS_MEMO: std::cell::RefCell<Option<Memo>> =
        const { std::cell::RefCell::new(None) };
}

/// Drop the memo — an edit was just appended.
pub(crate) fn forget_sessions() {
    SESSIONS_MEMO.with(|m| *m.borrow_mut() = None);
}

fn scan_sessions(root: &Path) -> Vec<Vec<hook::Edit>> {
    let dir = hook::state_dir().join("sessions").join("edits");
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
// The cutoff is the caller's newest row, ignoring rows stamped with another
// session (01M67ET2); unstamped rows and a caller without an id still count.
pub(crate) fn derive(root: &Path, log: &core::Log) -> Vec<String> {
    // No per-request session binding exists in the stdio protocol. A sole
    // active editor may be a different chat, so MCP must name files explicitly.
    if mcp_server() {
        return vec![];
    }
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
    let last = log
        .rows
        .iter()
        .chain(log.closes.iter())
        .filter(|r| {
            let other = r.extra.get("session").and_then(|v| v.as_str());
            id.is_empty() || other.is_none_or(|s| s == id)
        })
        .filter_map(|r| core::ts_ms(&r.ts))
        .max();
    let mut seen = HashSet::new();
    let mut out = vec![];
    for (path, at, ..) in mine.unwrap_or_default() {
        if last.is_none_or(|r| at > r) && seen.insert(path.clone()) {
            out.push(path);
        }
    }
    out
}

#[cfg(test)]
mod client_tests {
    use super::client_of;

    #[test]
    fn client_follows_the_env_key() {
        let env = |set: &'static [&'static str]| {
            move |k: &str| {
                if set.contains(&k) {
                    "x".to_string()
                } else {
                    String::new()
                }
            }
        };
        assert_eq!(client_of(env(&["FAEL_SESSION"])), "opencode");
        assert_eq!(client_of(env(&["CLAUDE_CODE_SESSION_ID"])), "claude");
        assert_eq!(client_of(env(&["CODEX_THREAD_ID"])), "codex");
        assert_eq!(client_of(env(&[])), "cli");
        // opencode inside a Claude shell: the fresher signal wins, as in env_session
        assert_eq!(
            client_of(env(&["FAEL_SESSION", "CLAUDE_CODE_SESSION_ID"])),
            "opencode"
        );
    }
}
