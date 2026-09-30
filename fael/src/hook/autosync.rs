//! Auto sync (PLAN-fael-journal-transport chunk 6): session start, and a Stop
//! that lets a turn through, start one `fael sync` when `fael.remote` is set —
//! once per session and worktree for each newest row this writer has filed.
//! Session start runs it first, so teammates' rows land in the journal before
//! the session's first read and the rows the last session left behind go out;
//! a turn's new rows go out at the next Stop, and a Stop with nothing new
//! starts nothing. Never per `add`.
//!
//! Skip, never block: the sync runs as a detached child whose output goes to
//! `<state>/auto-sync-<repo>.log` (last run only, one file per repo so one repo's
//! run never erases another's last error), so a dead network, a slow remote or
//! a failed auth costs the turn nothing and nothing retries. `GIT_TERMINAL_PROMPT=0`
//! turns a credential prompt into a failure instead of a hang; ssh prompts
//! (host key, passphrase) read `/dev/tty` instead, so OpenSSH gets `BatchMode`.
// ponytail: the mark is the writer's newest row id, read from the journal at each
// event; a row filed after the last Stop of a session waits for the next session's
// start (a real SessionEnd event is the upgrade). The session-start sync is never
// awaited, so its rows reach reads and pushes, not that session's kickoff context. No timeout either — a wedged
// remote leaves one idle git child until the OS reaps it.

use super::protocol::Event;
use super::state::{session_key, state_dir};
use super::stop::stop_blocked_before;
use crate::{Repo, git, repo_at};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub(crate) fn start(e: &Event) {
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
    // per newest row for everyone, not once per turn
    if !repo.cfg.sync_auto {
        return;
    }
    let kind = format!("sync:{}", newest(&repo));
    if stop_blocked_before(session, &repo.root.to_string_lossy(), &kind) {
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
        .and_then(|()| std::fs::File::create(state_dir().join(log_name(&repo.root))).ok());
    let (out, err) = match log.and_then(|f| f.try_clone().ok().map(|c| (f, c))) {
        Some((a, b)) => (Stdio::from(a), Stdio::from(b)),
        None => (Stdio::null(), Stdio::null()),
    };
    let mut sync = Command::new(exe);
    if let Some(ssh) = batch_ssh(&repo.root) {
        sync.env("GIT_SSH_COMMAND", ssh);
    }
    let _ = sync
        .arg("sync")
        .current_dir(&repo.root)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .stdout(out)
        .stderr(err)
        .spawn();
}

/// The newest row id this writer filed, or empty — what the session mark keys on.
fn newest(repo: &Repo) -> String {
    let (by, log) = (crate::writer(repo), crate::read(repo));
    let mine = log.rows.iter().chain(&log.closes).filter(|r| r.by == by);
    mine.map(|r| r.id.as_str()).max().unwrap_or("").to_string()
}

/// `auto-sync-<hash of the worktree path>.log` — per worktree, so a repo's
/// runs share one file and other repos' logs are left alone.
fn log_name(root: &Path) -> String {
    format!("auto-sync-{}.log", session_key(&root.to_string_lossy()))
}

/// The ssh command git would run (`GIT_SSH_COMMAND`, else `core.sshCommand`,
/// else `ssh`) plus `-o BatchMode=yes` when it is OpenSSH. `None` leaves git's
/// own choice alone: `GIT_SSH`, plink or a wrapper may not take `-o`.
fn batch_ssh(root: &Path) -> Option<String> {
    let env = std::env::var("GIT_SSH_COMMAND").ok();
    if env.is_none() && std::env::var_os("GIT_SSH").is_some() {
        return None;
    }
    let cmd = env
        .or_else(|| git(root, &["config", "core.sshCommand"]))
        .unwrap_or_else(|| "ssh".into());
    let prog = Path::new(cmd.split_whitespace().next()?).file_stem()?;
    (prog == "ssh").then(|| format!("{cmd} -o BatchMode=yes"))
}
