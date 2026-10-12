//! Labels address a chunk (issue 01M4N4CVK), `show` reads one (01M4N4CVN), a start binds
//! the caller's session and `beat` keeps a run seen (01M4N4CVC).

use super::super::chunk::tests::{NOW, add, get, here, state};
use super::super::{Fields, Start, Store};

fn labeled(s: &mut Store, plan: &str, l: &str) -> Result<String, String> {
    let f = Fields {
        title: Some(l.into()),
        brief: Some("b".into()),
        label: Some(l.into()),
        ..Fields::default()
    };
    s.add(plan, &f, &[], NOW)
}

#[test]
fn a_label_is_unique_in_its_plan_and_names_the_chunk() {
    let mut s = Store::open_in_memory().unwrap();
    let b10 = labeled(&mut s, "inbox", "b10").unwrap();
    let taken = labeled(&mut s, "inbox", "b10").unwrap_err();
    assert!(taken.contains(&format!("taken by chunk {b10}")), "{taken}");
    for bad in ["two words", "01M4N4CVK"] {
        assert!(labeled(&mut s, "inbox", bad).is_err(), "{bad}");
    }
    // every verb takes the label for the uid, and the event keys on the uid
    s.start("b10", &Start::default(), &here("/w")).unwrap();
    assert_eq!(state(&s, &b10), "running");
    let by = "SELECT COUNT(*) FROM event WHERE chunk_uid = ?1";
    let n: i64 = s.conn.query_row(by, [&b10], |r| r.get(0)).unwrap();
    assert_eq!(n, 2, "add and start");
    // edit: another chunk cannot take it, a free one can, "" clears
    let other = add(&mut s, "x", Some("b"));
    let edit = |l: &str| Fields {
        label: Some(l.into()),
        ..Fields::default()
    };
    assert!(s.edit(&other, &edit("b10"), NOW).is_err());
    s.edit(&other, &edit("b11"), NOW).unwrap();
    s.edit("b11", &edit(""), NOW).unwrap();
    assert_eq!(
        get(&s, "SELECT label FROM chunk WHERE uid = ?1", &other),
        None
    );
}

#[test]
fn a_label_in_two_plans_asks_for_the_uid() {
    let mut s = Store::open_in_memory().unwrap();
    s.conn
        .execute(
            "INSERT INTO plan(app, name, title, state)
             VALUES ('', 'p2', 'p2', 'active')",
            [],
        )
        .unwrap();
    let a = labeled(&mut s, "inbox", "b1").unwrap();
    labeled(&mut s, "p2", "b1").unwrap();
    let e = s.show("b1").unwrap_err();
    assert!(e.contains("more than one chunk"), "{e}");
    assert_eq!(s.show(&a).unwrap().uid, a, "the uid still works");
    // --plan picks the label's plan, and checks a uid it is given
    assert_eq!(s.uid_in("b1", "inbox").unwrap(), a);
    assert_ne!(s.uid_in("b1", "p2").unwrap(), a);
    assert_eq!(s.uid_in(&a, "inbox").unwrap(), a);
    let e = s.uid_in(&a, "p2").unwrap_err();
    assert!(e.contains("is in plan inbox, not p2"), "{e}");
    let e = s.uid_in("b9", "p2").unwrap_err();
    assert!(e.contains("no chunk b9 in plan p2"), "{e}");
    // an `after` label is read in the new chunk's plan first
    let f = Fields {
        title: Some("next".into()),
        ..Fields::default()
    };
    let next = s.add("inbox", &f, &["b1".into()], NOW).unwrap();
    let waits = "SELECT d.uid FROM edge e JOIN chunk s ON s.id = e.src
                 JOIN chunk d ON d.id = e.dst WHERE s.uid = ?1 AND e.kind = 'after'";
    assert_eq!(get(&s, waits, &next).as_deref(), Some(a.as_str()));
}

#[test]
fn show_prints_what_the_next_start_will() {
    let mut s = Store::open_in_memory().unwrap();
    let a = add(&mut s, "a", Some("the brief"));
    s.note(&a, "first", NOW).unwrap();
    assert_eq!(s.show(&a).unwrap().notes, ["first"]);
    s.start(&a, &Start::default(), &here("/w")).unwrap();
    // running: the notes its start printed
    assert_eq!(s.show(&a).unwrap().notes, ["first"]);
    assert_eq!(s.show(&a).unwrap().brief, "the brief");
    assert!(s.show("nope").unwrap_err().contains("no chunk nope"));
}

