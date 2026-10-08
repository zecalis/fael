//! plan kickoff: a ticked chunk's rows leave, the handoff and unkeyed rows stay.

use super::{ids, row};
use fael_core::*;

#[test]
fn closed_chunks_reads_ticks_not_guesses() {
    let doc = "- **Progress:**\n  - [x] chunk 1 — a\n  - [~] chunk 3 — dropped\n  - [ ] chunk 2 — open\n  - [x] fix — no number\n1. **chunk 4 — step**\n";
    assert_eq!(closed_chunks(doc), [1, 3]);
}

#[test]
fn drop_closed_keeps_handoff_other_keys_and_open_chunks() {
    let key = |id: &str, k: &str| row(id, "note", &["plan:foo"], Some(k));
    let l = Log {
        rows: vec![
            key("A0000000000000000000000001", "plan:foo:chunk-1"),
            key("A0000000000000000000000002", "plan:foo:chunk-2"),
            key("A0000000000000000000000003", "plan:foo:handoff"),
            key("A0000000000000000000000004", "plan:foobar:chunk-1"),
            row("A0000000000000000000000005", "note", &["plan:foo"], None),
        ],
        closes: vec![],
        warnings: vec![],
    };
    let rows: Vec<&Row> = l.rows.iter().collect();
    assert_eq!(
        ids(&drop_closed(rows, "plan:foo", &[1])),
        ["02", "03", "04", "05"]
    );
}

#[test]
fn closed_chunks_edges() {
    // multi-digit, upper-case X, a tick with nothing after the number
    assert_eq!(
        closed_chunks("  - [x] chunk 10 — a\n- [X] chunk 11\n- [x] chunk 12"),
        [10, 11, 12]
    );
    // not a number, not a chunk line: nothing closes
    assert!(closed_chunks("- [x] chunk\n- [x] chunk two\n- [x] fix 3\n- [ ] chunk 4").is_empty());
    // a number glued to letters is no chunk number — never guess
    assert!(closed_chunks("- [x] chunk 2a — x").is_empty());
}
