//! Brief drift: an owner edit of the held chunk's brief, scope or title after
//! its run started. Read-only and fail-open; `fael-crew` reuses `in_conn` so the
//! hook and the chunk commands agree on what drift is.

use rusqlite::Connection;
use std::path::Path;

/// What changed under a running chunk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Drift {
    pub uid: String,
    /// The newest drifting edit event: a later edit is a new drift.
    pub event: i64,
    /// The drifting fields, in first-edited order.
    pub fields: Vec<String>,
}

impl Drift {
    /// The one line the hook and `chunk note|done` say, so they cannot differ.
    pub fn text(&self) -> String {
        format!(
            "fael: chunk {0} brief changed since you started ({1}) — read it: fael chunk show {0}\n",
            self.uid,
            self.fields.join(", ")
        )
    }
}

/// `uid`'s drift since its latest start event (`by = 'agent'`, `to_state = 'running'`),
/// from the `edited <fields>` events `chunk edit` writes. `None` when none touches
/// brief, scope or title, or when no agent ever started it (issue 01M5Z4C2): a planner's
/// edit of an unstarted chunk drifts from no one.
pub fn in_conn(c: &Connection, uid: &str) -> Option<Drift> {
    let mut q = c
        .prepare(
            "SELECT id, text FROM event
             WHERE chunk_uid = ?1 AND by = 'owner' AND text LIKE 'edited %'
               AND id > (SELECT MAX(id) FROM event WHERE chunk_uid = ?1
                         AND by = 'agent' AND to_state = 'running')
             ORDER BY id",
        )
        .ok()?;
    let rows = q
        .query_map([uid], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))
        .ok()?;
    let mut d = Drift {
        uid: uid.into(),
        event: 0,
        fields: vec![],
    };
    for (id, text) in rows.flatten() {
        let what = text["edited ".len()..].split(" · ").next().unwrap_or("");
        let hit: Vec<&str> = what
            .split(", ")
            .filter(|f| matches!(*f, "brief" | "scope" | "title"))
            .collect();
        if hit.is_empty() {
            continue;
        }
        d.event = id;
        for f in hit {
            if !d.fields.iter().any(|x| x == f) {
                d.fields.push(f.into());
            }
        }
    }
    (d.event > 0).then_some(d)
}

/// The drift of the first live chunk run in `worktree` or bound to `session`.
pub fn drift(db: &Path, worktree: &str, session: &str) -> Option<Drift> {
    let c = super::db::readonly(db)?;
    let uid: String = c
        .query_row(
            "SELECT c.uid FROM run r JOIN chunk c ON c.id = r.chunk
             WHERE (r.worktree = ?1 OR (?2 <> '' AND r.session = ?2)) AND r.ended IS NULL ORDER BY c.seq LIMIT 1",
            [worktree, session],
            |r| r.get(0),
        )
        .ok()?;
    in_conn(&c, &uid)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_run_is_found_by_worktree_or_by_its_session() {
        let d = std::env::temp_dir().join(format!("fael-drift-{}", crate::ulid()));
        std::fs::create_dir_all(&d).unwrap();
        let db = d.join("crew.db");
        let c = Connection::open(&db).unwrap();
        c.execute_batch(
            "CREATE TABLE chunk(id INTEGER PRIMARY KEY, uid TEXT, seq INTEGER);
             CREATE TABLE run(id INTEGER PRIMARY KEY, chunk INTEGER, worktree TEXT,
                              ended TEXT, session TEXT);
             CREATE TABLE event(id INTEGER PRIMARY KEY, chunk_uid TEXT, by TEXT, to_state TEXT, text TEXT);
             INSERT INTO chunk VALUES (1, 'U', 1);
             INSERT INTO run VALUES (1, 1, '/a', NULL, 'S');
             INSERT INTO event VALUES (1, 'U', 'agent', 'running', 'started'),
                                      (2, 'U', 'owner', 'running', 'edited brief');",
        )
        .unwrap();
        assert!(drift(&db, "/a", "").is_some(), "worktree");
        assert!(
            drift(&db, "/b", "S").is_some(),
            "same session, other worktree"
        );
        assert!(drift(&db, "/b", "T").is_none(), "another session");
        assert!(
            drift(&db, "/b", "").is_none(),
            "empty session: worktree only"
        );
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn a_chunk_never_started_never_drifts() {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(
            "CREATE TABLE event(id INTEGER PRIMARY KEY, chunk_uid TEXT, by TEXT, to_state TEXT, text TEXT);
             INSERT INTO event VALUES (1, 'U', 'owner', 'open', 'edited brief');",
        )
        .unwrap();
        assert_eq!(in_conn(&c, "U"), None);
    }

    #[test]
    fn only_an_owner_edit_of_brief_scope_or_title_after_the_start_drifts() {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(
            "CREATE TABLE event(id INTEGER PRIMARY KEY, chunk_uid TEXT, by TEXT, to_state TEXT, text TEXT);
             INSERT INTO event VALUES
              (1, 'U', 'owner', 'open', 'edited brief'),
              (2, 'U', 'agent', 'running', 'started'),
              (3, 'U', 'owner', 'running', 'edited size, model_hint'),
              (4, 'U', 'note', 'running', 'edited brief');",
        )
        .unwrap();
        assert_eq!(in_conn(&c, "U"), None, "before start, size, a note");
        c.execute_batch("INSERT INTO event VALUES (5, 'U', 'owner', 'running', 'edited brief, scope · approval cleared')")
            .unwrap();
        let d = in_conn(&c, "U").unwrap();
        assert_eq!(
            (d.event, d.fields),
            (5, vec!["brief".into(), "scope".into()])
        );
    }
}
