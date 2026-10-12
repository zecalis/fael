use super::*;

#[test]
fn a_newer_epoch_is_refused() {
    let dir = std::env::temp_dir().join(format!("fael-plans-{}", crate::ulid()));
    let path = dir.join("plans.db");
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
    let path = dir.join("plans.db");
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
            "INSERT INTO plan(app, name, title, state, source) VALUES ('', 't', 'T', 'active', '')",
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
