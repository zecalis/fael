//! One project's board (SPEC §2, §10): derived fields and the md title clean-up.

use super::super::board_math::{clean, overlap, plan_title};
use super::super::chunk::tests::{NOW, add, here, start};
use super::super::{ALL, Fields, Owner, Store};
use rusqlite::params;

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
    s.wait(&d, "owner", "Which tone?", None, &[], &here("/wt/1"))
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
fn a_review_chunk_still_holds_its_scope() {
    let mut s = Store::open_in_memory().unwrap();
    let pr = scoped(&mut s, "pr", &["src/"], &[]);
    let next = scoped(&mut s, "next", &["src/a.rs"], &[]);
    start(&mut s, &pr, "/wt/1").unwrap();
    s.done(&pr, "h", None, None, &here("/wt/1")).unwrap();
    let bd = s.board("2026-10-11", NOW).unwrap();
    let by = |u: &str| bd.chunks.iter().find(|c| c.uid == u).unwrap();
    assert_eq!(by(&pr).state, "review");
    assert_eq!(by(&next).overlaps, vec![pr.clone()]);
    assert_eq!(by(&pr).overlaps, vec![next]);
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

#[test]
fn owner_actions_are_the_verbs_that_succeed() {
    for st in ALL {
        let (mut listed, mut ok) = (None, Vec::new());
        for o in Owner::ALL {
            let mut s = Store::open_in_memory().unwrap();
            let c = add(&mut s, "c", Some("b"));
            s.conn
                .execute(
                    "UPDATE chunk SET state = ?1 WHERE uid = ?2",
                    params![st.as_str(), c],
                )
                .unwrap();
            let b = s.board("2026-10-11", NOW).unwrap();
            let ch = b.chunks.iter().find(|x| x.uid == c).unwrap();
            listed.get_or_insert(ch.owner_actions.clone());
            if s.owner(&c, o, Some("x"), NOW).is_ok() {
                ok.push(o.as_str());
            }
        }
        assert_eq!(listed.unwrap(), ok, "{}", st.as_str());
    }
}

#[test]
fn done_keeps_the_branch_the_agent_switched_to() {
    let mut s = Store::open_in_memory().unwrap();
    let (a, b) = (add(&mut s, "a", Some("x")), add(&mut s, "b", Some("x")));
    start(&mut s, &a, "/wt/1").unwrap();
    let rb = start(&mut s, &b, "/wt/2").unwrap();
    let moved = |wt| super::super::Here {
        branch: Some("feat/real"),
        ..here(wt)
    };
    s.done(&a, "h", Some(1), None, &moved("/wt/1")).unwrap();
    // `--run R` from another checkout names the run, not its branch
    let by_run = super::super::Here {
        run: Some(&rb),
        ..moved("/elsewhere")
    };
    s.done(&b, "h", Some(2), None, &by_run).unwrap();
    let bd = s.board("2026-10-11", NOW).unwrap();
    let branch = |u: &str| {
        let c = bd.chunks.iter().find(|c| c.uid == u).unwrap();
        c.run.as_ref().unwrap().branch.clone()
    };
    assert_eq!(branch(&a).as_deref(), Some("feat/real"));
    assert_eq!(branch(&b).as_deref(), Some("b"), "started on b");
}

/// A wave PR's diff sliced per chunk: board carries what `done` claimed, absent before.
#[test]
fn touched_is_the_claimed_diff_absent_until_claimed() {
    let mut s = Store::open_in_memory().unwrap();
    let a = scoped(&mut s, "a", &["src/"], &[]);
    let touched = |s: &Store| {
        let bd = s.board("2026-10-11", "2026-10-11T09:00:00Z").unwrap();
        bd.chunks.into_iter().find(|c| c.uid == a).unwrap().touched
    };
    assert_eq!(touched(&s), None, "a declared scope is no file touched");
    s.claim(&a, &["src/a.rs".into(), "docs/b.md".into()])
        .unwrap();
    assert_eq!(
        touched(&s),
        Some(vec!["src/a.rs".to_string(), "docs/b.md".to_string()])
    );
}

/// `plan done|park inbox` would hide every inbox chunk from the board: refused.
#[test]
fn inbox_plan_cannot_leave_active() {
    let mut s = Store::open_in_memory().unwrap();
    scoped(&mut s, "a", &[], &[]);
    let id: i64 = s
        .conn
        .query_row("SELECT id FROM plan WHERE name = 'inbox'", [], |r| r.get(0))
        .unwrap();
    for to in ["done", "parked"] {
        let e = s.plan_state(id, to, NOW).unwrap_err();
        assert!(e.starts_with("rejected: plan inbox"), "{e}");
    }
}

/// A done inbox (data from before the refusal) heals on the next add.
#[test]
fn add_heals_a_done_inbox() {
    let mut s = Store::open_in_memory().unwrap();
    scoped(&mut s, "a", &[], &[]);
    s.conn
        .execute("UPDATE plan SET state = 'done' WHERE name = 'inbox'", [])
        .unwrap();
    let b = scoped(&mut s, "b", &[], &[]);
    let bd = s.board("2026-10-11", "2026-10-11T09:00:00Z").unwrap();
    assert!(
        bd.chunks.iter().any(|c| c.uid == b),
        "inbox back on the board"
    );
}

/// Plan `owner_actions` the board serves: state moves plus `drop` only when
/// the plan holds no chunk and no child (the app's undone/drop buttons).
#[test]
fn plan_actions_name_state_moves_and_drop_only_when_empty() {
    use super::super::board::plan_actions;
    assert_eq!(
        plan_actions("active", false, false, false),
        vec!["park", "done", "drop"]
    );
    assert_eq!(
        plan_actions("active", false, true, false),
        vec!["park", "done"]
    );
    assert_eq!(
        plan_actions("active", false, false, true),
        vec!["park", "done"]
    );
    assert_eq!(
        plan_actions("parked", false, false, false),
        vec!["unpark", "drop"]
    );
    assert_eq!(
        plan_actions("done", false, false, false),
        vec!["undone", "drop"]
    );
    assert_eq!(plan_actions("done", false, true, false), vec!["undone"]);
    assert!(
        plan_actions("active", true, false, false).is_empty(),
        "inbox never moves"
    );
}
