//! L2 Focus (PLAN-fael-push-focus chunk 2): built at session start — git is
//! allowed there — and written beside the seen file. The read/edit push reads
//! it back and rebuilds it only when HEAD moved: no git spawn on the 5 ms path, and a missing or
//! unparsable file falls back to `Focus::default()` (today's order, only the
//! row cap). Core owns the shape and the pure build (`Focus::from_rows`);
//! this module is the file.

use super::state::{session_key, state_dir};
use crate::core;
use std::path::{Path, PathBuf};

/// `<state>/sessions/<session+worktree>.focus.json` — the same key the seen
/// file uses, so one session's Focus follows it across repos and no two
/// sessions share one.
pub(crate) fn path(session: &str, root: &Path) -> PathBuf {
    let key = session_key(&format!("{session}\0{}", root.to_string_lossy()));
    state_dir()
        .join("sessions")
        .join(format!("{key}.focus.json"))
}

/// Write the session Focus (`core::Focus::from_rows`: start branch and the
/// keys of the open rows filed on it). An empty session has no key to write
/// under — MCP and CLI calls outside a hook session write nothing — and a
/// detached HEAD's `Focus::default()` reads back the same as no file at all.
pub(crate) fn write(session: &str, root: &Path, focus: &core::Focus) {
    if session.is_empty() {
        return;
    }
    let Ok(body) = serde_json::to_string(focus) else {
        return;
    };
    let path = path(session, root);
    if path
        .parent()
        .is_some_and(|p| std::fs::create_dir_all(p).is_ok())
    {
        let _ = std::fs::write(path, body);
    }
}

/// The Focus a push ranks with, kept on the checked-out branch: when
/// `<gitdir>/HEAD` (a file read, no git spawn) names another branch than the
/// stored Focus — another session switched this worktree mid-session — it is
/// rebuilt from the open rows and written back. Only ranking follows HEAD:
/// no warning (the branch-drift warning stays removed, 01M3SQ8AD). No
/// session, no file, or a file a future/older fael wrote: `Focus::default()`
/// — today's order, only the row cap.
pub(crate) fn current(session: &str, root: &Path, log: &core::Log) -> core::Focus {
    let Some(stored) = read(session, root) else {
        return core::Focus::default();
    };
    let head = crate::journal::work_branch(root);
    if stored.branch == head {
        return stored;
    }
    let all = core::find(log, &core::Filter::default());
    let f = core::Focus::from_rows(head.as_deref(), &all);
    write(session, root, &f);
    f
}

fn read(session: &str, root: &Path) -> Option<core::Focus> {
    if session.is_empty() {
        return None;
    }
    let body = std::fs::read_to_string(path(session, root)).ok()?;
    serde_json::from_str(&body).ok()
}
