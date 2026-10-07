//! PLAN-fael-learn-loop chunk 7: a canary or ramp look reads usage from where
//! the stage began, archives included, instead of this and last month; a
//! state with no offset reads as before; a shadow row names itself screening.

use super::stage::{
    append, gate_file, gate_rows, put_stage, shadow_usage, stage_now, stop_as, stop_look, with_rows,
};
use super::stage_arms::arm_usage_into;
use super::working_set::{add, grep};
use super::{fael, repo, state};
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

fn gate_state(d: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(gate_file(d)).unwrap()).unwrap()
}

/// Arm data of 20 candidate and 20 baseline sessions — under the 30-session
/// minimum on its own — with session names made unique to `tag`, so two
/// halves add up to 40. Written to a scratch file, handed back as text.
fn half(d: &Path, tag: &str) -> String {
    let scratch = state(d).join("half.jsonl");
    let _ = std::fs::remove_file(&scratch);
    arm_usage_into(d, &scratch, (20, 20), (1, 1));
    std::fs::read_to_string(&scratch)
        .unwrap()
        .replace(
            "\"session\":\"candidate",
            &format!("\"session\":\"candidate{tag}"),
        )
        .replace(
            "\"session\":\"holdout",
            &format!("\"session\":\"holdout{tag}"),
        )
}

#[test]
fn each_stage_starts_its_own_offset_and_a_look_that_changes_nothing_keeps_it() {
    let d = with_rows();
    let envs = |s: &'static str| [("FAEL_SESSION", s)];
    shadow_usage(&d, 40, false);
    stop_as(&state(&d), &d, "e1", &envs("e1"));
    let canary = gate_state(&d);
    assert_eq!(canary["stage"], "canary");
    arm_usage_into(&d, &state(&d).join("usage.jsonl"), (40, 40), (1, 1));
    stop_as(&state(&d), &d, "e2", &envs("e2"));
    let ramp = gate_state(&d);
    assert_eq!(ramp["stage"], "ramp");
    assert!(
        ramp["stage_at"].as_u64() > canary["stage_at"].as_u64(),
        "ramp reads from its own start, not canary's: {canary} {ramp}"
    );
    // a look that holds the stage keeps where it began
    arm_usage_into(&d, &state(&d).join("usage.jsonl"), (40, 40), (1, 1));
    stop_look(&d);
    let held = gate_state(&d);
    assert_eq!(held["stage"], "ramp");
    assert!(
        held["usage_at"].as_u64() > ramp["usage_at"].as_u64(),
        "{held}"
    );
    assert_eq!(
        (&held["stage_at"], &held["stage_month"]),
        (&ramp["stage_at"], &ramp["stage_month"])
    );
}

#[test]
fn arm_data_on_both_sides_of_a_monthly_move_adds_up() {
    let d = with_rows();
    let (a, b) = (half(&d, "a"), half(&d, "b"));
    let live = state(&d).join("usage.jsonl");
    let _ = std::fs::remove_file(&live); // the probe's own lines
    // the stage began 60% of a half into the live file, so a read that took the
    // moved live file from that byte instead of its start would lose b's candidates
    let junk = format!("{}\n", " ".repeat(a.len() * 6 / 10));
    std::fs::write(&live, format!("{junk}{a}")).unwrap();
    let month = fael_core::rfc3339(fael_core::now_ms())[..7].to_string();
    put_canary(&d, Some((&month, junk.len() as u64)));
    stop_look(&d);
    assert_eq!(
        stage_now(&d).as_deref(),
        Some("canary"),
        "20 sessions are too few"
    );
    // the live file moves to its month's archive; b lands in the new one
    let archive = state(&d).join("usage").join(format!("{month}.jsonl"));
    std::fs::create_dir_all(archive.parent().unwrap()).unwrap();
    std::fs::rename(&live, &archive).unwrap();
    std::fs::write(&live, &b).unwrap();
    stop_look(&d);
    assert_eq!(stage_now(&d).as_deref(), Some("ramp"), "20 + 20 sessions");
}

#[test]
fn tune_names_a_shadow_replay_screening_and_the_arm_stages_validation() {
    let d = repo();
    add(&d, "decision", "keep the parser pure", "src/a.rs");
    grep(&d, "s1", &["src/a.rs"]);
    let stage_line = |d: &Path, stage: &str| {
        let (ok, out, err) = fael(d, &["tune"], "");
        assert!(ok, "{out}{err}");
        let at = format!(" — {stage}");
        out.lines()
            .find(|l| l.contains(&at))
            .expect(&out)
            .to_string()
    };
    let shadow = stage_line(&d, "shadow");
    assert!(
        shadow.contains("screening") && !shadow.contains("validation"),
        "{shadow}"
    );
    put_stage(&d, "touch@1", "canary");
    let canary = stage_line(&d, "canary");
    assert!(
        canary.contains("validation") && !canary.contains("screening"),
        "{canary}"
    );
}
