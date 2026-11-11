//! The held chunk's last checkpoint note: read-only, fail-open.

use std::path::Path;

/// One checkpoint note: its event id, text, `at` timestamp, the run holder's
/// client (`""` when the run never reported one) and the writer session
/// (`None` on old dbs or notes filed outside any session).
pub type LastNote = (i64, String, String, String, Option<String>);

/// For the first live chunk run in `worktree` or bound to `session` (by `c.seq`): its uid and the latest
/// `note` event since that run started, `(event id, text, at, client, writer session)`.
/// `None` with no db, no live run or any SQL error.
pub fn last_note(db: &Path, worktree: &str, session: &str) -> Option<(String, Option<LastNote>)> {
    let c = super::db::readonly(db)?;
    let (uid, started, client): (String, String, String) = c
        .query_row(
            "SELECT c.uid, r.started, COALESCE(r.client, '') FROM run r JOIN chunk c ON c.id = r.chunk
             WHERE (r.worktree = ?1 OR (?2 <> '' AND r.session = ?2)) AND r.ended IS NULL ORDER BY c.seq LIMIT 1",
            [worktree, session],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .or_else(|_| {
            c.query_row(
                "SELECT c.uid, r.started FROM run r JOIN chunk c ON c.id = r.chunk
                 WHERE (r.worktree = ?1 OR (?2 <> '' AND r.session = ?2)) AND r.ended IS NULL ORDER BY c.seq LIMIT 1",
                [worktree, session],
                |r| {
                    let (uid, started): (String, String) = (r.get(0)?, r.get(1)?);
                    Ok((uid, started, String::new()))
                },
            )
        })
        .ok()?;
    // event.session rides COLUMNS (no VERSION bump): old dbs lack it — fall back
    // to the session-less read so a shared db opens both ways (issue 01M4MN60).
    if let Ok(mut q) = c.prepare(
        "SELECT id, COALESCE(text, ''), at, session FROM event
         WHERE chunk_uid = ?1 AND by = 'note' AND at >= ?2
           AND COALESCE(text, '') != '' ORDER BY id DESC LIMIT 1",
    ) {
        let note = q
            .query_row(rusqlite::params![uid, started], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    client.clone(),
                    r.get::<_, Option<String>>(3)?,
                ))
            })
            .ok();
        return Some((uid, note));
    }
    let note = c
        .query_row(
            "SELECT id, COALESCE(text, ''), at FROM event
             WHERE chunk_uid = ?1 AND by = 'note' AND at >= ?2
               AND COALESCE(text, '') != '' ORDER BY id DESC LIMIT 1",
            rusqlite::params![uid, started],
            |r| {
                let (id, text, at): (i64, String, String) = (r.get(0)?, r.get(1)?, r.get(2)?);
                Ok((id, text, at, client.clone(), None))
            },
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
        assert_eq!(last_note(&db, "/w", ""), None, "no db");
        let c = rusqlite::Connection::open(&db).unwrap();
        c.execute_batch(
            "CREATE TABLE chunk(id INTEGER PRIMARY KEY, uid TEXT, title TEXT, seq INTEGER);
             CREATE TABLE run(id INTEGER PRIMARY KEY, chunk INTEGER, worktree TEXT,
                              started TEXT, ended TEXT, session TEXT, client TEXT);
             CREATE TABLE event(id INTEGER PRIMARY KEY, chunk_uid TEXT, at TEXT, by TEXT, text TEXT, session TEXT);
             INSERT INTO chunk VALUES (1, 'U1', 't', 2), (2, 'U2', 't', 1);
             INSERT INTO run VALUES (1, 1, '/w', '2026-01-02', NULL, 'S', 'claude'), (2, 2, '/other', '2026-01-02', NULL, NULL, NULL);
             INSERT INTO event VALUES (1, 'U1', '2026-01-01', 'note', 'old run', NULL),
                                      (2, 'U1', '2026-01-03', 'owner', 'not a note', NULL);",
        )
        .unwrap();
        assert_eq!(last_note(&db, "/w", ""), Some(("U1".into(), None)));
        c.execute_batch(
            "INSERT INTO event VALUES (3, 'U1', '2026-01-03', 'note', 'step 1; next 2', 'other'),
                                      (4, 'U1', '2026-01-04', 'note', 'finished: done', 'other'),
                                      (5, 'U1', '2026-01-05', 'note', '', 'other');",
        )
        .unwrap();
        assert_eq!(
            last_note(&db, "/w", ""),
            Some((
                "U1".into(),
                Some((
                    4,
                    "finished: done".into(),
                    "2026-01-04".into(),
                    "claude".into(),
                    Some("other".into())
                ))
            )),
            "latest non-empty note with its time, the run client and writer session"
        );
        assert_eq!(last_note(&db, "/none", ""), None, "no live run");
        assert_eq!(last_note(&db, "/none", "T"), None, "another session");
        assert_eq!(
            last_note(&db, "/none", "S").map(|r| r.0),
            Some("U1".into()),
            "same session, another worktree"
        );
        c.execute("UPDATE run SET ended = 'x'", []).unwrap();
        assert_eq!(last_note(&db, "/w", ""), None, "ended");
        let _ = std::fs::remove_dir_all(d);
    }
}
