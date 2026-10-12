use super::*;

#[test]
fn a_newer_epoch_is_refused() {
    let dir = std::env::temp_dir().join(format!("fael-plans-{}", crate::ulid()));
    let path = dir.join("fael.db");
    drop(Store::open(&path).unwrap());
    let c = Connection::open(&path).unwrap();
    c.pragma_update(None, "user_version", 99).unwrap();
    drop(c);
    let e = Store::open(&path).err().unwrap();
    // a dev build names its commit and a newer main, never the release channel
    let fix = if crate::GIT.is_some() {
        "newer main"
    } else {
        "fael upgrade"
    };
    assert!(e.contains("v99") && e.contains(fix), "{e}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn builds_with_different_additions_share_one_db_both_ways() {
    let dir = std::env::temp_dir().join(format!("fael-plans-{}", crate::ulid()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("fael.db");
    const T: &[(&str, &str)] = &[(
        "ask",
        "CREATE TABLE IF NOT EXISTS ask(id INTEGER PRIMARY KEY)",
    )];
    const C: &[(&str, &str, &str)] = &[("run", "beat", "TEXT")];
    let open = |t, c| Store::init_with(Connection::open(&path).unwrap(), t, c);
    // the other lane's build opens first: it adds its table and column, no version bump
    let s = open(T, C).unwrap();
    s.conn
        .execute("INSERT INTO ask(id) VALUES (1)", [])
        .unwrap();
    drop(s);
    let user_version = |s: &Store| -> i32 {
        s.conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap()
    };
    // this build lacks both and still reads and writes
    let s = open(&[], &[]).unwrap();
    assert_eq!(user_version(&s), VERSION);
    s.conn
        .execute(
            "INSERT INTO plan(app, name, title, state) VALUES ('', 't', 'T', 'active')",
            [],
        )
        .unwrap();
    assert_eq!(s.plans().unwrap().len(), 1);
    drop(s);
    // and the other way: reopened by the build that has them, nothing lost or re-added
    let s = open(T, C).unwrap();
    let n: i64 = s
        .conn
        .query_row("SELECT COUNT(*) FROM ask", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 1);
    assert_eq!(super::super::schema::missing(&s.conn, T, C).unwrap(), "");
    drop(s);
    // a db at this epoch made without them gains them on the next open by a build with them
    let other = dir.join("other.db");
    drop(Store::init_with(Connection::open(&other).unwrap(), &[], &[]).unwrap());
    let s = Store::init_with(Connection::open(&other).unwrap(), T, C).unwrap();
    assert_eq!(super::super::schema::missing(&s.conn, T, C).unwrap(), "");
    assert_eq!(user_version(&s), VERSION);
    let _ = std::fs::remove_dir_all(dir);
}

fn scratch() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("fael-rename-{}", crate::ulid()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn version_of(path: &Path) -> i32 {
    Connection::open(path)
        .unwrap()
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap()
}

#[test]
fn plans_db_becomes_fael_db_and_a_stub_refuses_an_older_fael() {
    let dir = scratch();
    let old = dir.join("plans.db");
    let c = Connection::open(&old).unwrap();
    c.execute_batch(include_str!("v6.sql")).unwrap();
    c.query_row("PRAGMA journal_mode=WAL", [], |_| Ok(()))
        .unwrap();
    drop(c);
    let path = db_at(&dir);
    assert_eq!(path, dir.join("fael.db"));
    assert_eq!(version_of(&path), 6, "renamed as is; the open upgrades it");
    assert_eq!(version_of(&old), STUB);
    // what a fael before v8 does with the stub: refuse it, never start an empty db
    let e = Store::open(&old).err().unwrap();
    assert!(e.contains(&format!("schema v{STUB}")), "{e}");
    let s = Store::open(&path).unwrap();
    assert_eq!(s.plans().unwrap().len(), 2);
    drop(s);
    // later calls leave both alone
    assert_eq!(db_at(&dir), path);
    assert_eq!(version_of(&old), STUB);
    // fael.db removed by hand: the stub is never renamed in, a fresh db starts
    std::fs::remove_file(&path).unwrap();
    assert_eq!(db_at(&dir), path);
    assert!(!path.exists());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn no_plans_db_names_fael_db_and_an_unreadable_one_stays_the_db() {
    let dir = scratch();
    assert_eq!(db_at(&dir), dir.join("fael.db"));
    assert!(!dir.join("fael.db").exists(), "naming it creates nothing");
    std::fs::write(dir.join("plans.db"), "not sqlite").unwrap();
    // the open names the error; the next call tries the rename again
    assert_eq!(db_at(&dir), dir.join("plans.db"));
    assert!(Store::open(&db_at(&dir)).is_err());
    let _ = std::fs::remove_dir_all(dir);
}
