//! Auto sync (PLAN-fael-journal-transport chunk 6): session start, and a Stop
//! that lets a turn through, start one `fael sync` when `fael.remote` is set —
//! once per session and worktree for each newest row this writer has filed.
//! Session start waits for its sync up to a bound so teammates' rows reach
//! the kickoff; a sync still running past the bound stays detached and the
//! start says so in one line, and the rows reach the session's later reads.
//! A turn's new rows go out at the next Stop (detached, never waited for),
//! and a Stop with nothing new starts nothing. Never per `add`.
//!
//! Skip, never block: the sync runs as a detached child whose output goes to
//! `<state>/auto-sync-<repo>.log` (last run only, one file per repo so one repo's
//! run never erases another's last error), so a dead network, a slow remote or
//! a failed auth costs the turn nothing past the session-start bound and
//! nothing retries. `GIT_TERMINAL_PROMPT=0`
//! turns a credential prompt into a failure instead of a hang; ssh prompts
//! (host key, passphrase) read `/dev/tty` instead, so OpenSSH gets `BatchMode`.
// ponytail: with a remote set, the mark is the writer's newest row id, read from the
// journal at each event; without one the session is marked "no-remote", but the
// mark saves a journal read, never the config look — every event re-checks
// `fael.remote` (one cheap spawn), so a remote set mid-session syncs at the
// next event instead of waiting for the next session. A row filed after the
// last Stop of a session waits for the next session's start (a real SessionEnd
// event is the upgrade). A wedged remote leaves one idle git child until the
// OS reaps it.

use super::protocol::Event;
use super::state::{session_key, state_dir};
use super::stop::seen_before;
use crate::{git, repo_at};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// How long session start waits for its sync (Threshold trial: the owner
/// sets it) — never blocks a start past it. `FAEL_SYNC_WAIT_MS` overrides it
/// (tests); Stop never waits.
fn session_bound() -> Duration {
    std::env::var("FAEL_SYNC_WAIT_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .map(Duration::from_millis)
        .unwrap_or(Duration::from_secs(3))
}

/// What the session-start sync did.
pub(crate) enum SessionSync {
    /// No sync: no session, no repo, the off switch, no remote, or this
    /// newest row already synced — the kickoff reads as before.
    Skipped,
    /// The sync finished inside the bound: the caller re-reads the journal.
    Done,
    /// Still running past the bound, left detached: the caller says one
    /// `sync still running` line and the rows land at the next read.
    Running,
}

/// Ready to sync: the repo, session and worktree behind `e`.
struct Ready {
    repo: crate::Repo,
    session: String,
    wt: String,
}

/// The guards both events share: a session, a repo, the switch on, and a
/// remote — marking the session "no-remote" when there is none. The mark
/// never gates: a remote set after it is seen at the next event.
fn ready(e: &Event) -> Option<Ready> {
    let session = e.session.as_deref().filter(|s| !s.is_empty())?;
    let cwd = e
        .cwd
        .as_deref()
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok())?;
    let repo = repo_at(&cwd).ok()?;
    if !repo.cfg.sync_auto {
        return None;
    }
    // remote first: the mark saves a journal read, never this look — one
    // cheap `git config` per event picks up a remote set mid-session
    let wt = repo.root.to_string_lossy().into_owned();
    if git(&repo.root, &["config", "fael.remote"]).is_none() {
        seen_before(session, &wt, "no-remote");
        return None;
    }
    Some(Ready {
        repo,
        session: session.to_string(),
        wt,
    })
}

pub(crate) fn start(e: &Event) {
    let Some(r) = ready(e) else { return };
    if seen_before(&r.session, &r.wt, &format!("sync:{}", newest(&r.repo))) {
        return;
    }
    // detached and never awaited: teammates' rows reach later reads, not this stop
    let _ = spawn(&r.repo);
}

/// Session start's sync: like [`start`], but waits up to [`session_bound`]
/// so the kickoff sees what the sync ingests. Past the bound the child is
/// left running detached ([`SessionSync::Running`]).
pub(crate) fn start_session(e: &Event) -> SessionSync {
    let Some(r) = ready(e) else {
        return SessionSync::Skipped;
    };
    if seen_before(&r.session, &r.wt, &format!("sync:{}", newest(&r.repo))) {
        return SessionSync::Skipped;
    }
    let Some(mut child) = spawn(&r.repo) else {
        return SessionSync::Skipped;
    };
    let bound = session_bound();
    let t = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return SessionSync::Done,
            Ok(None) if t.elapsed() >= bound => return SessionSync::Running,
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            // fail open: a wait error reads like a finished sync
            Err(_) => return SessionSync::Done,
        }
    }
}

/// Spawn the detached `fael sync` child. `None` = unspawnable (fail open).
fn spawn(repo: &crate::Repo) -> Option<Child> {
    let exe = std::env::current_exe().ok()?;
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
    sync.arg("sync")
        .current_dir(&repo.root)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("FAEL_FROM", "autosync")
        .stdin(Stdio::null())
        .stdout(out)
        .stderr(err)
        .spawn()
        .ok()
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

/// This writer's newest row or close id — the once-per-newest-row mark.
fn newest(r: &crate::Repo) -> String {
    let (log, by) = (crate::read(r), crate::writer(r));
    let mine = log.rows.iter().chain(&log.closes).filter(|x| x.by == by);
    mine.map(|x| x.id.as_str()).max().unwrap_or("").to_string()
}
