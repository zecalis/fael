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
        conflicts: None,
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
    let b = lists(String::new(), vec![p]);
    // stable: `tie` keeps its place after `first`
    assert_eq!(b.queue, ["frees3", "first", "tie", "frees0"]);
}
