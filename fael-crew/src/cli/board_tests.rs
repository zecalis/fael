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
        overlaps_how: Vec::new(),
        overlap_classes: Vec::new(),
        pair: Vec::new(),
        stalled: false,
        ended: false,
        wait: None,
        approval: None,
        run: None,
        handoff: None,
        issues: vec![],
        left: vec![],
        left_open: vec![],
        stale: vec![],
        owner_actions: Vec::new(),
        conflicts: None,
        touched: None,
        merged: false,
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
fn stale_chunks_get_their_own_section() {
    let mut s = ready("fresh", 0);
    s.title = "live work".into();
    let mut old = ready("ghost", 0);
    old.title = "landed elsewhere".into();
    old.stale = vec!["warning: linked issue 01M1 — closed: shipped in #902 — done elsewhere? fael chunk drop ghost …".into()];
    let p = Project {
        chunks: vec![s, old],
        ..Project::named(Path::new(""))
    };
    for cut in [false, true] {
        let t = text::text(&lists(String::new(), vec![&p]), cut);
        assert!(t.contains("stale\n"), "{t}");
        assert!(
            t.contains("ghost") && t.contains("linked issue 01M1"),
            "{t}"
        );
    }
}

#[test]
fn blocked_folds_to_plan_counts_unless_plan_cut() {
    let blocked = |uid: &str, plan: &str| {
        let mut c = ready(uid, 0);
        c.ready = false;
        c.plan = plan.into();
        c.blocked_by = vec![fael_crew::Blocker {
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

fn archived(plan: &str, state: &str, counts: &[(&str, usize)]) -> BoardPlan {
    BoardPlan {
        app: String::new(),
        name: plan.into(),
        title: plan.into(),
        area: "dev".into(),
        state: state.into(),
        truth: "db",
        parent: None,
        counts: counts.iter().map(|(s, n)| (s.to_string(), *n)).collect(),
        owner_actions: Vec::new(),
    }
}

#[test]
fn plan_cut_on_parked_plan_names_hidden_chunks() {
    // the board lists no chunks of a parked plan: the cut would read empty
    let p = Project {
        plans: vec![archived("house-books", "parked", &[("open", 2)])],
        chunks: vec![],
        ..Project::named(Path::new(""))
    };
    let t = text::text(&lists(String::new(), vec![&p]), true);
    assert!(
        t.contains("plan house-books is parked — 2 open chunks hidden"),
        "{t}"
    );
    // same retype `chunk show` on a hidden uid prints (issue 01M5KVYE)
    assert!(
        t.contains("`fael plan export house-books` lists its uids"),
        "{t}"
    );
    assert!(!t.contains("nothing on the board"), "{t}");
}

#[test]
fn plan_cut_on_done_plan_names_hidden_chunks_with_undone() {
    let p = Project {
        plans: vec![archived("old", "done", &[("open", 1), ("done", 4)])],
        chunks: vec![],
        ..Project::named(Path::new(""))
    };
    let t = text::text(&lists(String::new(), vec![&p]), true);
    assert!(t.contains("plan old is done — 1 open chunk hidden"), "{t}");
    assert!(t.contains("`fael plan undone old` first"), "{t}");
    assert!(!t.contains("unpark"), "{t}");
}

#[test]
fn plan_cut_on_active_empty_plan_stays_empty() {
    let p = Project {
        plans: vec![archived("fresh", "active", &[])],
        chunks: vec![],
        ..Project::named(Path::new(""))
    };
    let t = text::text(&lists(String::new(), vec![&p]), true);
    assert!(t.contains("nothing on the board"), "{t}");
}

#[test]
fn empty_repo_names_its_next_command_before_other_repos_rows() {
    // repos registered, no chunks anywhere: a fresh repo's own empty state
    // names the next command, then the repos line lists where it looked
    let p = Project {
        chunks: vec![],
        ..Project::named(Path::new("here"))
    };
    let t = text::text(&lists(String::new(), vec![&p]), false);
    assert!(
        t.contains("this repo has no chunks — start one: `fael plan new <name> \"<title>\"`"),
        "{t}"
    );
    assert!(t.contains("repos:"), "{t}");
}

#[test]
fn merged_branch_puts_running_chunk_in_needs_you() {
    use fael_crew::BoardRun;
    let mut c = ready("r1", 0);
    c.ready = false;
    c.state = "running".into();
    c.run = Some(BoardRun {
        id: "R1".into(),
        client: None,
        session: None,
        model: None,
        worktree: None,
        branch: Some("feat/x".into()),
        pr: None,
        out: None,
        started: "2026-10-10T08:00:00Z".into(),
        last_seen: None,
        ended: None,
        cost: None,
        review: None,
        joint: None,
        partner: None,
    });
    let p = Project {
        branches: vec![super::branches::Branch {
            name: "feat/x".into(),
            state: "merged",
            chunks: Vec::new(),
        }],
        chunks: vec![c],
        ..Project::named(Path::new(""))
    };
    let b = lists(String::new(), vec![&p]);
    assert!(
        b.needs_you
            .iter()
            .any(|n| n.kind == "merged" && n.uid == "r1"),
        "{:?}",
        b.needs_you
            .iter()
            .map(|n| (n.kind, &n.uid))
            .collect::<Vec<_>>()
    );
    // a running chunk on a live branch is not merged
    let mut c = ready("r2", 0);
    c.ready = false;
    c.state = "running".into();
    c.run = Some(BoardRun {
        id: "R2".into(),
        client: None,
        session: None,
        model: None,
        worktree: None,
        branch: Some("feat/live".into()),
        pr: None,
        out: None,
        started: "2026-10-10T08:00:00Z".into(),
        last_seen: None,
        ended: None,
        cost: None,
        review: None,
        joint: None,
        partner: None,
    });
    let p = Project {
        branches: vec![super::branches::Branch {
            name: "feat/live".into(),
            state: "local",
            chunks: Vec::new(),
        }],
        chunks: vec![c],
        ..Project::named(Path::new(""))
    };
    let b = lists(String::new(), vec![&p]);
    assert!(
        b.needs_you.iter().all(|n| n.kind != "merged"),
        "{:?}",
        b.needs_you
            .iter()
            .map(|n| (n.kind, &n.uid))
            .collect::<Vec<_>>()
    );
}

#[test]
fn lean_cuts_only_finished_chunks() {
    let mut done = ready("d", 0);
    done.state = "done".into();
    done.touched = Some(vec!["a".into()]);
    let mut live = ready("l", 0);
    live.touched = Some(vec!["a".into()]);
    let mut cs = vec![done, live];
    lean(&mut cs);
    assert!(cs[0].touched.is_none());
    assert!(cs[1].touched.is_some());
}

#[test]
fn lean_keeps_the_summary_of_a_finished_chunk() {
    let mut done = ready("d", 0);
    done.state = "done".into();
    done.handoff = Some(fael_crew::BoardHandoff {
        at: "t".into(),
        text: "long".into(),
    });
    done.run = Some(fael_crew::BoardRun {
        id: "R".into(),
        client: None,
        session: Some("s".into()),
        model: Some("m".into()),
        worktree: None,
        branch: None,
        pr: Some(7),
        out: None,
        started: "t".into(),
        last_seen: None,
        ended: None,
        cost: Some(1.5),
        review: None,
        joint: None,
        partner: None,
    });
    let mut cs = vec![done];
    lean(&mut cs);
    let h = cs[0].handoff.as_ref().unwrap();
    assert_eq!((h.at.as_str(), h.text.as_str()), ("t", ""));
    let r = cs[0].run.as_ref().unwrap();
    assert_eq!(
        (r.pr, r.cost, r.model.as_deref()),
        (Some(7), Some(1.5), Some("m"))
    );
    assert!(r.session.is_none());
}
