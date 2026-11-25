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

/// The live chunk's uid and its notes since the run started, minus the notes
/// written from the run's own worktree: its own checkpoints, which the next
/// `chunk start` brief prints anyway (decision 01M88ZGB).
/// `None` with no db, no tables or no live run; empty notes = nothing new.
pub fn notes(db: &Path, worktree: &str, session: &str) -> Option<(String, Vec<Note>)> {
    let c = readonly(db)?;
    let (uid, started, run_wt): (String, String, Option<String>) = c
        .query_row(
            "SELECT c.uid, r.started, r.worktree FROM run r JOIN chunk c ON c.id = r.chunk
             WHERE (r.worktree = ?1 OR (?2 <> '' AND r.session = ?2)) AND r.ended IS NULL
             ORDER BY c.seq LIMIT 1",
            [worktree, session],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .ok()?;
    // a `--no-worktree` run has none: nothing is its own worktree, every note shows
    let run_wt = run_wt.unwrap_or_default();
    // event.session and event.worktree ride COLUMNS (no VERSION bump): an old db
    // lacks one or both — each read falls back to the next, so a shared db opens
    // every way (issue 01M4MN60). Every variant binds ?3 so one call fits all.
    const READS: [&str; 3] = [
        "SELECT id, COALESCE(text, ''), session FROM event
         WHERE chunk_uid = ?1 AND by = 'note' AND at >= ?2
           AND COALESCE(text, '') != ''
           AND (?3 = '' OR COALESCE(worktree, '') != ?3) ORDER BY id",
        "SELECT id, COALESCE(text, ''), session FROM event
         WHERE chunk_uid = ?1 AND by = 'note' AND at >= ?2
           AND COALESCE(text, '') != '' AND ?3 = ?3 ORDER BY id",
        "SELECT id, COALESCE(text, ''), NULL FROM event
         WHERE chunk_uid = ?1 AND by = 'note' AND at >= ?2
           AND COALESCE(text, '') != '' AND ?3 = ?3 ORDER BY id",
    ];
    let mut q = READS.iter().find_map(|sql| c.prepare(sql).ok())?;
    let out = q
        .query_map(rusqlite::params![uid, started, run_wt], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
            ))
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

    /// A note written from the run's own worktree is its own checkpoint: left
    /// out. Another worktree's (owner, peer) and an unstamped old one stay; a db
    /// with `session` but no `worktree` column still reads, sessions kept.
    #[test]
    fn own_worktree_notes_are_left_out() {
        let p = db();
        let c = rusqlite::Connection::open(&p).unwrap();
        c.execute_batch(
            "ALTER TABLE event ADD COLUMN session TEXT;
             UPDATE event SET session = 'P' WHERE id = 3;",
        )
        .unwrap();
        let (_, ns) = notes(&p, "/w", "").unwrap();
        assert_eq!(
            ns,
            vec![(3, "hello".into(), Some("P".into()))],
            "no worktree column"
        );
        c.execute_batch(
            "ALTER TABLE event ADD COLUMN worktree TEXT;
             UPDATE event SET worktree = '/w' WHERE id = 3;
             INSERT INTO event(id, chunk_uid, at, by, text, worktree) VALUES
               (5, 'U1', '2026-01-05T00:00:00Z', 'note', 'from the owner', '/main'),
               (6, 'U1', '2026-01-05T00:00:00Z', 'note', 'old, unstamped', NULL);",
        )
        .unwrap();
        drop(c);
        let ids: Vec<i64> = notes(&p, "/w", "").unwrap().1.iter().map(|n| n.0).collect();
        assert_eq!(ids, vec![5, 6], "own worktree's note 3 left out");
        let _ = std::fs::remove_file(p);
    }

    #[test]
    fn no_db_is_silent() {
        let none = std::env::temp_dir().join(format!("fael-inbox-{}", crate::ulid()));
        assert_eq!(notes(&none.join("crew.db"), "/w", ""), None);
    }
}
