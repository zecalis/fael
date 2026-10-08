//! Per-machine runtime state, never in `.fael/` (that is shared project
//! data): session edit lists, seen ids, once-per-session marks, usage — plus the
//! tiny std-only time helpers the hook path uses instead of chrono.

use crate::core;
use std::collections::HashSet;
use std::fs::{File, OpenOptions};
use std::io::Write;
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

/// The session id inside a hook session string. Claude keys hooks by the
/// transcript path (`…/<session id>.jsonl`), while `add`/`find` only know
/// `$CLAUDE_CODE_SESSION_ID` — the stem is what both share, so the seen list
/// is keyed by it and a row filed before the session's first edit (no edit
/// file yet to bridge the two) still lands where the push reads.
fn session_id(s: &str) -> &str {
    if !s.contains(['/', '\\']) {
        return s;
    }
    Path::new(s)
        .file_stem()
        .and_then(|f| f.to_str())
        .unwrap_or(s)
}

/// Ids already pushed into one context window, one per line: the session's
/// own thread (`agent` empty), or one sub-agent — it starts with an empty
/// context, so what the parent was told says nothing about what it knows.
/// Session-start drops the thread's list when the client compacted.
pub(crate) fn seen_path(session: &str, agent: &str, root: &Path) -> PathBuf {
    let sub = match agent {
        "" => String::new(),
        a => format!("\0{a}"),
    };
    let id = session_id(session);
    let key = session_key(&format!("{id}\0{}{sub}", root.to_string_lossy()));
    state_dir().join("sessions").join(format!("{key}.seen"))
}

/// `<seen file>.touched` — the files this context window has pushed on, one
/// per line (the session's working set). Keyed like the seen list, so the
/// lock `lock_seen` holds guards it too.
/// `<main seen file>.turn` — the id of the user's current turn, rewritten by
/// every prompt. Keyed without the sub-agent, so a sub-agent's edits share
/// the turn of the prompt that started them.
pub(crate) fn turn_path(session: &str, root: &Path) -> PathBuf {
    seen_path(session, "", root).with_extension("turn")
}

/// Mark a new user turn: a fresh id, so `per_turn` kinds may speak again.
/// Fails open: unwritable = no new turn, the old mark stands.
pub(crate) fn new_turn(session: &str, root: &Path) {
    let p = turn_path(session, root);
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let _ = std::fs::write(p, crate::core::ulid());
}

/// The current turn's id; `None` before any prompt marked one (a client
/// without a prompt hook keeps no per-turn limit).
pub(crate) fn read_turn(session: &str, root: &Path) -> Option<String> {
    (!session.is_empty())
        .then(|| std::fs::read_to_string(turn_path(session, root)).ok())
        .flatten()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
}

pub(crate) fn touched_path(session: &str, agent: &str, root: &Path) -> PathBuf {
    seen_path(session, agent, root).with_extension("touched")
}

/// The working set so far, then `files` added to it — one read and one append,
/// so the caller's `touch` counts only what came before this push. Fails open:
/// unreadable = empty, unwritable = not remembered.
pub(crate) fn swap_touched(p: &Path, files: &[String]) -> HashSet<String> {
    let before: HashSet<String> = std::fs::read_to_string(p)
        .unwrap_or_default()
        .lines()
        .map(String::from)
        .collect();
    let new: String = files
        .iter()
        .filter(|f| !before.contains(*f))
        .map(|f| format!("{f}\n"))
        .collect();
    if !new.is_empty()
        && let Ok(mut f) = OpenOptions::new().create(true).append(true).open(p)
    {
        let _ = f.write_all(new.as_bytes());
    }
    before
}

/// Chunk 6e: ids this session already holds in context — just filed by `add`
/// or just shown by `find --files`. The next push skips them instead of
/// repeating them. Empty session or ids = no-op (MCP outside a hook session).
// ponytail: always the session's own thread — an `add`/`find` run by a sub-agent
// carries no agent id, so that sub-agent may be told its own row once more.
pub(crate) fn note_seen(session: &str, root: &Path, ids: &[&str]) {
    if session.is_empty() || ids.is_empty() {
        return;
    }
    let out: String = ids.iter().map(|id| format!("{id}\n")).collect();
    if let Some(mut f) = lock_seen(&seen_path(session, "", root)) {
        let _ = f.write_all(out.as_bytes());
    }
}

