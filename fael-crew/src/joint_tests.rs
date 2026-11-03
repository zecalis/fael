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
        with: vec![partner.into()],
        joint: Some(joint),
        stale: STALE.into(),
        ..Start::default()
    }
}

fn with_all(partners: &[String], joint: Joint) -> Start {
    Start {
        with: partners.to_vec(),
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
    // no --with, no --force: a dir overlap lanes by itself, the event is the receipt
    start(&mut s, &b, "/w2").unwrap();
    let ev = col_event(&s, &b);
    assert!(ev.contains("lane beside") && ev.contains(&a), "{ev}");
    // a lane's files are no "outside declared scope" against its partner
    let out = s.outside(&b, &["src/y.rs".into()], &[]).unwrap();
    assert!(out.1.iter().all(|(_, o)| o.is_none()), "{:?}", out.1);
    s.done(&a, "h", Some(1), None, &here("/w1")).unwrap();
    s.done(&b, "h", Some(2), None, &here("/w2")).unwrap();
}

#[test]
fn same_scope_with_narrow_touch_lanes_instead_of_pair() {
    use super::super::chunk::tests::here;
    let mut s = Store::open_in_memory().unwrap();
    let a = scoped(&mut s, "a", "src/");
    let b = scoped(&mut s, "b", "src/");
    start(&mut s, &a, "/w1").unwrap();
    // same dir, no touch: exact refuses, names pair
    let e = s
        .start(
            &b,
            &Start {
                stale: STALE.into(),
                ..Start::default()
            },
            &here("/w2"),
        )
        .unwrap_err();
    assert!(e.contains("--as pair"), "{e}");
    // the partner touched one file: the same start lanes beside it, no force
    s.touch(&a, &["src/a.rs".into()], None, &here("/w1"))
        .unwrap();
    s.start(
        &b,
        &Start {
            stale: STALE.into(),
            ..Start::default()
        },
        &here("/w2"),
    )
    .unwrap();
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
        with: vec![a],
        ..Start::default()
    };
    let e = s.start(&b, &only, &here("/w2")).unwrap_err();
    assert!(e.contains("--with and --as go together"), "{e}");
}

#[test]
fn follow_prints_the_read_only_contract_and_the_partner_handoff() {
    let mut s = Store::open_in_memory().unwrap();
    let a = scoped(&mut s, "a", "src/x.rs");
    let b = scoped(&mut s, "b", "src/x.rs");
    start(&mut s, &a, "/w1").unwrap();
    let t = s.follow_text(&a).unwrap();
    assert!(
        t.contains("read-only") && t.contains("handed off nothing yet"),
        "{t}"
    );
    s.note(&a, "api shape is frozen", "2026-10-11T09:30:00Z")
        .unwrap();
    s.start(&b, &with(&a, Joint::Follow), &here("/w2")).unwrap();
    let t = s.follow_text(&a).unwrap();
    assert!(t.contains("> api shape is frozen"), "{t}");
}

#[test]
fn pair_frees_the_callers_seat_but_keeps_a_shared_one() {
    use super::super::chunk::tests::live;
    let mut s = Store::open_in_memory().unwrap();
    let a = scoped(&mut s, "a", "src/x.rs");
    let b = scoped(&mut s, "b", "src/x.rs");
    let c = scoped(&mut s, "c", "other/");
    start(&mut s, &a, "/w1").unwrap();
    start(&mut s, &c, "/w2").unwrap();
    // b joins a's run in /w1; the caller leaves /w2, so its live run ends
    s.start(&b, &with(&a, Joint::Pair), &here("/w2")).unwrap();
    assert_eq!(live(&s, &c), 0, "caller seat /w2 freed");
    assert_eq!(live(&s, &a), 1, "partner run stays live");
    // same worktree both sides: nothing to free, the joined run stays live
    // (an unconditional free would kill the partner run just joined)
    let d = scoped(&mut s, "d", "unrelated/");
    s.start(&d, &with(&a, Joint::Pair), &here("/w1")).unwrap();
    assert_eq!(live(&s, &a), 1, "shared seat keeps the run");
    assert_eq!(live(&s, &d), 1, "joiner runs");
}

fn col_event(s: &Store, uid: &str) -> String {
    s.conn
        .query_row(
            "SELECT text FROM event WHERE chunk_uid = ?1 ORDER BY id DESC LIMIT 1",
            [uid],
            |r| r.get(0),
        )
        .unwrap()
}

#[test]
fn two_dir_overlaps_lane_together_without_with() {
    let mut s = Store::open_in_memory().unwrap();
    let a = scoped(&mut s, "a", "src/");
    let b = scoped(&mut s, "b", "src/y.rs");
    let c = scoped(&mut s, "c", "src/z.rs");
    start(&mut s, &a, "/w1").unwrap();
    start(&mut s, &b, "/w2").unwrap();
    start(&mut s, &c, "/w3").unwrap();
}

