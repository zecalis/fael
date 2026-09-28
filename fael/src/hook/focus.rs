//! L2 Focus (PLAN-fael-push-focus chunk 2): built once at session start —
//! git is allowed there — and written beside the seen file. The read/edit
//! push only reads it: no git spawn on the 5 ms path, and a missing or
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

/// Build the session Focus (start branch + the keys of the open rows filed on
/// it) and write it. An empty session has no key to write under — MCP and
/// CLI calls outside a hook session build nothing — and a detached HEAD
/// builds `Focus::default()`, which reads back the same as no file at all.
pub(crate) fn write(session: &str, root: &Path, branch: Option<&str>, rows: &[&core::Row]) {
    if session.is_empty() {
        return;
    }
    let focus = core::Focus::from_rows(branch, rows);
    let Ok(body) = serde_json::to_string(&focus) else {
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

/// The Focus a push ranks with. No session, no file, or a file written by a
/// future/older fael: `Focus::default()` — today's order, only the row cap.
/// Never spawns git: one small read on the push path.
pub(crate) fn read(session: &str, root: &Path) -> core::Focus {
    if session.is_empty() {
        return core::Focus::default();
    }
    std::fs::read_to_string(path(session, root))
        .ok()
        .and_then(|body| serde_json::from_str(&body).ok())
        .unwrap_or_default()
}
