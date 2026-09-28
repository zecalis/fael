//! L2 Focus (PLAN-fael-push-focus chunk 2): built once at session start —
//! git is allowed there — and written beside the seen file. The read/edit
//! push only reads it: no git spawn on the 5 ms path, and a missing or
//! unparsable file falls back to `Focus::default()` (today's order, only the
//! row cap). Core owns the shape and the pure build (`Focus::from_rows`,
//! `open_plans` + `resolve_plan` for the plan); this module is the file — and
//! the active plan's path, the one other thing session start may read from
//! disk (`Config::plan_dirs`).

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

/// Build the session Focus (start branch, the keys of the open rows filed on
/// it, and the resolved active plan) and write it. An empty session has no key
/// to write under — MCP and CLI calls outside a hook session build nothing —
/// and a detached HEAD builds `Focus::default()`, which reads back the same
/// as no file at all. Returns what was built, so session-start can say the
/// active plan line from the same Focus the push ranks with.
pub(crate) fn write(
    session: &str,
    root: &Path,
    branch: Option<&str>,
    rows: &[&core::Row],
) -> core::Focus {
    let facts = core::open_plans(rows);
    let mut focus = core::Focus::from_rows(branch, rows);
    // chunk 1 passes no intent — declared intent arrives in chunk 2
    focus.plan = core::resolve_plan(&facts, branch, None);
    if session.is_empty() {
        return focus;
    }
    let Ok(body) = serde_json::to_string(&focus) else {
        return focus;
    };
    let path = path(session, root);
    if path
        .parent()
        .is_some_and(|p| std::fs::create_dir_all(p).is_ok())
    {
        let _ = std::fs::write(path, body);
    }
    focus
}

/// The active plan file, as the agent can run it from the repo root: the
/// first `<dir>/PLAN-<name>.md` under `Config::plan_dirs` that exists.
/// Relative dirs keep their spelling (`.fapony/plan/PLAN-x.md`), absolute
/// ones their own. Files are read here, at session start — the push never
/// resolves a path, it only reads the Focus this wrote.
pub(crate) fn plan_path(root: &Path, dirs: &[String], name: &str) -> Option<String> {
    let file = format!("PLAN-{name}.md");
    dirs.iter().find_map(|d| {
        let dir = Path::new(d);
        let full = if dir.is_absolute() {
            dir.to_path_buf()
        } else {
            root.join(dir)
        };
        if !full.join(&file).exists() {
            return None;
        }
        // a relative dir stays relative — the line reads like a command
        let shown = if dir.is_absolute() {
            full.join(&file).to_string_lossy().replace('\\', "/")
        } else {
            let dir = d.trim_end_matches(['/', '\\']);
            match dir.is_empty() {
                true => file.clone(),
                false => format!("{dir}/{file}"),
            }
        };
        Some(shown)
    })
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