/// Open a seen list under an exclusive lock held until the returned file
/// drops. The push reads, filters and appends under it: reads fired in one
/// batch would otherwise each see the old list and push the same row. `None`
/// = unopenable, and the push then runs unlisted (fail open).
// ponytail: a filesystem without flock runs unlocked, as before the lock.
pub(crate) fn lock_seen(p: &Path) -> Option<File> {
    std::fs::create_dir_all(p.parent()?).ok()?;
    let f = OpenOptions::new()
        .create(true)
        .read(true)
        .append(true)
        .open(p)
        .ok()?;
    let _ = f.lock();
    Some(f)
}

/// A Weak risk line stashed by stop for the next push — shown once, deleted.
pub(crate) fn risk_path(session: &str, root: &Path) -> PathBuf {
    let key = session_key(&format!("{session}\0{}", root.to_string_lossy()));
    state_dir().join("sessions").join(format!("{key}.risk"))
}

/// A capture-reject hint stashed by stop for the next push — shown once, deleted.
pub(crate) fn hint_path(session: &str, root: &Path) -> PathBuf {
    let key = session_key(&format!("{session}\0{}", root.to_string_lossy()));
    state_dir().join("sessions").join(format!("{key}.hint"))
}

/// A fix phrase stashed by stop for the next push (PLAN-fael-experience-loop
/// chunk 5a) — shown once, deleted.
pub(crate) fn fixed_path(session: &str, root: &Path) -> PathBuf {
    let key = session_key(&format!("{session}\0{}", root.to_string_lossy()));
    state_dir().join("sessions").join(format!("{key}.fixed"))
}

/// The checked-out branch, read straight from `<gitdir>/HEAD` — no git spawn
/// on this path (session-start already spawns elsewhere). Lives in `journal`
/// beside the git-dir traversal the journal root uses too, so the two cannot
/// drift.
pub(crate) use crate::journal::head_branch;

/// Per-session files untouched this long are dead: a resumed session only
/// loses its seen ids (rows push again) — it fails quiet.
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

/// The stashed risk note, capture-reject hint and fix phrase, if any — left
/// on disk until `clear_stash`, so a push that had no budget for them leaves
/// them for the next.
pub(crate) fn peek_stash(session: &str, root: &Path) -> [Option<String>; 3] {
    let peek = |path: &Path| {
        let s = std::fs::read_to_string(path).ok()?;
        let s = s.trim().to_string();
        (!s.is_empty()).then_some(s)
    };
    [risk_path, hint_path, fixed_path].map(|p| peek(&p(session, root)))
}

/// Delete the stashes — once their line was said, never before.
pub(crate) fn clear_stash(session: &str, root: &Path) {
    for p in [risk_path, hint_path, fixed_path] {
        let _ = std::fs::remove_file(p(session, root));
    }
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

/// The files this session edited in this worktree, newest first, deduped, at
/// most `n` — what a fix line names for `--files`, not the file the next push
/// happens to touch. Empty = no edit recorded.
pub(crate) fn edited_files(session: &str, root: &Path, n: usize) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    for (path, ..) in session_edits(&edits_path(session, root)).into_iter().rev() {
        if out.len() == n {
            break;
        }
        if !out.contains(&path) {
            out.push(path);
        }
    }
    out
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
    use super::{STALE_SECS, head_branch, prune_sessions, swap_touched};
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

    #[test]
    fn swap_touched_returns_the_set_before_and_appends_only_new_files() {
        let d = std::env::temp_dir().join(format!("fael-touched-{}", fael_core::ulid()));
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("s.touched");
        let files = |l: &[&str]| l.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        // no file yet: an empty set, and the files are remembered
        assert!(swap_touched(&p, &files(&["a", "b"])).is_empty());
        // the push's own files are not in the set it is handed
        let before = swap_touched(&p, &files(&["b", "c"]));
        assert_eq!(before.len(), 2);
        assert!(before.contains("a") && before.contains("b"));
        // a known file is never appended twice
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "a\nb\nc\n");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Fail open: a path that cannot be read or written is an empty set, not a panic.
    #[test]
    fn swap_touched_fails_open() {
        let d = std::env::temp_dir().join(format!("fael-touched-{}", fael_core::ulid()));
        std::fs::create_dir_all(&d).unwrap();
        // a directory cannot be read as a file, nor appended to
        assert!(swap_touched(&d, &["a".to_string()]).is_empty());
        // a parent that is a file: nothing can be created under it
        let blocker = d.join("f");
        std::fs::write(&blocker, "x").unwrap();
        assert!(swap_touched(&blocker.join("s.touched"), &["a".to_string()]).is_empty());
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
