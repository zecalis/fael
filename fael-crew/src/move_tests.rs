//! `chunk move`: uid, runs, notes and edges stay; running, a closed target and a label
//! clash are refused.

use super::super::chunk::tests::{NOW, add, get, start, state};
use super::super::{Fields, Owner, Store};

fn plan(s: &mut Store, name: &str) -> i64 {
    s.new_plan("", name, name, "dev", None, NOW).unwrap();
    s.plans()
        .unwrap()
        .into_iter()
        .find(|p| p.name == name)
        .unwrap()
        .id
}

fn count(s: &Store, sql: &str, uid: &str) -> i64 {
    s.conn.query_row(sql, [uid], |r| r.get(0)).unwrap()
}

#[test]
fn move_keeps_uid_runs_notes_and_edges() {
    let mut s = Store::open_in_memory().unwrap();
    let p2 = plan(&mut s, "p2");
    let tail = s
        .add(
            "p2",
            &Fields {
                title: Some("already there".into()),
                ..Fields::default()
            },
            &[],
            NOW,
        )
        .unwrap();
    let a = add(&mut s, "a", Some("do a"));
    start(&mut s, &a, "/w").unwrap();
    let f = Fields {
        title: Some("b".into()),
        brief: Some("after a".into()),
        ..Fields::default()
    };
    s.add("inbox", &f, std::slice::from_ref(&a), NOW).unwrap();
    let e = s.move_to(&a, "p2", NOW).unwrap_err();
    assert!(e.contains("is running"), "{e}");
    s.owner(&a, Owner::Park, Some("later"), NOW).unwrap();
    s.note(&a, "a word for the next start", NOW).unwrap();
    let inbox = s
        .plans()
        .unwrap()
        .into_iter()
        .find(|p| p.name == "inbox")
        .unwrap()
        .id;
    let rev = |s: &Store, plan: i64| super::super::export::rev(&s.conn, plan).unwrap();
    let inbox_rev = rev(&s, inbox);

    assert_eq!(s.move_to(&a, "p2", NOW).unwrap(), "inbox");
    let pid = "SELECT plan FROM chunk WHERE uid = ?1";
    assert_eq!(count(&s, pid, &a), p2, "the chunk is in p2, same uid");
    let seq = "SELECT seq FROM chunk WHERE uid = ?1";
    assert_eq!(
        count(&s, seq, &a),
        count(&s, seq, &tail) + 1024,
        "at the end"
    );
    let runs = "SELECT COUNT(*) FROM run r JOIN chunk c ON c.id = r.chunk WHERE c.uid = ?1";
    assert_eq!(count(&s, runs, &a), 1, "its run stays");
    let edges = "SELECT COUNT(*) FROM edge e JOIN chunk d ON d.id = e.dst
                 WHERE d.uid = ?1 AND e.kind = 'after'";
    assert_eq!(count(&s, edges, &a), 1, "b's after edge on it stays");
    assert_eq!(s.show(&a).unwrap().notes, ["a word for the next start"]);
    assert_eq!(
        get(
            &s,
            "SELECT text FROM event WHERE chunk_uid = ?1 ORDER BY id DESC LIMIT 1",
            &a
        )
        .as_deref(),
        Some("moved from inbox to p2")
    );
    assert_eq!(state(&s, &a), "parked", "its state stays");
    assert!(rev(&s, inbox) > inbox_rev, "the old plan's rev moved");
    let e = s.move_to(&a, "p2", NOW).unwrap_err();
    assert!(e.contains("already in p2"), "{e}");
}

#[test]
fn a_closed_target_another_db_and_a_label_clash_are_refused() {
    let mut s = Store::open_in_memory().unwrap();
    let p2 = plan(&mut s, "p2");
    plan(&mut s, "gone");
    let id = s
        .plans()
        .unwrap()
        .into_iter()
        .find(|p| p.name == "gone")
        .unwrap()
        .id;
    s.plan_state(id, "done", NOW).unwrap();
    let labeled = |s: &mut Store, plan: &str| {
        let f = Fields {
            title: Some("t".into()),
            brief: Some("b".into()),
            label: Some("b1".into()),
            ..Fields::default()
        };
        s.add(plan, &f, &[], NOW).unwrap()
    };
    let a = labeled(&mut s, "inbox");
    labeled(&mut s, "p2");
    let e = s.move_to(&a, "p2", NOW).unwrap_err();
    assert!(
        e.contains("label b1 is taken") && e.contains(&format!("fael chunk edit {a} --label")),
        "{e}"
    );
    let e = s.move_to(&a, "gone", NOW).unwrap_err();
    assert!(e.contains("plan gone is done"), "{e}");
    s.plan_state(p2, "parked", NOW).unwrap();
    let e = s.move_to(&a, "p2", NOW).unwrap_err();
    assert!(e.contains("plan p2 is parked"), "{e}");
    let e = s.move_to(&a, "other-repo", NOW).unwrap_err();
    assert!(e.contains("no plan 'other-repo'"), "{e}");
    let pid = "SELECT p.name FROM chunk c JOIN plan p ON p.id = c.plan WHERE c.uid = ?1";
    assert_eq!(
        get(&s, pid, &a).as_deref(),
        Some("inbox"),
        "every refusal left it"
    );
}
