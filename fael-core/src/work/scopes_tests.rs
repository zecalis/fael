use super::*;

fn db(rows: &str) -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!("fael-scopes-{}.db", crate::ulid()));
    let c = rusqlite::Connection::open(&p).unwrap();
    c.execute_batch(&format!(
        "CREATE TABLE plan(id INTEGER PRIMARY KEY, name TEXT);
         CREATE TABLE chunk(id INTEGER PRIMARY KEY, uid TEXT, label TEXT, plan INTEGER,
           seq INTEGER, scope TEXT, state TEXT);
         CREATE TABLE run(id INTEGER PRIMARY KEY, chunk INTEGER, worktree TEXT, ended TEXT,
           session TEXT, branch TEXT, touched TEXT, touch_msg TEXT, started TEXT, last_seen TEXT);
         CREATE TABLE edge(src INTEGER, dst INTEGER, kind TEXT);
         INSERT INTO plan VALUES (1, 'p');
         {rows}"
    ))
    .unwrap();
    p
}

fn other(uid: &str, scope: &[&str]) -> Other {
    Other {
        uid: uid.into(),
        name: format!("p/{uid}"),
        state: "running".into(),
        scope: scope.iter().map(|s| s.to_string()).collect(),
        branch: None,
        doing: None,
        since: None,
        last_seen: None,
        touched: Vec::new(),
    }
}

#[test]
fn reads_held_scope_and_other_unfinished_scopes() {
    let p = db(
        "INSERT INTO chunk VALUES (1,'U1','a',1,1,'src/a/','running'),
          (2,'U2','b',1,2,'src/b/\nshared/x','ready'),
          (3,'U3','c',1,3,'src/c/','done'),
          (4,'U4',NULL,1,4,'src/d/','ready'),
          (5,'U5','e',1,5,'src/e/','review');
         INSERT INTO run VALUES
           (10,1,'/wt',NULL,'S',NULL,NULL,NULL,'2026-10-11T09:00:00Z',NULL),
           (30,3,'/wt','2026-01-01',NULL,NULL,NULL,NULL,'2026-10-11T08:00:00Z',NULL);
         INSERT INTO edge VALUES (1,5,'pair');",
    );
    let s = scopes(&p, "/wt", "", &["shared/".into()]).unwrap();
    assert_eq!((s.uid.as_str(), s.declared.len()), ("U1", 2));
    let names: Vec<&str> = s.others.iter().map(|o| o.name.as_str()).collect();
    assert_eq!(
        names,
        ["p/b", "p/U4"],
        "done, own and pair partner left out"
    );
    assert_eq!(s.others[0].scope, ["src/b/"], "shared path dropped");
    assert_eq!(s.others[0].state, "ready", "state as stored");
    assert!(
        s.others.iter().all(|o| o.touched.is_empty()),
        "none touched"
    );
    assert!(scopes(&p, "/other", "", &[]).is_none());
    assert!(scopes(&p, "/other", "T", &[]).is_none(), "another session");
    assert_eq!(
        scopes(&p, "/other", "S", &[]).map(|s| s.uid),
        Some("U1".into()),
        "same session, other worktree"
    );
    let _ = std::fs::remove_file(p);
}

#[test]
fn reads_a_live_partner_touch_with_branch_doing_and_since() {
    let p = db(
        "INSERT INTO chunk VALUES (1,'U1','a',1,1,'src/a/','running'),
          (2,'U2','b',1,2,'src/b/','running');
         INSERT INTO run VALUES
           (10,1,'/w1',NULL,'S1','mine','src/a/x.rs','wiring mine','2026-10-11T09:00:00Z',NULL),
           (20,2,'/w2',NULL,'S2','feat-x','src/a/x.rs','wiring auth','2026-10-11T09:05:00Z','2026-10-11T09:06:00Z');",
    );
    let s = scopes(&p, "/w1", "", &[]).unwrap();
    assert_eq!(s.others.len(), 1);
    let o = &s.others[0];
    assert_eq!(o.touched, ["src/a/x.rs"]);
    assert_eq!(o.branch.as_deref(), Some("feat-x"));
    assert_eq!(o.doing.as_deref(), Some("wiring auth"));
    assert_eq!(o.since.as_deref(), Some("2026-10-11T09:05:00Z"));
    assert_eq!(o.last_seen.as_deref(), Some("2026-10-11T09:06:00Z"));
    assert_eq!(
        s.touched_overlap("src/a/x.rs", &[]).map(|o| o.uid.as_str()),
        Some("U2"),
        "touched fires inside my scope"
    );
    // the shared filter drops a touched path nobody owns
    let s = scopes(&p, "/w1", "", &["src/a/".into()]).unwrap();
    assert_eq!(s.others[0].touched, Vec::<String>::new());
    assert!(
        s.touched_overlap("src/a/x.rs", &["src/a/".into()])
            .is_none()
    );
    let _ = std::fs::remove_file(p);
}

