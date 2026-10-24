//! Joint start (issue 01M5PB7Z): lane over a dir overlap, exact clash still refuses with the
//! joint command, pair joins the partner's run.

use super::super::chunk::tests::{add, here, start};
use super::super::{Joint, Start, Store};
use rusqlite::params;

const STALE: &str = "2026-10-11T09:00:00Z";

fn scoped(s: &mut Store, title: &str, scope: &str) -> String {
    let c = add(s, title, Some(title));
    s.conn
        .execute(
            "UPDATE chunk SET scope = ?2 WHERE uid = ?1",
            params![c, scope],
        )
        .unwrap();
    c
}

fn with(partner: &str, joint: Joint) -> Start {
    Start {
        with: Some(partner.into()),
        joint: Some(joint),
        stale: STALE.into(),
        ..Start::default()
    }
}

fn col(s: &Store, uid: &str, c: &str) -> Option<String> {
    s.conn
        .query_row(
            &format!("SELECT r.{c} FROM run r JOIN chunk k ON k.id = r.chunk WHERE k.uid = ?1"),
            [uid],
            |r| r.get(0),
        )
        .unwrap()
}

#[test]
fn lane_starts_a_dir_overlap_without_force_and_both_reach_done() {
    let mut s = Store::open_in_memory().unwrap();
    let a = scoped(&mut s, "a", "src/");
    let b = scoped(&mut s, "b", "src/x.rs");
    start(&mut s, &a, "/w1").unwrap();
    let e = start(&mut s, &b, "/w2").unwrap_err();
    assert!(e.contains(&format!("--with {a} --as lane")), "{e}");
    assert!(e.contains("--force"), "{e}");
    s.start(&b, &with(&a, Joint::Lane), &here("/w2")).unwrap();
    assert_eq!(col(&s, &b, "joint").as_deref(), Some("lane"));
    assert_eq!(col(&s, &b, "partner").as_deref(), Some(a.as_str()));
    assert_eq!(col(&s, &a, "joint"), None);
    // a lane's files are no "outside declared scope" against its partner
    let out = s.outside(&b, &["src/y.rs".into()], &[]).unwrap();
    assert!(out.1.iter().all(|(_, o)| o.is_none()), "{:?}", out.1);
    s.done(&a, "h", Some(1), None, &here("/w1")).unwrap();
    s.done(&b, "h", Some(2), None, &here("/w2")).unwrap();
}

#[test]
fn an_exact_clash_refuses_and_prints_the_pair_command() {
    let mut s = Store::open_in_memory().unwrap();
    let a = scoped(&mut s, "a", "src/x.rs");
    let b = scoped(&mut s, "b", "src/x.rs");
    start(&mut s, &a, "/w1").unwrap();
    let e = start(&mut s, &b, "/w2").unwrap_err();
    assert!(e.contains(&format!("--with {a} --as pair")), "{e}");
    let e = s
        .start(&b, &with(&a, Joint::Lane), &here("/w2"))
        .unwrap_err();
    assert!(e.contains("--as pair"), "{e}");
}

#[test]
fn pair_joins_the_partners_run_and_seat() {
    let mut s = Store::open_in_memory().unwrap();
    let a = scoped(&mut s, "a", "src/x.rs");
    let b = scoped(&mut s, "b", "src/x.rs");
    let r = start(&mut s, &a, "/w1").unwrap();
    let st = s.start(&b, &with(&a, Joint::Pair), &here("/w2")).unwrap();
    assert_eq!(st.run, r);
    assert_eq!(col(&s, &b, "worktree").as_deref(), Some("/w1"));
    // a second pair start under the same R is a clear refusal, not a sqlite error
    let e = s
        .start(&b, &with(&a, Joint::Pair), &here("/w2"))
        .unwrap_err();
    assert!(e.contains("already ran under run"), "{e}");
    assert_eq!(s.run_end(&r, None, "2026-10-11T10:00:00Z").unwrap(), 2);
}

#[test]
fn with_needs_a_live_partner_and_a_mode() {
    let mut s = Store::open_in_memory().unwrap();
    let a = scoped(&mut s, "a", "src/");
    let b = scoped(&mut s, "b", "src/x.rs");
    let e = s
        .start(&b, &with(&a, Joint::Lane), &here("/w2"))
        .unwrap_err();
    assert!(e.contains("has no live run"), "{e}");
    let only = Start {
        with: Some(a),
        ..Start::default()
    };
    let e = s.start(&b, &only, &here("/w2")).unwrap_err();
    assert!(e.contains("--with and --as go together"), "{e}");
}
