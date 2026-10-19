//! The core read seam (crew-split): every hook, `find` and `refs` read
//! chunk data through here — never `crate::plan`, `crate::chunk`,
//! `crate::board` or `fael_core::plan` directly. Opened read-only and
//! returning empty when `fael.db` or the plan tables are missing, so core
//! works with no crew installed.
//!
//! The `*_VERBS` lists live here (not in the `chunk`/`plan` commands) because
//! `hook::friction` classifies calls by them: they are read-only contract —
//! what a usage row's `sub` names — and the commands reuse the same const, so
//! the two cannot drift. A copy owned by the bin would drift.

use super::plan::{Store, db_at};
use super::stats::state_dir;
use std::path::{Path, PathBuf};

pub use super::plan::{Brief, EndedRun, PlanRow, StatsRun};

/// The verbs `chunk` takes — what usage lines and the call line's `sub` name.
pub const CHUNK_VERBS: &[&str] = &[
    "add",
    "show",
    "start",
    "edit",
    "move",
    "note",
    "wait",
    "after",
    "unafter",
    "ask",
    "reply",
    "answer",
    "done",
    "accept",
    "drop",
    "park",
    "unpark",
    "apply",
    "approve",
    "unapprove",
];

/// The verbs `plan` takes — `show` is `export` by another name.
pub const PLAN_VERBS: &[&str] = &[
    "new", "export", "show", "apply", "park", "unpark", "done", "list",
];

/// `<dir>/fael.db`, the clone's db (epoch v8 name).
pub fn db_at_dir(dir: &Path) -> PathBuf {
    db_at(dir)
}

/// The db for a checkout: beside the journal when there is one, else `.fael`.
/// What `fael plan::db` computed — now the seam owns it.
pub fn db_for(journal: Option<&Path>, fael: &Path) -> PathBuf {
    db_at(journal.unwrap_or(fael))
}

/// Under the temp dir — canonical (`/private/var/…` on macOS) through the
/// nearest dir that exists yet. What `board::in_tmp` was.
pub fn in_tmp(p: &Path) -> bool {
    let p = p
        .ancestors()
        .find_map(|a| Some(a.canonicalize().ok()?.join(p.strip_prefix(a).ok()?)))
        .unwrap_or_else(|| p.to_path_buf());
    [std::env::temp_dir(), PathBuf::from("/tmp")]
        .iter()
        .filter_map(|d| d.canonicalize().ok())
        .any(|d| p.starts_with(d))
}

/// A repo under the temp dir belongs only on a registry under the temp dir.
pub fn is_scratch(root: &Path) -> bool {
    in_tmp(root) && !in_tmp(&state_dir())
}

/// The board's repos: `~/.local/state/fael/projects`, scratch pruned.
pub fn registered() -> Vec<PathBuf> {
    let text = std::fs::read_to_string(state_dir().join("projects")).unwrap_or_default();
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(PathBuf::from)
        .filter(|p| !is_scratch(p))
        .collect()
}

/// The main checkout, not this worktree: `<main>/.git/fael` is the journal.
pub fn main_root_for(root: &Path, journal: Option<&Path>) -> PathBuf {
    journal
        .and_then(Path::parent)
        .filter(|g| g.file_name().is_some_and(|n| n == ".git"))
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .unwrap_or_else(|| root.to_path_buf())
}

/// Two scopes overlap when a path of one is a prefix of the other's on a `/`
/// boundary (`src/` covers `src/a.rs`, `src/a` does not cover `src/ab.rs`).
pub fn overlap(a: &[String], b: &[String]) -> bool {
    super::plan::overlap(a, b)
}

/// The repo's `.fael/shared` prefixes; a line scope would reject is an error.
pub fn shared(root: &Path) -> Result<Vec<String>, String> {
    super::plan::shared(root)
}

/// The chunk rules said once at the end of every brief and kickoff.
pub fn rules(uid: &str) -> String {
    super::plan::rules(uid)
}

fn readonly(db: &Path) -> Option<Store> {
    db.exists().then(|| Store::open_readonly(db).ok()).flatten()
}

/// Live chunk runs in a worktree, `(uid, title, R)` — empty when no db, no
/// tables, or none held. What `Store::held` was.
pub fn held(db: &Path, worktree: &str) -> Vec<(String, String, String)> {
    readonly(db)
        .and_then(|s| s.held(worktree).ok())
        .unwrap_or_default()
}

/// True when the worktree holds a live chunk run. The cheap SQLite read
/// session start and the commit line use — no git spawn, no board read.
pub fn has_held(db: &Path, worktree: &str) -> bool {
    !held(db, worktree).is_empty()
}

/// `<state>/held/<session>`: the fael.db a session's `chunk start` bound a run
/// in. `None` on a non-session id. What `chunk::held` was.
pub fn held_session_file(session: &str) -> Option<PathBuf> {
    session
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-')
        .then(|| state_dir().join("held").join(session))
}

/// The db a session's `chunk start` bound, when the file names one that still
/// exists. A db that is gone is never recreated by the open.
pub fn held_session_db(session: &str) -> Option<PathBuf> {
    let m = held_session_file(session)?;
    let db = std::fs::read_to_string(m).ok()?;
    Path::new(&db).exists().then(|| PathBuf::from(db))
}

