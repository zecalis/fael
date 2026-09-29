//! Per-machine runtime state, never in `.fael/` (that is shared project
//! data): session edit lists, seen ids, stop-block dedupe, usage — plus the
//! tiny std-only time helpers the hook path uses instead of chrono.

use crate::core;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Per-machine state dir — owned by `fael-core::stats`, re-exported here so
/// every hook path keeps spelling `state_dir()`.
pub(crate) use crate::core::stats::state_dir;

/// Opaque filename for a session id or transcript path (paths are long).
pub(crate) fn session_key(s: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    s.hash(&mut h);
    format!("{:016x}", h.finish())
}

/// Keyed by session + worktree so one session across two repos stays apart.
pub(crate) fn edits_path(session: &str, root: &Path) -> PathBuf {
    let key = session_key(&format!("{session}\0{}", root.to_string_lossy()));
    state_dir().join("sessions").join(format!("{key}.jsonl"))
}

/// Ids already pushed in this session, one per line.
// ponytail: after the client compacts its context the pushed text may be gone, yet the
// row stays "seen" — add a reset on the compact hook if agents miss rows because of it.
pub(crate) fn seen_path(session: &str, root: &Path) -> PathBuf {
    let key = session_key(&format!("{session}\0{}", root.to_string_lossy()));
    state_dir().join("sessions").join(format!("{key}.seen"))
}

/// Chunk 6e: ids this session already holds in context — just filed by `add`
/// or just shown by `find --files`. The next push skips them instead of
/// repeating them. Empty session or ids = no-op (MCP outside a hook session).
pub(crate) fn note_seen(session: &str, root: &Path, ids: &[&str]) {
    if session.is_empty() || ids.is_empty() {
        return;
    }
    let p = seen_path(session, root);
    use std::io::Write;
    let out: String = ids.iter().map(|id| format!("{id}\n")).collect();
    let _ = std::fs::create_dir_all(p.parent().unwrap_or(root));
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&p)
        .and_then(|mut f| f.write_all(out.as_bytes()));
}

/// A Weak risk line stashed by stop for the next push — shown once, deleted.
pub(crate) fn risk_path(session: &str, root: &Path) -> PathBuf {
    let key = session_key(&format!("{session}\0{}", root.to_string_lossy()));
    state_dir().join("sessions").join(format!("{key}.risk"))
}

/// The branch the session started on, written by session-start and read by
/// stop (row-hygiene chunk 9) — two agents sharing one worktree move HEAD
/// under each other, so stop warns instead of letting a push/PR land on the
/// wrong branch. No file (sessions from before this existed) = silent.
pub(crate) fn branch_path(session: &str, root: &Path) -> PathBuf {
    let key = session_key(&format!("{session}\0{}", root.to_string_lossy()));
    state_dir().join("sessions").join(format!("{key}.branch"))
}

/// The checked-out branch, read straight from `<gitdir>/HEAD` — no git spawn
/// on this path (read/edit push must stay spawn-free; session-start already
/// spawns elsewhere, stop only here). Lives in `journal` beside the git-dir
/// traversal the journal root uses too, so the two cannot drift.
pub(crate) use crate::journal::head_branch;

/// Per-session files untouched this long are dead: a resumed session only
/// loses its seen ids (rows push again) and its start branch (drift stays
/// silent) — both fail quiet.
const STALE_SECS: u64 = 30 * 24 * 60 * 60;

