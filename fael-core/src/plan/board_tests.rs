//! One project's board (SPEC §2, §10): derived fields and the md title clean-up.

use super::super::board_math::{clean, overlap, plan_title};
use super::super::chunk::tests::{NOW, add, here, start};
use super::super::{Fields, Store};

fn scoped(s: &mut Store, title: &str, scope: &[&str], after: &[String]) -> String {
    let f = Fields {
        title: Some(title.into()),
        brief: Some("b".into()),
        scope: Some(scope.iter().map(|x| x.to_string()).collect()),
        ..Fields::default()
    };
    s.add("inbox", &f, after, NOW).unwrap()
}

#[test]
fn derived_fields() {
    let mut s = Store::open_in_memory().unwrap();
    let a = scoped(&mut s, "a", &["src/"], &[]);
    let b = scoped(&mut s, "b", &["src/b.rs"], std::slice::from_ref(&a));
    let c = scoped(&mut s, "c", &[], std::slice::from_ref(&b));
    let d = scoped(&mut s, "d", &["src/x.rs"], &[]);
    let idea = add(&mut s, "idea", None);
    start(&mut s, &d, "/wt/1").unwrap();
    s.wait(&d, "owner", "Which tone?", None, &here("/wt/1"))
        .unwrap();
    // a's live run, last seen before the stale line
    start(&mut s, &a, "/wt/2").unwrap();
    let bd = s.board("2026-10-11", "2026-10-11T09:30:00Z").unwrap();
    let by = |u: &str| bd.chunks.iter().find(|c| c.uid == u).unwrap();
    let (ca, cb, cc, cd) = (by(&a), by(&b), by(&c), by(&d));
    assert!(ca.state == "running" && ca.stalled && !ca.ended);
    assert_eq!(ca.unblocks, 2, "b, then c through b");
    assert_eq!(ca.run.as_ref().unwrap().worktree.as_deref(), Some("/wt/2"));
    assert!(!cb.ready && cb.blocked_by[0].uid.as_deref() == Some(a.as_str()));
    assert_eq!(cc.blocked_by[0].what, "b");
    assert!(
        ca.overlaps.is_empty(),
        "b is blocked, d waits: neither is hot"
    );
    assert_eq!(
        cd.wait.as_ref().unwrap().text.as_deref(),
        Some("Which tone?")
    );
    assert_eq!(cd.handoff.as_ref().unwrap().text, "Which tone?");
    assert!(cd.run.as_ref().unwrap().ended.is_some());
    assert_eq!(by(&idea).state, "draft");
    let inbox = &bd.plans[0];
    assert_eq!(inbox.counts["open"], 2);
    assert_eq!(inbox.counts["running"], 1);
    // d answered: ready beside the running a, and `src/` covers `src/x.rs`
    s.owner(&d, super::super::Owner::Answer, Some("warm"), NOW)
        .unwrap();
    let bd = s.board("2026-10-11", "2026-10-11T09:00:00Z").unwrap();
    let by = |u: &str| bd.chunks.iter().find(|c| c.uid == u).unwrap();
    assert!(by(&d).ready && !by(&a).stalled);
    assert_eq!(by(&a).overlaps, vec![d.clone()]);
    assert_eq!(by(&d).overlaps, vec![a]);
}

#[test]
fn scopes_overlap_on_a_segment() {
    let v = |x: &[&str]| x.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert!(overlap(&v(&["src/"]), &v(&["src/a.rs"])));
    assert!(overlap(&v(&["src/a.rs"]), &v(&["src"])));
    assert!(overlap(&v(&["a.rs"]), &v(&["a.rs"])));
    assert!(!overlap(&v(&["src/a"]), &v(&["src/ab.rs"])));
    assert!(!overlap(&v(&[]), &v(&["src/"])));
}

#[test]
fn md_titles_come_clean() {
    assert_eq!(
        plan_title("PLAN-fael-board — fael runs the chunk", "fael-board"),
        "fael runs the chunk"
    );
    assert_eq!(plan_title("PLAN-x (v1): one log", "x"), "one log");
    assert_eq!(plan_title("inbox", "inbox"), "inbox");
    assert_eq!(
        clean(
            "b3b — `fael board` (wip feat/x) (after b2a) more",
            Some("b3b")
        ),
        "`fael board` more"
    );
    assert_eq!(clean("chunk 2 — two (wait owner)", Some("2")), "two");
    assert_eq!(clean("(wipe) stays", None), "(wipe) stays");
}
