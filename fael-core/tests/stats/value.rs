//! `value` shape: retired + value + verdict keys. Split from `main.rs`
//! (file-size ratchet).

use super::{BASE_ROWS, keys, value_of};

#[test]
fn json_shape_retired_value_and_verdict_are_frozen() {
    let v = value_of(BASE_ROWS, false);
    assert_eq!(keys(&v["retired"]), ["at_touch", "bumped", "pushed"], "{v}");
    assert!(
        v["retired"]["pushed"].is_u64()
            && v["retired"]["at_touch"].is_u64()
            && v["retired"]["bumped"].is_u64(),
        "{v}"
    );
    assert_eq!(
        keys(&v["value"]),
        [
            "by_event",
            "cross_agent",
            "decision_by_state",
            "handoffs_by_client",
            "handoffs_picked_up",
            "handoffs_same_client",
            "handoffs_writer_unknown",
            "in_context_at_edit",
            "issues_closed"
        ],
        "{v}"
    );
    assert!(
        [
            "handoffs_picked_up",
            "handoffs_same_client",
            "handoffs_writer_unknown",
            "in_context_at_edit",
            "issues_closed"
        ]
        .iter()
        .all(|k| v["value"][k].is_u64()),
        "{v}"
    );
    assert!(
        v["value"]["by_event"].is_object() && v["value"]["handoffs_by_client"].is_object(),
        "{v}"
    );
}
