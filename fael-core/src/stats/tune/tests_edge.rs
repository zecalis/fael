//! Edges of the replay: which history level speaks, what is not replayed,
//! where a day or a stratum ends.

use super::tests::{line, pol, push, push_at, run, run_tz};
use super::*;

const A: [(&str, &str); 1] = [("A", "decision")];

/// Five quiet sessions that said A with `trigger` and `files`, then one that
/// says it with `now_trigger` and `now_files`: which level made the call?
fn levels(trigger: &str, files: &str, now_trigger: &str, now_files: &str) -> Tune {
    let hist: String = (0..5)
        .map(|i| push_at(&format!("h{i}"), i, "A", 0, trigger, files, ""))
        .collect();
    run(
        &(hist + &push_at("now", 100, "A", 0, now_trigger, now_files, "")),
        &A,
    )
}

#[test]
fn each_history_level_speaks_when_the_narrower_ones_have_nothing() {
    let a = r#"["a.rs"]"#;
    // same row, file and trigger
    let t = levels("hitlist", a, "hitlist", a);
    assert_eq!(t.all.fallback_used, [1, 0, 0, 0, 5], "{t:?}");
    // another trigger: (row,file) speaks
    let t = levels("glob", a, "hitlist", a);
    assert_eq!(t.all.fallback_used, [0, 1, 0, 0, 5], "{t:?}");
    // history was on a file the row does not name: (row) speaks
    let t = levels("hitlist", r#"["b.rs"]"#, "hitlist", a);
    assert_eq!(t.all.fallback_used, [0, 0, 1, 0, 5], "{t:?}");
    for t in [
        levels("hitlist", a, "hitlist", a),
        levels("glob", a, "hitlist", a),
        levels("hitlist", r#"["b.rs"]"#, "hitlist", a),
    ] {
        assert_eq!(pol(&t, "touch-yield@1").dropped, 1, "{t:?}");
    }
}

#[test]
fn the_row_file_is_the_hit_list_file_the_row_names() {
    // the hit list leads with z.rs, which the row does not name; a.rs is the key
    let hist: String = (0..5)
        .map(|i| push_at(&format!("h{i}"), i, "A", 0, "x", r#"["z.rs","a.rs"]"#, ""))
        .collect();
    let now = push_at("now", 100, "A", 0, "x", r#"["a.rs"]"#, "");
    let t = run(&(hist + &now), &A);
    assert_eq!(t.all.fallback_used[0], 1, "{:?}", t.all.fallback_used);
}

#[test]
fn a_session_that_ends_at_the_instant_a_row_is_said_is_no_history() {
    let hist = |last: u32| -> String {
        (0..4)
            .map(|i| push(&format!("h{i}"), i, "A", 0, ""))
            .collect::<String>()
            + &push("h4", last, "A", 0, "")
    };
    // h4 ends at minute 5; the row is said at minute 5: 4 earlier sessions only
    let t = run(&(hist(5) + &push("now", 5, "A", 0, "")), &A);
    assert_eq!(pol(&t, "touch-yield@1").dropped, 0);
    // one minute later all five have ended
    let t = run(&(hist(5) + &push("now", 6, "A", 0, "")), &A);
    assert_eq!(pol(&t, "touch-yield@1").dropped, 1);
}

#[test]
fn a_row_gone_from_the_log_is_said_but_not_replayed() {
    let t = run(&push("s1", 0, "GONE", 0, ""), &A);
    assert_eq!((t.all.sizes.rows_said, t.all.sizes.evaluable_rows), (1, 0));
    assert_eq!(pol(&t, "touch@1").exposure, Rate::new(0, 0));
    assert_eq!(pol(&t, "touch@1").dropped, 0);
}

#[test]
fn only_search_pushes_are_replayed() {
    for event in ["read", "edit", "shell-edit", "brief"] {
        let u = push("s1", 0, "A", 0, "")
            .replace("\"event\":\"search\"", &format!("\"event\":\"{event}\""));
        let t = run(&u, &A);
        assert_eq!(t.all.sizes.rows_said, 0, "{event}");
        assert_eq!(t.all.sizes.search_pushes, 0, "{event}");
    }
}

#[test]
fn a_stratum_learns_from_its_own_sessions_only() {
    // five quiet sessions in the codex stratum, then claude says the same row
    let hist: String = (0..5)
        .map(|i| push(&format!("h{i}"), i, "A", 0, "").replace("\"claude\"", "\"codex\""))
        .collect();
    let t = run(&(hist + &push("now", 100, "A", 0, "")), &A);
    assert_eq!(
        pol(&t, "touch-yield@1").dropped,
        1,
        "pooled, the history speaks"
    );
    for s in &t.strata {
        let y = s
            .section
            .policies
            .iter()
            .find(|p| p.policy == "touch-yield@1")
            .unwrap();
        assert_eq!(y.dropped, 0, "{} saw no history of its own", s.client);
    }
}

#[test]
fn coverage_counts_the_local_day_not_the_utc_day() {
    let at = |s: &str, ts: &str| {
        line(s, 0, "\"event\":\"search\",\"ids\":[]").replace("2026-10-05T00:00:00", ts)
    };
    let u = at("a", "2026-10-05T23:30:00") + &at("b", "2026-10-06T00:30:00");
    let utc = run_tz(&u, &A, 0);
    assert_eq!(utc.all.coverage.distinct_days, 2);
    assert_eq!(utc.days, Some(("2026-10-05".into(), "2026-10-06".into())));
    // +07:00: 06:30 and 07:30 on the 6th — one day
    let bkk = run_tz(&u, &A, 7 * 60);
    assert_eq!(bkk.all.coverage.distinct_days, 1);
    assert_eq!(bkk.days, Some(("2026-10-06".into(), "2026-10-06".into())));
}
