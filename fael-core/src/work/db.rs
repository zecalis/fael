//! Read-only crew db access: fail-open when the file or tables are missing.

use super::{Brief, EndedRun, PlanRow, StatsRun};
use std::path::{Path, PathBuf};

/// The breaking epoch crew owns; the seam only needs it to spot the stub.
const VERSION: i32 = 8;
/// The stub `plans.db`'s user_version: past any epoch a fael will ever reach.
const STUB: i32 = 1_000_000;

/// `<dir>/fael.db`, the clone's db (epoch v8 name).
pub fn db_at_dir(dir: &Path) -> PathBuf {
    db_at(dir)
}

/// The db for a checkout: beside the journal when there is one, else `.fael`.
pub fn db_for(journal: Option<&Path>, fael: &Path) -> PathBuf {
    db_at(journal.unwrap_or(fael))
}

/// `<dir>/fael.db`, with the old `plans.db` hard-linked once. A copy of the
/// crew store's rename so core finds the db without depending on crew.
pub fn db_at(dir: &Path) -> PathBuf {
    let (path, old) = (dir.join("fael.db"), dir.join("plans.db"));
    if path.exists() || rename(&old, &path).is_ok() {
        return path;
    }
    old
}

fn rename(old: &Path, new: &Path) -> Result<(), String> {
    let e = |e: String| format!("plans.db → fael.db refused: {e}; nothing changed");
    let Ok(c) =
        rusqlite::Connection::open_with_flags(old, rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE)
    else {
        return Ok(());
    };
    let v: i32 = c
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .map_err(|x| e(x.to_string()))?;
    if v > VERSION {
        return Ok(());
    }
    c.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))
        .map_err(|x| e(x.to_string()))?;
    drop(c);
    if let Err(x) = std::fs::hard_link(old, new) {
        return match x.kind() {
            std::io::ErrorKind::AlreadyExists => Ok(()),
            _ => Err(e(x.to_string())),
        };
    }
    for side in ["-wal", "-shm"] {
        let from = format!("{}{side}", old.display());
        let _ = std::fs::rename(&from, format!("{}{side}", new.display()));
    }
    let tmp = old.with_extension(format!("stub.{}", crate::ulid()));
    rusqlite::Connection::open(&tmp)
        .and_then(|c| c.pragma_update(None, "user_version", STUB))
        .map_err(|x| e(x.to_string()))?;
    std::fs::rename(&tmp, old).map_err(|x| e(x.to_string()))
}