#[test]
fn a_bound_session_and_a_beat_keep_a_no_worktree_run_seen() {
    let mut s = Store::open_in_memory().unwrap();
    let a = add(&mut s, "a", Some("b"));
    let o = Start {
        no_worktree: true,
        session: Some("s1".into()),
        run: Some("R1".into()),
        ..Start::default()
    };
    s.start(&a, &o, &here("/w")).unwrap();
    let seen = "SELECT r.last_seen FROM run r JOIN chunk c ON c.id = r.chunk WHERE c.uid = ?1";
    assert_eq!(s.seen("/w", "s1", NOW).unwrap(), 0, "no worktree to match");
    assert_eq!(s.seen_session("s2", NOW).unwrap(), 0, "another session");
    assert_eq!(s.seen_session("s1", NOW).unwrap(), 1);
    assert_eq!(get(&s, seen, &a).as_deref(), Some(NOW));
    assert_eq!(s.beat("R1", "2026-10-11T10:00:00Z").unwrap(), 1);
    s.run_end("R1", None, NOW).unwrap();
    assert_eq!(s.seen_session("s1", NOW).unwrap(), 0, "never an ended run");
    assert_eq!(s.beat("R1", NOW).unwrap(), 0);
}

#[test]
fn a_chunk_uid_or_its_prefix_is_known() {
    let mut s = Store::open_in_memory().unwrap();
    let a = add(&mut s, "a", None);
    assert!(s.is_chunk(&a) && s.is_chunk(&a[..10].to_lowercase()));
    assert!(!s.is_chunk("01M4N4CV7"));
}

#[test]
fn scopes_overlap_across_the_plans_of_one_db() {
    let mut s = Store::open_in_memory().unwrap();
    s.conn
        .execute(
            "INSERT INTO plan(app, name, title, state)
             VALUES ('', 'app', 'app', 'active')",
            [],
        )
        .unwrap();
    let scoped = |s: &mut Store, plan: &str| {
        let f = Fields {
            title: Some("t".into()),
            brief: Some("b".into()),
            scope: Some(vec!["src/".into()]),
            ..Fields::default()
        };
        s.add(plan, &f, &[], NOW).unwrap()
    };
    let (a, b) = (scoped(&mut s, "inbox"), scoped(&mut s, "app"));
    let overlaps = |s: &Store, u: &str| {
        let t = s.board("2026-10-11", NOW).unwrap();
        t.chunks.into_iter().find(|c| c.uid == u).unwrap().overlaps
    };
    assert_eq!(
        overlaps(&s, &a),
        std::slice::from_ref(&b),
        "one repo: src/ is the same tree"
    );
}

#[test]
fn a_unique_uid_prefix_of_8_names_the_chunk_for_every_verb() {
    let mut s = Store::open_in_memory().unwrap();
    let (a, b) = (add(&mut s, "a", Some("b")), add(&mut s, "b", Some("b")));
    for (u, to) in [
        (&a, "01AAAAAAAA0000000000000000"),
        (&b, "01AAAAAAAA1000000000000000"),
    ] {
        let q = "UPDATE chunk SET uid = ?1 WHERE uid = ?2";
        s.conn.execute(q, [to, u.as_str()]).unwrap();
    }
    // a hit, any case, and every verb takes it
    assert_eq!(
        s.show("01aaaaaaaa0").unwrap().uid,
        "01AAAAAAAA0000000000000000"
    );
    s.note("01AAAAAAAA0", "n", NOW).unwrap();
    s.start("01AAAAAAAA0", &Start::default(), &here("/w"))
        .unwrap();
    assert_eq!(state(&s, "01AAAAAAAA0000000000000000"), "running");
    // ambiguous: rejected with both uids
    let e = s.show("01AAAAAAAA").unwrap_err();
    assert!(e.contains("names 2 chunks"), "{e}");
    assert!(
        e.contains("01AAAAAAAA0000000000000000, 01AAAAAAAA1000000000000000"),
        "{e}"
    );
    // too short, and a prefix nothing starts with
    assert!(s.show("01AAAAA").unwrap_err().contains("too short"));
    assert!(s.show("01ZZZZZZZZ").unwrap_err().contains("no chunk"));
}
