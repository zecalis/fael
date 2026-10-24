//! Read-only crew db access: fail-open when the file or tables are missing.

use super::{Brief, EndedRun, PlanRow, StatsRun};
use std::path::{Path, PathBuf};

/// The db for a checkout: beside the journal when there is one, else `.fael`.
pub fn db_for(journal: Option<&Path>, fael: &Path) -> PathBuf {
    db_at(journal.unwrap_or(fael))
}

/// `<dir>/crew.db`, else a not-yet-renamed `fael.db`. Names a path and touches nothing:
/// the rename to `crew.db` is crew's (`fael_crew::db_at`), so a hook or `find` stays read-only.
pub fn db_at(dir: &Path) -> PathBuf {
    let (path, old) = (dir.join("crew.db"), dir.join("fael.db"));
    if !path.exists() && old.exists() {
        return old;
    }
    path
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

/// Stamp the model the client reported onto this worktree's live runs still
/// missing one (run-telemetry t4: Claude/Codex transcript tail via the Stop
/// hook). NULL fills only — a start-time `--model`, approval or hint
/// (start.rs COALESCE) always wins, never overwritten here.
pub fn mark_model(db: &Path, worktree: &str, session: &str, model: &str) -> usize {
    if !db.exists() || session.is_empty() || model.is_empty() {
        return 0;
    }
    let Ok(c) = rusqlite::Connection::open(db) else {
        return 0;
    };
    c.execute(
        "UPDATE run SET model = COALESCE(model, ?3)
          WHERE worktree = ?1 AND ended IS NULL AND model IS NULL
            AND (session IS NULL OR session = ?2)",
        rusqlite::params![worktree, session, model],
    )
    .unwrap_or(0)
}

/// Stamp the model onto every live run a session's `chunk start` bound,
/// wherever it works now — the `--no-worktree` half of `mark_model`.
pub fn mark_model_session(db: &Path, session: &str, model: &str) -> usize {
    if !db.exists() || session.is_empty() || model.is_empty() {
        return 0;
    }
    let Ok(c) = rusqlite::Connection::open(db) else {
        return 0;
    };
    c.execute(
        "UPDATE run SET model = COALESCE(model, ?2)
          WHERE session = ?1 AND ended IS NULL AND model IS NULL",
        rusqlite::params![session, model],
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

/// Open chunks linked to ledger row `row_id` — `(uid, state)`, oldest first.
/// Empty when no db, no tables, or none linked. Fail-open: crew owns writes.
pub fn linked_open_chunks(db: &Path, row_id: &str) -> Vec<(String, String)> {
    let Some(c) = readonly(db) else {
        return Vec::new();
    };
    let mut q = match c.prepare(
        "SELECT c.uid, c.state FROM chunk_issue i JOIN chunk c ON c.id = i.chunk
         WHERE i.row_id = ?1 AND c.state NOT IN ('done','dropped','replaced')
         ORDER BY c.id",
    ) {
        Ok(q) => q,
        Err(_) => return Vec::new(),
    };
    q.query_map([row_id], |r| Ok((r.get(0)?, r.get(1)?)))
        .and_then(|rows| rows.collect::<Result<Vec<_>, _>>())
        .unwrap_or_default()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn naming_the_db_touches_nothing() {
        let d = std::env::temp_dir().join(format!("fael-dbat-{}", crate::ulid()));
        std::fs::create_dir_all(&d).unwrap();
        assert_eq!(db_at(&d), d.join("crew.db"));
        std::fs::write(d.join("fael.db"), "x").unwrap();
        std::fs::write(d.join("plans.db"), "x").unwrap();
        assert_eq!(db_at(&d), d.join("fael.db"), "unrenamed: read it as is");
        assert!(d.join("plans.db").exists() && !d.join("crew.db").exists());
        std::fs::write(d.join("crew.db"), "x").unwrap();
        assert_eq!(db_at(&d), d.join("crew.db"));
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn linked_open_chunks_skips_terminal_and_missing() {
        let d = std::env::temp_dir().join(format!("fael-linked-{}", crate::ulid()));
        std::fs::create_dir_all(&d).unwrap();
        let db = d.join("crew.db");
        // missing file: fail-open empty
        assert!(linked_open_chunks(&d.join("nope.db"), "01X").is_empty());
        let c = rusqlite::Connection::open(&db).unwrap();
        c.execute_batch(
            "CREATE TABLE chunk(id INTEGER PRIMARY KEY, uid TEXT, state TEXT);
             CREATE TABLE chunk_issue(chunk INTEGER, row_id TEXT);",
        )
        .unwrap();
        let issue = "01M0000000000000000000001";
        for (id, uid, state) in [
            (1, "01M00000000000000000000A1", "open"),
            (2, "01M00000000000000000000A2", "done"),
            (3, "01M00000000000000000000A3", "running"),
            (4, "01M00000000000000000000A4", "dropped"),
        ] {
            c.execute(
                "INSERT INTO chunk(id, uid, state) VALUES (?1, ?2, ?3)",
                rusqlite::params![id, uid, state],
            )
            .unwrap();
            c.execute(
                "INSERT INTO chunk_issue(chunk, row_id) VALUES (?1, ?2)",
                rusqlite::params![id, issue],
            )
            .unwrap();
        }
        drop(c);
        let got = linked_open_chunks(&db, issue);
        assert_eq!(
            got,
            vec![
                ("01M00000000000000000000A1".to_string(), "open".to_string()),
                (
                    "01M00000000000000000000A3".to_string(),
                    "running".to_string()
                ),
            ],
            "{got:?}"
        );
        assert!(linked_open_chunks(&db, "01M0000000000000000000009").is_empty());
        let _ = std::fs::remove_dir_all(d);
    }
}
