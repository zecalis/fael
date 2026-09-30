//! Session-end auto sync (PLAN-fael-journal-transport chunk 6): the first Stop
//! that lets a turn through, per session and worktree, starts one `fael sync`
//! when `fael.remote` is set — never per `add`, never a second time.
//!
//! Skip, never block: the sync runs as a detached child whose output goes to
//! `<state>/auto-sync.log` (last run only), so a dead network, a slow remote or
//! a failed auth costs the turn nothing and nothing retries. `GIT_TERMINAL_PROMPT=0`
//! turns a credential prompt into a failure instead of a hang.
// ponytail: Stop fires per turn in Claude/Codex, so "once per session" means the
// first turn's end: rows filed later ride the next session's first stop. A real
// session-end event (SessionEnd) is the upgrade if that lag matters. No timeout
// either — a wedged remote leaves one idle git child until the OS reaps it.

use super::protocol::Event;
use super::state::state_dir;
use super::stop::stop_blocked_before;
use crate::{git, repo_at};
use std::path::PathBuf;
use std::process::{Command, Stdio};

pub(crate) fn after_stop(e: &Event) {
    let session = match e.session.as_deref() {
        Some(s) if !s.is_empty() => s,
        _ => return,
    };
    let Some(cwd) = e
        .cwd
        .as_deref()
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok())
    else {
        return;
    };
    let Ok(repo) = repo_at(&cwd) else { return };
    // marks the session even without a remote: the git spawn below runs once
    // per session for everyone, not once per turn
    if !repo.cfg.sync_auto || stop_blocked_before(session, &repo.root.to_string_lossy(), "sync") {
        return;
    }
    if git(&repo.root, &["config", "fael.remote"]).is_none() {
        return;
    }
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let log = std::fs::create_dir_all(state_dir())
        .ok()
        .and_then(|()| std::fs::File::create(state_dir().join("auto-sync.log")).ok());
    let (out, err) = match log.and_then(|f| f.try_clone().ok().map(|c| (f, c))) {
        Some((a, b)) => (Stdio::from(a), Stdio::from(b)),
        None => (Stdio::null(), Stdio::null()),
    };
    let _ = Command::new(exe)
        .arg("sync")
        .current_dir(&repo.root)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .stdout(out)
        .stderr(err)
        .spawn();
}