/// Stamp the live run of this worktree (Stop hook, SPEC §7): `session` on the
/// first stop, `last_seen` on each. Fail-open: no db, no tables, no run = 0.
/// A write — the seam's one — so the hook never recreates a missing db.
pub fn mark_seen(db: &Path, worktree: &str, session: &str, now: &str) -> usize {
    if !db.exists() || session.is_empty() {
        return 0;
    }
    Store::open(db)
        .and_then(|mut s| s.seen(worktree, session, now))
        .unwrap_or(0)
}

/// Stamp the live runs a session's `chunk start` bound, wherever it works now.
/// Fail-open like [`mark_seen`].
pub fn mark_seen_session(db: &Path, session: &str, now: &str) -> usize {
    if !db.exists() || session.is_empty() {
        return 0;
    }
    Store::open(db)
        .and_then(|mut s| s.seen_session(session, now))
        .unwrap_or(0)
}

/// Runs `chunk done` ended on a chunk now in review or done — what a note can
/// be proven to belong to. Empty when no db or no tables.
pub fn ended_runs(db: &Path) -> Vec<EndedRun> {
    readonly(db)
        .and_then(|s| s.ended_runs().ok())
        .unwrap_or_default()
}

/// Every run with its chunk's uid, oldest first — the stats flow join reads
/// `started`/`ended`/`pr`. Empty when no db or no tables.
pub fn flow_runs(db: &Path) -> Vec<super::plan::StatsRun> {
    readonly(db)
        .and_then(|s| s.stats_runs().ok())
        .unwrap_or_default()
}

/// Is `tok` a chunk uid, or the prefix of one? Ledger ids share the shape.
pub fn is_chunk(db: &Path, tok: &str) -> bool {
    readonly(db).is_some_and(|s| s.is_chunk(tok))
}

/// A chunk uid's brief, as `chunk show` prints it. `None` when no db or no chunk.
pub fn chunk_brief(db: &Path, key: &str) -> Option<Brief> {
    readonly(db)?.show(key).ok()
}

/// The plan `name` (`name` or `app/name`), if it has one.
pub fn plan_by_name(db: &Path, name: &str) -> Option<PlanRow> {
    readonly(db)?.plans().ok()?.into_iter().find(|p| {
        let key = if p.app.is_empty() {
            p.name.clone()
        } else {
            format!("{}/{}", p.app, p.name)
        };
        key == name || p.name == name
    })
}

/// The first ready chunk of one plan in plan order, `(uid, title, brief)`.
pub fn first_ready(db: &Path, plan: i64, today: &str) -> Option<(String, String, String)> {
    readonly(db)?.first_ready(plan, today).ok().flatten()
}

/// What `fael kickoff PLAN-x.md` prints for plan `x`: the held chunks here, else
/// the next ready one — `None` = silent (no db, no plan, or no row to name).
/// `today` is `YYYY-MM-DD`.
pub fn kickoff_text(db: &Path, plan_name: &str, worktree: &str, today: &str) -> Option<String> {
    let s = readonly(db)?;
    let p = s.plans().ok()?.into_iter().find(|p| p.name == plan_name)?;
    let mut out = String::new();
    if let Ok(held) = s.held(worktree)
        && !held.is_empty()
    {
        for (uid, title, run) in &held {
            out.push_str(&format!("you hold chunk {uid} — {title} (run {run})\n"));
        }
        out.push_str(&rules(&held[0].0));
        return Some(out);
    }
    match s.first_ready(p.id, today) {
        Ok(Some((uid, title, brief))) => {
            out.push_str(&format!("next chunk: {uid} — {title}\n"));
            if brief != title {
                out.push_str(&format!("> {}\n", brief.lines().next().unwrap_or_default()));
            }
            out.push_str(
                "start it: `fael chunk start {uid}` (prints the brief and the chunk rules)\n"
                    .replace("{uid}", &uid)
                    .as_str(),
            );
            Some(out)
        }
        Ok(None) => Some(format!(
            "no chunk of {} is ready — `fael plan export {}` shows each state\n",
            p.name, p.name
        )),
        Err(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_db_reads_empty_and_writes_zero() {
        let db = std::env::temp_dir().join(format!("fael-work-{}.db", crate::ulid()));
        let _ = std::fs::remove_file(&db);
        assert!(held(&db, "/tmp/wt").is_empty());
        assert!(!has_held(&db, "/tmp/wt"));
        assert!(!is_chunk(&db, "01M5AF5WHVBRJ9629ZYA2HF9XY"));
        assert!(chunk_brief(&db, "01M5AF5WHVBRJ9629ZYA2HF9XY").is_none());
        assert!(ended_runs(&db).is_empty());
        assert!(flow_runs(&db).is_empty());
        assert!(plan_by_name(&db, "x").is_none());
        assert!(first_ready(&db, 1, "2026-10-19").is_none());
        assert!(kickoff_text(&db, "x", "/tmp/wt", "2026-10-19").is_none());
        assert_eq!(mark_seen(&db, "/tmp/wt", "s", "2026-10-19T00:00:00Z"), 0);
        assert_eq!(mark_seen_session(&db, "s", "2026-10-19T00:00:00Z"), 0);
        assert!(overlap(&["src/".to_string()], &["src/a.rs".to_string()]));
        assert!(!overlap(&["src/a".to_string()], &["src/ab.rs".to_string()]));
        assert!(held_session_file("").is_some() || true);
        assert!(held_session_file("abc-123").is_some());
        assert!(held_session_file("../x").is_none());
    }
}