#[test]
fn two_exact_clashes_name_both_partners_and_start_with_both() {
    let mut s = Store::open_in_memory().unwrap();
    let a = scoped(&mut s, "a", "src/x.rs");
    let b = scoped(&mut s, "b", "src/y.rs");
    let c = scoped(&mut s, "c", "src/x.rs\nsrc/y.rs");
    start(&mut s, &a, "/w1").unwrap();
    start(&mut s, &b, "/w2").unwrap();
    let e = start(&mut s, &c, "/w3").unwrap_err();
    assert!(
        e.contains(&format!("--with {a}"))
            && e.contains(&format!("--with {b}"))
            && e.contains("--as pair"),
        "{e}"
    );
    // one partner is not enough: the other exact clash still refuses
    let e = s
        .start(&c, &with(&a, Joint::Pair), &here("/w3"))
        .unwrap_err();
    assert!(e.contains(&b), "{e}");
    // both partners at once clears both exact clashes
    s.start(
        &c,
        &with_all(&[a.clone(), b.clone()], Joint::Pair),
        &here("/w3"),
    )
    .unwrap();
    // the first partner's uid stays in run.partner, the rest in run.partners JSON
    assert_eq!(col(&s, &c, "partner").as_deref(), Some(a.as_str()));
    let rest: Vec<String> = serde_json::from_str(
        &s.conn
            .query_row(
                "SELECT r.partners FROM run r JOIN chunk k ON k.id = r.chunk WHERE k.uid = ?1",
                [&c],
                |r| r.get::<_, Option<String>>(0),
            )
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(rest, vec![b.clone()]);
    // board: joint both directions, so no clash is named between c and either partner
    let bd = s.board("2026-10-11", STALE).unwrap();
    let by = |u: &str| bd.chunks.iter().find(|x| x.uid == u).unwrap();
    assert!(
        !by(&c).overlaps.contains(&a) && !by(&c).overlaps.contains(&b),
        "{:?}",
        by(&c).overlaps
    );
    assert!(!by(&a).overlaps.contains(&c), "{:?}", by(&a).overlaps_how);
}

fn stack(partner: &str, contains: bool) -> Start {
    Start {
        with: vec![partner.into()],
        joint: Some(Joint::Stack),
        stale: STALE.into(),
        stack_contains: vec![contains],
        ..Start::default()
    }
}

#[test]
fn stack_keeps_own_run_and_allows_exact_when_head_contains_tip() {
    use super::super::chunk::tests::here_on;
    let mut s = Store::open_in_memory().unwrap();
    let a = scoped(&mut s, "a", "src/x.rs");
    let b = scoped(&mut s, "b", "src/x.rs");
    let r = s
        .start(&a, &Start::default(), &here_on("/w1", "base"))
        .unwrap()
        .run;
    let st = s
        .start(&b, &stack(&a, true), &here_on("/w2", "top"))
        .unwrap();
    assert_ne!(st.run, r, "stack keeps its own R");
    assert_eq!(col(&s, &b, "worktree").as_deref(), Some("/w2"));
    assert_eq!(col(&s, &b, "branch").as_deref(), Some("top"));
    assert_eq!(col(&s, &b, "joint").as_deref(), Some("stack"));
    assert_eq!(col(&s, &b, "partner").as_deref(), Some(a.as_str()));
    let ev = col_event(&s, &b);
    assert!(ev.contains("joint stack with") && ev.contains(&a), "{ev}");
    // board: joint both directions, so no clash is named between the stack and its partner
    let bd = s.board("2026-10-11", STALE).unwrap();
    let by = |u: &str| bd.chunks.iter().find(|x| x.uid == u).unwrap();
    assert!(!by(&b).overlaps.contains(&a), "{:?}", by(&b).overlaps);
    assert!(!by(&a).overlaps.contains(&b), "{:?}", by(&a).overlaps);
}

#[test]
fn stack_rejects_when_head_missing_partner_tip() {
    use super::super::chunk::tests::here_on;
    let mut s = Store::open_in_memory().unwrap();
    let a = scoped(&mut s, "a", "src/x.rs");
    let b = scoped(&mut s, "b", "src/x.rs");
    s.start(&a, &Start::default(), &here_on("/w1", "base"))
        .unwrap();
    let e = s
        .start(&b, &stack(&a, false), &here_on("/w2", "top"))
        .unwrap_err();
    assert!(e.contains("stacks on") && e.contains(&a), "{e}");
    assert!(e.contains("does not contain") && e.contains("base"), "{e}");
    assert!(
        e.contains("git switch -c top base"),
        "ready switch line: {e}"
    );
    assert_eq!(
        s.conn
            .query_row(
                "SELECT COUNT(*) FROM run WHERE chunk = (SELECT id FROM chunk WHERE uid = ?1)",
                [&b],
                |r| r.get::<_, i64>(0),
            )
            .unwrap(),
        0,
        "a rejected stack writes no run"
    );
}
