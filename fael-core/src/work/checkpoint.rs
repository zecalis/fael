//! The held chunk's last checkpoint note: read-only, fail-open.

use std::path::Path;

/// For the first live chunk run in `worktree` (by `c.seq`): its uid and the latest
/// `note` event since that run started, `(event id, text)`. `None` with no db, no live
/// run or any SQL error.
pub fn last_note(db: &Path, worktree: &str) -> Option<(String, Option<(i64, String)>)> {
    let c = super::db::readonly(db)?;
    let (uid, started): (String, String) = c
        .query_row(
            "SELECT c.uid, r.started FROM run r JOIN chunk c ON c.id = r.chunk
             WHERE r.worktree = ?1 AND r.ended IS NULL ORDER BY c.seq LIMIT 1",
            [worktree],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .ok()?;
    let note = c
        .query_row(
            "SELECT id, COALESCE(text, '') FROM event
             WHERE chunk_uid = ?1 AND by = 'note' AND at >= ?2 ORDER BY id DESC LIMIT 1",
            rusqlite::params![uid, started],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .ok();
    Some((uid, note))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_latest_note_since_the_run_started() {
        let d = std::env::temp_dir().join(format!("fael-ckpt-{}", crate::ulid()));
        std::fs::create_dir_all(&d).unwrap();
        let db = d.join("crew.db");
        assert_eq!(last_note(&db, "/w"), None, "no db");
        let c = rusqlite::Connection::open(&db).unwrap();
        c.execute_batch(
            "CREATE TABLE chunk(id INTEGER PRIMARY KEY, uid TEXT, title TEXT, seq INTEGER);
             CREATE TABLE run(id INTEGER PRIMARY KEY, chunk INTEGER, worktree TEXT,
                              started TEXT, ended TEXT);
             CREATE TABLE event(id INTEGER PRIMARY KEY, chunk_uid TEXT, at TEXT, by TEXT, text TEXT);
             INSERT INTO chunk VALUES (1, 'U1', 't', 2), (2, 'U2', 't', 1);
             INSERT INTO run VALUES (1, 1, '/w', '2026-01-02', NULL), (2, 2, '/other', '2026-01-02', NULL);
             INSERT INTO event VALUES (1, 'U1', '2026-01-01', 'note', 'old run'),
                                      (2, 'U1', '2026-01-03', 'owner', 'not a note');",
        )
        .unwrap();
        assert_eq!(last_note(&db, "/w"), Some(("U1".into(), None)));
        c.execute_batch(
            "INSERT INTO event VALUES (3, 'U1', '2026-01-03', 'note', 'step 1; next 2'),
                                      (4, 'U1', '2026-01-04', 'note', 'finished: done');",
        )
        .unwrap();
        assert_eq!(
            last_note(&db, "/w"),
            Some(("U1".into(), Some((4, "finished: done".into()))))
        );
        assert_eq!(last_note(&db, "/none"), None, "no live run");
        c.execute("UPDATE run SET ended = 'x'", []).unwrap();
        assert_eq!(last_note(&db, "/w"), None, "ended");
        let _ = std::fs::remove_dir_all(d);
    }
}
