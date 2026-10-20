//! v2, v5, v6 → v8 on a db with fael's own v6 schema (`v6.sql`): every row kept, the md
//! era's columns and tables gone, a `plan.doc` cut into sections first when needed.

use super::super::store::Store;
use super::tests::{baks, db_id, rows, version};
use rusqlite::Connection;
use std::path::{Path, PathBuf};

/// A db at v6 as fael's own was on 2026-10-12, made-up rows in every column v8 drops.
fn v6_db(then: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("fael-v6-{}", fael_core::ulid()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("crew.db");
    let c = Connection::open(&path).unwrap();
    c.execute_batch(include_str!("v6.sql")).unwrap();
    c.execute_batch(then).unwrap();
    path
}

fn columns(s: &Store, table: &str) -> Vec<String> {
    rows(s, &format!("SELECT name FROM pragma_table_info('{table}')"))
}

#[test]
fn v6_drops_the_md_era_and_keeps_every_row() {
    let path = v6_db("");
    let s = Store::open(&path).unwrap();
    assert_eq!(version(&path), 8);
    assert_eq!(
        columns(&s, "plan"),
        ["id", "app", "name", "title", "area", "state", "parent"]
    );
    let chunk = columns(&s, "chunk");
    for gone in ["due", "pin"] {
        assert!(!chunk.contains(&gone.to_string()), "{chunk:?}");
    }
    // the epoch dropped v6's client_hint; COLUMNS adds it back empty (issue 01M4XM98)
    assert_eq!(
        rows(&s, "SELECT uid FROM chunk WHERE client_hint IS NOT NULL"),
        Vec::<String>::new()
    );
    for kept in ["reads", "wait_options", "approved_client", "scope"] {
        assert!(chunk.contains(&kept.to_string()), "{chunk:?}");
    }
    assert_eq!(
        rows(
            &s,
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name IN ('tr', 'contract')"
        ),
        Vec::<String>::new()
    );
    assert_eq!(
        rows(
            &s,
            "SELECT uid || ' ' || state || ' ' || COALESCE(approved_model, '-') FROM chunk ORDER BY seq"
        ),
        [
            "01M4AAAAAAAAAAAAAAAAAAAAAA done opus",
            "01M4BBBBBBBBBBBBBBBBBBBBBB open -"
        ]
    );
    // edge.ref, run.cost and run.out hold rows: kept
    assert_eq!(rows(&s, "SELECT ref FROM edge"), ["section:1"]);
    assert_eq!(
        rows(&s, "SELECT out || ' ' || cost FROM run"),
        ["/out/a 0.5"]
    );
    assert_eq!(rows(&s, "SELECT row_id FROM chunk_issue").len(), 1);
    assert_eq!(rows(&s, "SELECT body FROM section"), ["ship"]);
    assert_eq!(db_id(&path), "01M4DBDBDBDBDBDBDBDBDBDBDB");
    assert_eq!(
        rows(&s, "SELECT value FROM meta WHERE key = 'reads'"),
        ["8"]
    );
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn a_v6_db_before_sections_cuts_its_doc_before_the_drop() {
    let path = v6_db("DROP TABLE section;");
    let s = Store::open(&path).unwrap();
    assert_eq!(
        rows(&s, "SELECT key || ' ' || title || ' ' || body FROM section"),
        ["1 1. Goal ship"]
    );
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn v2_gains_run_cost_and_lands_at_v8() {
    let path = v6_db("");
    to_v5(
        &path,
        "ALTER TABLE run DROP COLUMN cost; ALTER TABLE chunk DROP COLUMN client_hint;
         ALTER TABLE plan DROP COLUMN repo; PRAGMA user_version = 2;",
    );
    let s = Store::open(&path).unwrap();
    assert_eq!(version(&path), 8);
    assert_eq!(
        rows(&s, "SELECT COALESCE(cost, '-') || ' ' || branch FROM run"),
        ["- feat/a"]
    );
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

/// Undo v5 → v6 on a v6 db (`section` came with v6 too), then run `then`.
fn to_v5(path: &Path, then: &str) {
    Connection::open(path)
        .unwrap()
        .execute_batch(&format!(
            "DROP INDEX event_plan; ALTER TABLE event DROP COLUMN plan;
             ALTER TABLE event DROP COLUMN batch; DROP TABLE meta; DROP TABLE section;
             ALTER TABLE plan RENAME COLUMN doc TO goal; PRAGMA user_version = 5; {then}"
        ))
        .unwrap();
}

#[test]
fn v5_gains_event_plan_batch_a_stable_db_id_and_its_goal_as_sections() {
    let path = v6_db("");
    to_v5(
        &path,
        "UPDATE plan SET goal = '## Why
because' WHERE id = 1;
         INSERT INTO event(chunk_uid, at, by, to_state)
           VALUES ('plan:/app', 't1', 'owner', 'parked'), ('gone', 't1', 'import', 'open');",
    );
    let s = Store::open(&path).unwrap();
    assert_eq!(version(&path), 8);
    assert_eq!(
        rows(&s, "SELECT title || ' ' || body FROM section"),
        ["Why because"]
    );
    assert_eq!(
        rows(
            &s,
            "SELECT chunk_uid || ' ' || COALESCE(plan || '', '-') || ' ' || COALESCE(batch, '-')
             FROM event WHERE chunk_uid IN ('plan:/app', 'gone') AND at = 't1' ORDER BY id"
        ),
        ["plan:/app 1 -", "gone - -"]
    );
    assert_eq!(
        rows(
            &s,
            "SELECT COALESCE(plan || '', '-') FROM event WHERE to_state = 'done'"
        ),
        ["1"]
    );
    let id = db_id(&path);
    assert_eq!(id.len(), 26);
    drop(s);
    drop(Store::open(&path).unwrap());
    assert_eq!(db_id(&path), id, "reopen keeps the id");
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn a_v6_upgrade_leaves_one_backup_at_v6() {
    let path = v6_db("");
    for _ in 0..2 {
        drop(Store::open(&path).unwrap());
    }
    let six = baks(&path, 6);
    assert_eq!(six.len(), 1, "one upgrade, one copy: {six:?}");
    assert_eq!(version(&six[0]), 6);
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}