pub(super) fn readonly(db: &Path) -> Option<rusqlite::Connection> {
    if !db.exists() {
        return None;
    }
    let c = rusqlite::Connection::open_with_flags(db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .ok()?;
    c.busy_timeout(std::time::Duration::from_secs(10)).ok()?;
    Some(c)
}

/// Live chunk runs in a worktree, `(uid, title, R)` — empty when no db/tables.
pub fn held(db: &Path, worktree: &str) -> Vec<(String, String, String)> {
    let Some(c) = readonly(db) else {
        return Vec::new();
    };
    let mut q = match c.prepare(
        "SELECT c.uid, c.title, r.start FROM run r JOIN chunk c ON c.id = r.chunk
         WHERE r.worktree = ?1 AND r.ended IS NULL ORDER BY c.seq",
    ) {
        Ok(q) => q,
        Err(_) => return Vec::new(),
    };
    q.query_map([worktree], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .and_then(|rows| rows.collect::<Result<Vec<_>, _>>())
        .unwrap_or_default()
}

/// True when the worktree holds a live chunk run.
pub fn has_held(db: &Path, worktree: &str) -> bool {
    !held(db, worktree).is_empty()
}

/// Stamp the live run of this worktree (Stop hook): fail-open, never migrates.
pub fn mark_seen(db: &Path, worktree: &str, session: &str, now: &str) -> usize {
    if !db.exists() || session.is_empty() {
        return 0;
    }
    let Ok(c) = rusqlite::Connection::open(db) else {
        return 0;
    };
    let set = if crate::ts_ms(session).is_some() {
        "last_seen = ?3"
    } else {
        "session = COALESCE(session, ?2), last_seen = ?3"
    };
    let sql = format!(
        "UPDATE run SET {set} WHERE worktree = ?1 AND ended IS NULL AND (session IS NULL OR session = ?2)"
    );
    c.execute(&sql, rusqlite::params![worktree, session, now])
        .unwrap_or(0)
}

/// Stamp the live runs a session's `chunk start` bound, wherever it works now.
pub fn mark_seen_session(db: &Path, session: &str, now: &str) -> usize {
    if !db.exists() || session.is_empty() {
        return 0;
    }
    let Ok(c) = rusqlite::Connection::open(db) else {
        return 0;
    };
    c.execute(
        "UPDATE run SET last_seen = ?2 WHERE session = ?1 AND ended IS NULL",
        rusqlite::params![session, now],
    )
    .unwrap_or(0)
}

/// Runs `chunk done` ended on a chunk now in review or done. Empty when no db.
pub fn ended_runs(db: &Path) -> Vec<EndedRun> {
    let Some(c) = readonly(db) else {
        return Vec::new();
    };
    let mut q = match c.prepare(
        "SELECT p.name, r.session, r.branch, r.ended FROM run r
         JOIN chunk c ON c.id = r.chunk JOIN plan p ON p.id = c.plan
         WHERE r.ended IS NOT NULL AND c.state IN ('review','done')",
    ) {
        Ok(q) => q,
        Err(_) => return Vec::new(),
    };
    q.query_map([], |r| {
        Ok(EndedRun {
            plan: r.get(0)?,
            session: r.get(1)?,
            branch: r.get(2)?,
            ended: r.get(3)?,
        })
    })
    .and_then(|rows| rows.collect::<Result<Vec<_>, _>>())
    .unwrap_or_default()
}

/// Every run with its chunk's uid, oldest first. Empty when no db or tables.
pub fn flow_runs(db: &Path) -> Vec<StatsRun> {
    let Some(c) = readonly(db) else {
        return Vec::new();
    };
    let mut q = match c.prepare(
        "SELECT r.started, r.ended, r.pr, r.branch, c.uid
         FROM run r JOIN chunk c ON c.id = r.chunk ORDER BY r.id",
    ) {
        Ok(q) => q,
        Err(_) => return Vec::new(),
    };
    q.query_map([], |r| {
        Ok(StatsRun {
            started: r.get(0)?,
            ended: r.get(1)?,
            pr: r.get(2)?,
            branch: r.get(3)?,
            chunk: r.get(4)?,
        })
    })
    .and_then(|rows| rows.collect::<Result<Vec<_>, _>>())
    .unwrap_or_default()
}

/// Is `tok` a chunk uid, or the prefix of one? Ledger ids share the shape.
pub fn is_chunk(db: &Path, tok: &str) -> bool {
    let Some(c) = readonly(db) else {
        return false;
    };
    c.query_row(
        "SELECT EXISTS(SELECT 1 FROM chunk WHERE uid LIKE ?1 || '%')",
        [tok.to_uppercase()],
        |r| r.get(0),
    )
    .unwrap_or(false)
}

pub fn brief_row(c: &rusqlite::Connection, cond: &str, key: &str) -> Option<Brief> {
    let sql = format!(
        "SELECT c.uid, c.title, c.brief,
                CASE p.app WHEN '' THEN p.name ELSE p.app || '/' || p.name END,
                p.title
         FROM chunk c JOIN plan p ON p.id = c.plan WHERE {cond} LIMIT 2"
    );
    let mut q = c.prepare(&sql).ok()?;
    let mut rows = q
        .query_map([key], |r| {
            Ok(Brief {
                uid: r.get(0)?,
                title: r.get(1)?,
                brief: r.get(2)?,
                plan: r.get(3)?,
                plan_title: r.get(4)?,
            })
        })
        .ok()?
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    if rows.len() == 1 {
        return rows.pop();
    }
    if rows.is_empty() {
        return None;
    }
    rows.into_iter().find(|b| b.uid == key)
}

/// A chunk uid's brief. `None` when no db, no chunk, or an ambiguous prefix.
pub fn chunk_brief(db: &Path, key: &str) -> Option<Brief> {
    let c = readonly(db)?;
    if let Some(b) = brief_row(&c, "c.uid = ?1 OR c.label = ?1", key)
        && (b.uid == key
            || c.query_row(
                "SELECT uid FROM chunk WHERE label = ?1 LIMIT 2",
                [key],
                |r| r.get::<_, String>(0),
            )
            .ok()
            .is_some())
    {
        return Some(b);
    }
    if key.starts_with('0') && key.chars().all(|x| x.is_ascii_alphanumeric()) && key.len() >= 8 {
        let sql = "SELECT c.uid, c.title, c.brief,
                   CASE p.app WHEN '' THEN p.name ELSE p.app || '/' || p.name END, p.title
                   FROM chunk c JOIN plan p ON p.id = c.plan
                   WHERE c.uid LIKE ?1 || '%' ORDER BY c.uid";
        let mut q = c.prepare(sql).ok()?;
        let rows = q
            .query_map([key.to_uppercase()], |r| {
                Ok(Brief {
                    uid: r.get(0)?,
                    title: r.get(1)?,
                    brief: r.get(2)?,
                    plan: r.get(3)?,
                    plan_title: r.get(4)?,
                })
            })
            .ok()?
            .collect::<Result<Vec<_>, _>>()
            .ok()?;
        if rows.len() == 1 {
            return rows.into_iter().next();
        }
    }
    None
}

pub fn all_plans(c: &rusqlite::Connection) -> Option<Vec<PlanRow>> {
    let mut q = c
        .prepare("SELECT id, app, name, title, area, state, parent FROM plan ORDER BY app, name")
        .ok()?;
    q.query_map([], |r| {
        Ok(PlanRow {
            id: r.get(0)?,
            app: r.get(1)?,
            name: r.get(2)?,
            title: r.get(3)?,
            area: r.get(4)?,
            state: r.get(5)?,
            parent: r.get(6)?,
        })
    })
    .ok()?
    .collect::<Result<Vec<_>, _>>()
    .ok()
}

/// The plan `name` (`name` or `app/name`), if it has one.
pub fn plan_by_name(db: &Path, name: &str) -> Option<PlanRow> {
    all_plans(&readonly(db)?)?.into_iter().find(|p| {
        let key = if p.app.is_empty() {
            p.name.clone()
        } else {
            format!("{}/{}", p.app, p.name)
        };
        key == name || p.name == name
    })
}
