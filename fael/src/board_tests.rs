//! The queue order: unblocks most first, plan order breaking ties.

use super::*;

fn ready(uid: &str, unblocks: usize) -> BoardChunk {
    BoardChunk {
        uid: uid.into(),
        app: String::new(),
        plan: "p".into(),
        label: None,
        title: String::new(),
        state: "open".into(),
        size: None,
        model_hint: None,
        model: None,
        client_hint: None,
        scope: Vec::new(),
        ready: true,
        blocked_by: Vec::new(),
        unblocks,
        overlaps: Vec::new(),
        pair: Vec::new(),
        stalled: false,
        ended: false,
        wait: None,
        approval: None,
        run: None,
        handoff: None,
        issues: vec![],
        owner_actions: Vec::new(),
        ticket: None,
        conflicts: None,
        touched: None,
        seq: 0,
    }
}

#[test]
fn queue_order_is_unblocks_then_plan_order() {
    let mut held = ready("held", 9);
    held.ready = false;
    let chunks = vec![
        ready("first", 1),
        held,
        ready("tie", 1),
        ready("frees3", 3),
        ready("frees0", 0),
    ];
    let p = Project {
        chunks,
        ..Project::named(Path::new(""))
    };
    let b = lists(String::new(), vec![&p]);
    // stable: `tie` keeps its place after `first`
    assert_eq!(b.queue, ["frees3", "first", "tie", "frees0"]);
}

#[test]
fn ready_line_says_no_scope() {
    let mut scoped = ready("scoped", 0);
    scoped.scope = vec!["src/a.rs".into()];
    let p = Project {
        chunks: vec![scoped, ready("bare", 0)],
        ..Project::named(Path::new(""))
    };
    let t = text::text(&lists(String::new(), vec![&p]), false);
    assert!(t.contains("bare  p   — no scope\n"), "{t}");
    assert!(t.contains("scoped  p  \n"), "{t}");
}

#[test]
fn blocked_folds_to_plan_counts_unless_plan_cut() {
    let blocked = |uid: &str, plan: &str| {
        let mut c = ready(uid, 0);
        c.ready = false;
        c.plan = plan.into();
        c.blocked_by = vec![fael_core::plan::Blocker {
            uid: None,
            what: "x".into(),
        }];
        c
    };
    let p = Project {
        chunks: vec![blocked("u1", "a"), blocked("u2", "b"), blocked("u3", "b")],
        ..Project::named(Path::new(""))
    };
    let b = lists(String::new(), vec![&p]);
    let t = text::text(&b, false);
    assert!(
        t.contains("blocked  3 — b 2 · a 1 — `fael board --plan <p>`"),
        "{t}"
    );
    assert!(!t.contains("u1"), "{t}");
    let t = text::text(&b, true);
    assert!(
        t.contains("u1") && t.contains("u2") && t.contains("u3"),
        "{t}"
    );
}

fn draft(uid: &str, plan: &str) -> BoardChunk {
    let mut c = ready(uid, 0);
    c.ready = false;
    c.state = "draft".into();
    c.plan = plan.into();
    c.title = "wait run.cost coverage — cost per fixed issue".into();
    c
}

#[test]
fn gated_drafts_listed_only_under_plan_cut_never_queued() {
    let p = Project {
        chunks: vec![ready("open1", 0), draft("g1", "fael-flow")],
        ..Project::named(Path::new(""))
    };
    let b = lists(String::new(), vec![&p]);
    assert_eq!(b.queue, ["open1"]);
    let t = text::text(&b, false);
    assert!(!t.contains("g1"), "{t}");
    let t = text::text(&b, true);
    assert!(t.contains("gated") && t.contains("g1"), "{t}");
}