#[test]
fn a_db_without_touch_columns_reads_scope_only() {
    let p = std::env::temp_dir().join(format!("fael-scopes-{}.db", crate::ulid()));
    let c = rusqlite::Connection::open(&p).unwrap();
    c.execute_batch(
        "CREATE TABLE plan(id INTEGER PRIMARY KEY, name TEXT);
         CREATE TABLE chunk(id INTEGER PRIMARY KEY, uid TEXT, label TEXT, plan INTEGER,
           seq INTEGER, scope TEXT, state TEXT);
         CREATE TABLE run(chunk INTEGER, worktree TEXT, ended TEXT, session TEXT);
         CREATE TABLE edge(src INTEGER, dst INTEGER, kind TEXT);
         INSERT INTO plan VALUES (1, 'p');
         INSERT INTO chunk VALUES (1,'U1','a',1,1,'src/a/','running'),
           (2,'U2','b',1,2,'src/b/','running');
         INSERT INTO run VALUES (1,'/w1',NULL,'S1'),(2,'/w2',NULL,'S2');",
    )
    .unwrap();
    drop(c);
    let s = scopes(&p, "/w1", "", &[]).unwrap();
    assert_eq!(s.others[0].scope, ["src/b/"]);
    assert!(s.others[0].touched.is_empty(), "no column, no touch");
    assert_eq!(
        s.scope_overlap("src/b/x.rs", &[]).map(|o| o.uid.as_str()),
        Some("U2")
    );
    assert!(s.touched_overlap("src/b/x.rs", &[]).is_none());
    let _ = std::fs::remove_file(p);
}

#[test]
fn overlap_names_only_an_open_owner_outside_my_scope() {
    let s = Scopes {
        uid: "U1".into(),
        declared: vec!["src/a/".into()],
        others: vec![Other {
            uid: "U2".into(),
            name: "p/b".into(),
            state: "open".into(),
            scope: vec!["src/b/".into()],
            branch: None,
            doing: None,
            since: None,
            last_seen: None,
            touched: Vec::new(),
        }],
    };
    assert_eq!(
        s.overlap("src/b/x.rs", &[]).map(|o| o.uid.as_str()),
        Some("U2")
    );
    assert_eq!(
        s.scope_overlap("src/b/x.rs", &[]).map(|o| o.uid.as_str()),
        Some("U2")
    );
    assert!(
        s.touched_overlap("src/b/x.rs", &[]).is_none(),
        "none touched"
    );
    assert!(s.overlap("src/a/x.rs", &[]).is_none(), "in scope");
    assert!(s.overlap("src/z.rs", &[]).is_none(), "unowned");
    assert!(
        s.overlap("src/b/x.rs", &["src/b/".into()]).is_none(),
        "shared"
    );
    let none = Scopes {
        declared: vec![],
        ..s
    };
    assert!(
        none.overlap("src/b/x.rs", &[]).is_none(),
        "no declared scope"
    );
}

#[test]
fn touched_overlap_fires_inside_my_scope_and_wins() {
    let mut o = other("U2", &["src/b/"]);
    o.branch = Some("feat-x".into());
    o.doing = Some("wiring auth".into());
    o.since = Some("2026-10-11T09:05:00Z".into());
    o.touched = vec!["src/a/x.rs".into()];
    let s = Scopes {
        uid: "U1".into(),
        declared: vec!["src/a/".into()],
        others: vec![o],
    };
    assert_eq!(
        s.touched_overlap("src/a/x.rs", &[]).map(|o| o.uid.as_str()),
        Some("U2"),
        "inside my scope still names the toucher"
    );
    assert_eq!(
        s.overlap("src/a/x.rs", &[]).map(|o| o.uid.as_str()),
        Some("U2"),
        "touched wins over in-scope silence"
    );
    assert!(s.scope_overlap("src/a/x.rs", &[]).is_none(), "in scope");
    assert!(s.touched_overlap("src/a/other.rs", &[]).is_none());
    assert!(
        s.touched_overlap("src/a/x.rs", &["src/a/".into()])
            .is_none(),
        "shared"
    );
    assert!(
        s.touched_overlap("docs/a/x.rs", &[]).is_none(),
        "docs is nobody's"
    );
    let none = Scopes {
        uid: "U1".into(),
        declared: vec![],
        others: vec![other("U2", &["src/b/"])],
    };
    assert!(
        none.touched_overlap("src/a/x.rs", &[]).is_none(),
        "no declared scope"
    );
}
