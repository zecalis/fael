//! PLAN-fael-learn-loop chunk 7: a canary or ramp look reads usage from where
//! the stage began, archives included, instead of this and last month; a
//! state with no offset reads as before; a shadow row names itself screening.

use super::stage::{
    append, gate_file, gate_rows, put_stage, shadow_usage, stage_now, stop_look, with_rows,
};
use super::stage_arms::arm_usage_into;
use super::state;
use serde_json::{Value, json};
use std::path::Path;

fn put_canary(d: &Path, offset: Option<(&str, u64)>) {
    put_stage(d, "touch@1", "canary");
    let mut s: Value =
        serde_json::from_str(&std::fs::read_to_string(gate_file(d)).unwrap()).unwrap();
    if let Some((month, at)) = offset {
        s["stage_month"] = month.into();
        s["stage_at"] = at.into();
    }
    std::fs::write(gate_file(d), s.to_string()).unwrap();
}

/// An archive of Aug 2026 with arm data a canary could not have produced
/// (every candidate session dups) followed by data that clears the bars, and a
/// newer, empty archive — so the arm data is older than "last month". Returns
/// where the bad data ends.
fn old_archive(d: &Path) -> u64 {
    let dir = state(d).join("usage");
    std::fs::create_dir_all(&dir).unwrap();
    append(&dir.join("2026-09.jsonl"), "\n");
    let a = dir.join("2026-08.jsonl");
    arm_usage_into(d, &a, (40, 40), (40, 0));
    let at = std::fs::metadata(&a).unwrap().len();
    arm_usage_into(d, &a, (40, 40), (1, 1));
    at
}

#[test]
fn a_canary_look_reads_from_where_the_stage_began_however_many_months_back() {
    let d = with_rows();
    let at = old_archive(&d);
    put_canary(&d, Some(("2026-08", at)));
    stop_look(&d);
    assert_eq!(
        stage_now(&d).as_deref(),
        Some("ramp"),
        "the bad data is before the stage"
    );
    let body: Value = serde_json::from_str(gate_rows(&d)[0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(body["basis"], "validation");
}

#[test]
fn a_state_with_no_offset_is_read_as_before_this_and_last_month() {
    let d = with_rows();
    old_archive(&d);
    put_canary(&d, None);
    stop_look(&d);
    // the Aug archive is out of reach: no arm has a search push, so it holds
    assert_eq!(stage_now(&d).as_deref(), Some("canary"));
    assert!(gate_rows(&d).is_empty());
}

#[test]
fn a_shadow_row_names_itself_screening_and_the_stage_it_opens_records_where_it_began() {
    let d = with_rows();
    shadow_usage(&d, 40, false);
    stop_look(&d);
    assert_eq!(stage_now(&d).as_deref(), Some("canary"));
    let row = &gate_rows(&d)[0];
    let body: Value = serde_json::from_str(row["text"].as_str().unwrap()).unwrap();
    assert_eq!(body["basis"], "screening");
    assert!(
        row["title"].as_str().unwrap().contains("screening"),
        "{row}"
    );
    let s: Value = serde_json::from_str(&std::fs::read_to_string(gate_file(&d)).unwrap()).unwrap();
    assert_eq!(
        (&s["stage_at"], &s["stage_month"]),
        (&s["usage_at"], &s["usage_month"])
    );
    assert!(s["stage_at"].as_u64().unwrap() > 0, "{s}");
    assert_eq!(body["arm_split"], json!({"candidate": 50, "baseline": 50}));
}
