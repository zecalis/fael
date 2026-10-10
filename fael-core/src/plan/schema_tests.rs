//! v1 → v2 on a fixture written with b1's schema: every row carried over, a failure
//! leaves v1 for the next open, two first opens migrate once.

use super::super::store::Store;
use rusqlite::Connection;
use std::path::{Path, PathBuf};

/// b1's schema, verbatim — the fixture every v1 → v2 test starts from
const V1: &str = "
CREATE TABLE plan(
  id INTEGER PRIMARY KEY,
  app TEXT NOT NULL,
  name TEXT NOT NULL,
  title TEXT NOT NULL,
  area TEXT NOT NULL DEFAULT 'dev',
  kind TEXT,
  state TEXT NOT NULL CHECK(state IN ('active','blocked','parked','done')),
  rank INTEGER,
  goal TEXT,
  spec TEXT,
  source TEXT NOT NULL,
  UNIQUE(app, name));
CREATE TABLE chunk(
  id INTEGER PRIMARY KEY,
  plan INTEGER NOT NULL REFERENCES plan(id) ON DELETE CASCADE,
  pos INTEGER NOT NULL,
  label TEXT,
  title TEXT NOT NULL,
  state TEXT NOT NULL CHECK(state IN
    ('draft','open','running','waiting','review','done','merged','dropped','parked')),
  brief TEXT NOT NULL DEFAULT '',
  wait_on TEXT,
  wait_text TEXT,
  size TEXT,
  model_hint TEXT,
  scope TEXT,
  due TEXT,
  pin INTEGER,
  into_chunk INTEGER REFERENCES chunk(id) ON DELETE SET NULL,
  UNIQUE(plan, pos));
-- after NULL = a ref that names no chunk: never met, shown as blocked on `ref`
CREATE TABLE dep(
  chunk INTEGER NOT NULL REFERENCES chunk(id) ON DELETE CASCADE,
  after INTEGER REFERENCES chunk(id) ON DELETE SET NULL,
  ref TEXT NOT NULL);
CREATE TABLE pair(
  a INTEGER NOT NULL REFERENCES chunk(id) ON DELETE CASCADE,
  b INTEGER NOT NULL REFERENCES chunk(id) ON DELETE CASCADE);
CREATE TABLE run(
  id INTEGER PRIMARY KEY,
  chunk INTEGER NOT NULL REFERENCES chunk(id) ON DELETE CASCADE,
  client TEXT, session TEXT, model TEXT, worktree TEXT, branch TEXT, pr INTEGER,
  started TEXT NOT NULL, last_seen TEXT, ended TEXT);
-- append-only history of every transition; no FK, so it outlives what it names
CREATE TABLE event(
  id INTEGER PRIMARY KEY,
  chunk INTEGER,
  at TEXT NOT NULL,
  by TEXT NOT NULL,
  from_state TEXT,
  to_state TEXT NOT NULL,
  text TEXT);
CREATE TABLE tr(key TEXT NOT NULL, lang TEXT NOT NULL, text TEXT NOT NULL, PRIMARY KEY(key, lang));
CREATE INDEX chunk_plan ON chunk(plan, pos);
CREATE INDEX dep_chunk ON dep(chunk);
CREATE INDEX run_chunk ON run(chunk);
";

fn v1_db(extra: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("fael-v1-{}", crate::ulid()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("plans.db");
    let c = Connection::open(&path).unwrap();
    c.execute_batch(V1).unwrap();
    c.execute_batch(
        "INSERT INTO plan VALUES (1, '', 't', 'PLAN-t', 'dev', NULL, 'active', NULL, NULL,
           NULL, '.fapony/plan/PLAN-t.md');
         INSERT INTO chunk(id, plan, pos, label, title, state, brief, wait_text, into_chunk)
           VALUES (1, 1, 0, 'a', 'a — old', 'merged', 'a — old', NULL, 3),
                  (2, 1, 1, 'b', 'b — wip', 'running', 'b — wip', NULL, NULL),
                  (3, 1, 2, 'c', 'c — waits', 'waiting', 'c — waits', 'owner', NULL);
         INSERT INTO dep VALUES (3, 2, 'b'), (3, NULL, 'gone:1');
         INSERT INTO pair VALUES (2, 3);
         INSERT INTO run(id, chunk, branch, started) VALUES (1, 2, 'feat/b', 't0');
         INSERT INTO event(chunk, at, by, to_state, text) VALUES (2, 't0', 'import', 'running', 'x');
         PRAGMA user_version = 1;",
    )
    .unwrap();
    c.execute_batch(extra).unwrap();
    path
}

fn version(path: &Path) -> i32 {
    Connection::open(path)
        .unwrap()
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap()
}

fn rows(s: &Store, sql: &str) -> Vec<String> {
    s.conn
        .prepare(sql)
        .unwrap()
        .query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

#[test]
fn v1_rows_carry_over() {
    let path = v1_db("");
    let s = Store::open(&path).unwrap();
    assert_eq!(version(&path), 2);
    assert_eq!(
        rows(
            &s,
            "SELECT label || ' ' || seq || ' ' || state FROM chunk ORDER BY seq"
        ),
        ["a 1024 replaced", "b 2048 running", "c 3072 waiting"]
    );
    let uids = rows(&s, "SELECT uid FROM chunk");
    assert!(uids.iter().all(|u| u.len() == 26), "{uids:?}");
    assert_eq!(
        rows(
            &s,
            "SELECT kind || ' ' || src || '>' || COALESCE(dst, '-') || ' ' || COALESCE(ref, '')
             FROM edge ORDER BY kind, src, dst"
        ),
        [
            "after 3>- gone:1",
            "after 3>2 b",
            "pair 2>3 ",
            "replaced_by 1>3 "
        ]
    );
    assert_eq!(
        rows(&s, "SELECT branch || ' ' || length(start) FROM run"),
        ["feat/b 26"]
    );
    assert_eq!(
        rows(
            &s,
            "SELECT e.to_state FROM event e JOIN chunk c ON c.uid = e.chunk_uid"
        ),
        ["running"]
    );
    // foreign keys are back on after the step
    assert!(s.conn.execute("DELETE FROM plan", []).is_ok());
    assert_eq!(rows(&s, "SELECT uid FROM chunk").len(), 0);
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn a_failed_step_leaves_v1_and_the_next_open_retries() {
    // a stray table named like a v2 one makes CREATE TABLE edge fail mid-step
    let path = v1_db("CREATE TABLE edge(x);");
    assert!(Store::open(&path).is_err());
    assert_eq!(version(&path), 1);
    let c = Connection::open(&path).unwrap();
    let n: i64 = c
        .query_row("SELECT COUNT(*) FROM chunk WHERE pos >= 0", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(n, 3, "v1 rows intact");
    c.execute_batch("DROP TABLE edge").unwrap();
    drop(c);
    Store::open(&path).unwrap();
    assert_eq!(version(&path), 2);
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn two_first_opens_migrate_once() {
    // a race shows on some runs only: 30 rounds make one test run catch it
    for _ in 0..30 {
        two_first_opens_round();
    }
}

fn two_first_opens_round() {
    let path = v1_db("");
    let opens: Vec<_> = (0..2)
        .map(|_| {
            let p = path.clone();
            std::thread::spawn(move || Store::open(&p).map(|s| rows(&s, "SELECT kind FROM edge")))
        })
        .collect();
    for o in opens {
        assert_eq!(o.join().unwrap().unwrap().len(), 4);
    }
    assert_eq!(version(&path), 2);
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}