/// Drop per-session files older than `STALE_SECS` — without this `sessions/`
/// grows by a few files per session forever. Called once per session-start;
/// every error is ignored (the next session tries again).
pub(crate) fn prune_sessions(dir: &Path) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let stale = e
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age.as_secs() > STALE_SECS);
        if stale {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

/// Take the stashed risk note, if any — the file is gone after this call.
pub(crate) fn take_risk(session: &str, root: &Path) -> Option<String> {
    let path = risk_path(session, root);
    let s = std::fs::read_to_string(&path).ok()?;
    let _ = std::fs::remove_file(&path);
    let s = s.trim().to_string();
    (!s.is_empty()).then_some(s)
}

/// One `{"path","at"[, "worktree","session"]}` line per edit event, in order —
/// (path, at ms, worktree, session; None for lines written before those were).
/// A torn or unreadable line is skipped; any error is an empty list.
pub(crate) type Edit = (String, i64, Option<String>, Option<String>);
pub(crate) fn session_edits(path: &Path) -> Vec<Edit> {
    let Ok(s) = std::fs::read_to_string(path) else {
        return vec![];
    };
    s.lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter_map(|v| {
            Some((
                v["path"].as_str()?.to_string(),
                core::ts_ms(v["at"].as_str()?)?,
                v["worktree"].as_str().map(String::from),
                v["session"].as_str().map(String::from),
            ))
        })
        .collect()
}

/// Append one line per file. Fails open like record_usage. Worktree and
/// session ride along so `fael add` without `--files` can tell which session
/// files are its own, in its repo (the filename hash is one-way).
pub(crate) fn record_edits(path: &Path, worktree: &str, session: &str, files: &[String]) {
    let Some(at) = now_rfc3339() else { return };
    if let Some(parent) = path.parent()
        && std::fs::create_dir_all(parent).is_ok()
    {
        use std::io::Write;
        let body: String = files
            .iter()
            .map(|f| {
                format!(
                    "{}\n",
                    serde_json::json!({"path": f, "at": at, "worktree": worktree, "session": session})
                )
            })
            .collect();
        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .and_then(|mut f| f.write_all(body.as_bytes()));
    }
}

/// A file's birthtime (fallback: mtime) as unix ms.
pub(crate) fn file_birth_ms(path: &Path) -> Option<u64> {
    let md = std::fs::metadata(path).ok()?;
    let t = md.created().or_else(|_| md.modified()).ok()?;
    Some(t.duration_since(SystemTime::UNIX_EPOCH).ok()?.as_millis() as u64)
}

pub(crate) fn now_rfc3339() -> Option<String> {
    systemtime_to_rfc3339(SystemTime::now())
}

fn systemtime_to_rfc3339(st: SystemTime) -> Option<String> {
    let ms = st.duration_since(SystemTime::UNIX_EPOCH).ok()?.as_millis() as u64;
    Some(core::rfc3339(ms))
}

#[cfg(test)]
mod tests {
    use super::{STALE_SECS, head_branch, prune_sessions};
    use std::time::{Duration, SystemTime};

    #[test]
    fn prune_drops_only_stale_session_files() {
        let d = std::env::temp_dir().join(format!("fael-prune-{}", fael_core::ulid()));
        std::fs::create_dir_all(&d).unwrap();
        let (old, new) = (d.join("a.seen"), d.join("b.seen"));
        std::fs::write(&old, "x").unwrap();
        std::fs::write(&new, "x").unwrap();
        let past = SystemTime::now() - Duration::from_secs(STALE_SECS + 60);
        std::fs::File::options()
            .write(true)
            .open(&old)
            .unwrap()
            .set_modified(past)
            .unwrap();
        prune_sessions(&d);
        assert!(!old.exists() && new.exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    /// CI runs this on Windows too: a worktree's `.git` file with an absolute
    /// or relative `gitdir:` (git writes `/` there on every OS), and a
    /// CRLF-ended HEAD, all resolve; a detached HEAD is None.
    #[test]
    fn head_branch_reads_dir_and_worktree_file() {
        let d = std::env::temp_dir().join(format!("fael-head-{}", fael_core::ulid()));
        let git = d.join("main/.git");
        std::fs::create_dir_all(&git).unwrap();
        std::fs::write(git.join("HEAD"), "ref: refs/heads/feat/x\r\n").unwrap();
        assert_eq!(head_branch(&d.join("main")).as_deref(), Some("feat/x"));

        let wt_git = git.join("worktrees/wt");
        std::fs::create_dir_all(&wt_git).unwrap();
        std::fs::write(wt_git.join("HEAD"), "ref: refs/heads/fix/y\n").unwrap();
        let abs = d.join("abs");
        std::fs::create_dir_all(&abs).unwrap();
        let fwd = wt_git.to_string_lossy().replace('\\', "/");
        std::fs::write(abs.join(".git"), format!("gitdir: {fwd}\n")).unwrap();
        assert_eq!(head_branch(&abs).as_deref(), Some("fix/y"));

        let rel = d.join("rel");
        std::fs::create_dir_all(&rel).unwrap();
        std::fs::write(rel.join(".git"), "gitdir: ../main/.git/worktrees/wt\r\n").unwrap();
        assert_eq!(head_branch(&rel).as_deref(), Some("fix/y"));

        std::fs::write(
            wt_git.join("HEAD"),
            "0123456789abcdef0123456789abcdef01234567\n",
        )
        .unwrap();
        assert_eq!(head_branch(&rel), None);
        let _ = std::fs::remove_dir_all(&d);
    }
}
