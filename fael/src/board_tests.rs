//! The queue order (SPEC §3).

use super::*;

fn ready(
    uid: &str,
    pin: Option<i64>,
    due: Option<&str>,
    unblocks: usize,
    rank: Option<i64>,
) -> BoardChunk {
    BoardChunk {
        uid: uid.into(),
        app: String::new(),
        plan: "p".into(),
        label: None,
        title: String::new(),
        state: "open".into(),
        size: None,
        model_hint: None,
        client_hint: None,
        scope: Vec::new(),
        due: due.map(String::from),
        pin,
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
        owner_actions: Vec::new(),
        rank,
        seq: 0,
    }
}

#[test]
fn queue_order_is_pin_due_unblocks_rank() {
    let mut held = ready("held", Some(0), None, 9, Some(0));
    held.ready = false;
    let chunks = vec![
        ready("unranked", None, None, 1, None),
        ready("rank2", None, None, 1, Some(2)),
        held,
        ready("due-late", None, Some("2026-10-20"), 0, None),
        ready("rank1", None, None, 1, Some(1)),
        ready("tie", None, None, 1, Some(1)),
        ready("pin2", Some(2), None, 0, None),
        ready("frees3", None, None, 3, Some(9)),
        ready("due-soon", None, Some("2026-10-12"), 0, None),
        ready("pin1", Some(1), Some("2026-12-01"), 0, None),
    ];
    let p = Project {
        name: String::new(),
        root: String::new(),
        base: None,
        worktrees: Vec::new(),
        plans: Vec::new(),
        chunks,
        error: None,
    };
    let b = lists(String::new(), vec![p]);
    // stable: `tie` keeps its place after `rank1`
    assert_eq!(
        b.queue,
        [
            "pin1", "pin2", "due-soon", "due-late", "frees3", "rank1", "tie", "rank2", "unranked"
        ]
    );
}
