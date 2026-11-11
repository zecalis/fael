//! Notes left for the running chunk (issue 01M6D140): a running agent never
//! sees a `fael chunk note` left for it — no hook reads `by = 'note'` events.
//! Read-only and fail-open, like `scopes`: the notes on this session's live
//! chunk since its run started (`by = 'note'`, the run itself writes
//! `by = 'agent'`), oldest first. Core never depends on fael-crew.

use super::db::readonly;
use std::path::Path;

/// One inbox note: its event id, text and writer session (`None` on old dbs
/// or notes filed outside any session).
pub type Note = (i64, String, Option<String>);

/// The live chunk's uid and its notes since the run started.
/// `None` with no db, no tables or no live run; empty notes = nothing new.
pub fn notes(db: &Path, worktree: &str, session: &str) -> Option<(String, Vec<Note>)> {
    let c = readonly(db)?;
    let (uid, started): (String, String) = c
        .query_row(
            "SELECT c.uid, r.started FROM run r JOIN chunk c ON c.id = r.chunk
             WHERE (r.worktree = ?1 OR (?2 <> '' AND r.session = ?2)) AND r.ended IS NULL
             ORDER BY c.seq LIMIT 1",
            [worktree, session],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .ok()?;
    // event.session rides COLUMNS (no VERSION bump): old dbs lack it — fall back
    // to the session-less read so a shared db opens both ways (issue 01M4MN60).
    let with_session = c
        .prepare(
            "SELECT id, COALESCE(text, ''), session FROM event
             WHERE chunk_uid = ?1 AND by = 'note' AND at >= ?2
               AND COALESCE(text, '') != '' ORDER BY id",
        )
        .ok();
    if let Some(mut q) = with_session {
        let out = q
            .query_map(rusqlite::params![uid, started], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<String>>(2)?,
                ))
            })
            .ok()?
            .collect::<Result<Vec<_>, _>>()
            .ok()?;
        return Some((uid, out));
    }
    let mut q = c
        .prepare(
            "SELECT id, COALESCE(text, '') FROM event
             WHERE chunk_uid = ?1 AND by = 'note' AND at >= ?2
               AND COALESCE(text, '') != '' ORDER BY id",
        )
        .ok()?;
    let out = q
        .query_map(rusqlite::params![uid, started], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, None))
        })
        .ok()?
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    Some((uid, out))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("fael-inbox-{}.db", crate::ulid()));
        let c = rusqlite::Connection::open(&p).unwrap();
        c.execute_batch(
            "CREATE TABLE chunk(id INTEGER PRIMARY KEY, uid TEXT, seq INTEGER);
             CREATE TABLE run(id INTEGER PRIMARY KEY, chunk INTEGER, worktree TEXT,
                              started TEXT, ended TEXT, session TEXT);
             CREATE TABLE event(id INTEGER PRIMARY KEY, chunk_uid TEXT, at TEXT, by TEXT, text TEXT);
             INSERT INTO chunk VALUES (1, 'U1', 1);
             INSERT INTO run VALUES (1, 1, '/w', '2026-01-02T00:00:00Z', NULL, 'S');
             INSERT INTO event VALUES (1, 'U1', '2026-01-01T00:00:00Z', 'note', 'before start'),
                                      (2, 'U1', '2026-01-03T00:00:00Z', 'agent', 'started'),
                                      (3, 'U1', '2026-01-03T00:00:00Z', 'note', 'hello'),
                                      (4, 'U1', '2026-01-04T00:00:00Z', 'note', '');",
        )
        .unwrap();
        drop(c);
        p
    }

    #[test]
    fn notes_since_the_run_started_by_note_only() {
        let p = db();
        assert_eq!(notes(&p, "/none", ""), None, "no live run");
        assert_eq!(notes(&p, "/none", "T"), None, "another session");
        let (uid, ns) = notes(&p, "/w", "").unwrap();
        assert_eq!(uid, "U1");
        assert_eq!(
            ns,
            vec![(3, "hello".to_string(), None)],
            "before start and empty left out, agent start is not a note"
        );
        assert_eq!(
            notes(&p, "/other", "S").map(|r| r.0),
            Some("U1".into()),
            "same session, other worktree"
        );
        let _ = std::fs::remove_file(p);
    }

    #[test]
    fn no_db_is_silent() {
        let none = std::env::temp_dir().join(format!("fael-inbox-{}", crate::ulid()));
        assert_eq!(notes(&none.join("crew.db"), "/w", ""), None);
    }
}
