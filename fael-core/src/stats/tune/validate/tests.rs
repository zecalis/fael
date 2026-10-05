use super::*;
use crate::stats::parse::parse;
use crate::stats::tune::{Tune, tune};
use crate::{Log, Row};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

fn size(sessions: usize, pushes: usize, cuts: usize) -> ArmSize {
    let days = |passes| Coverage {
        sessions,
        pushes,
        distinct_days: 4,
        max_day_share_pct: 30.0,
        max_session_share_pct: 5.0,
        passes,
    };
    ArmSize {
        sessions,
        search_pushes: pushes,
        gate_cuts: cuts,
        missed_push: Rate::new(0, cuts),
        retrieved_sessions: Rate::new(2, sessions),
        coverage: days(true),
        ..ArmSize::default()
    }
}

/// A validation that clears every bar; a test breaks one.
fn good() -> Validation {
    let kept = Rate::new(90, 100);
    Validation {
        candidate: "touch@1".into(),
        strata: vec![],
        candidate_all: size(40, 400, 250),
        holdout_all: size(40, 400, 0),
        exposure_cut_pct: Some(50.0),
        retained: Retained {
            cited: kept,
            pulled: kept,
            acted: kept,
        },
        retrieved_pct: (Some(5.0), Some(5.0)),
        warning: None,
        notes: vec![],
        verdict: Verdict {
            result: "",
            why: vec![],
        },
    }
}

fn is(v: &Validation, used: usize, want: &str) -> Vec<String> {
    let d = decide(v, used);
    assert_eq!(d.result, want, "{:?}", d.why);
    d.why
}

#[test]
fn every_frozen_bar_decides_the_verdict() {
    is(&good(), 2, "validated");
    // too little data is never a fail
    is(&good(), 1, "insufficient_data");
    let mut v = good();
    v.candidate_all.gate_cuts = 199;
    is(&v, 2, "insufficient_data");
    let mut v = good();
    v.holdout_all.coverage.passes = false;
    is(&v, 2, "insufficient_data");
    let mut v = good();
    v.candidate = "touch@1,touch@2".into();
    is(&v, 2, "insufficient_data");
    // enough data, one bar missed: not_validated, and it names the bar
    let mut v = good();
    v.exposure_cut_pct = Some(39.9);
    assert!(is(&v, 2, "not_validated")[0].contains("exposure"));
    let mut v = good();
    v.retained.acted = Rate::new(84, 100);
    assert!(is(&v, 2, "not_validated")[0].contains("acted"));
    let mut v = good();
    v.candidate_all.missed_push = Rate::new(2, 250); // upper bound well over 2%
    assert!(is(&v, 2, "not_validated")[0].contains("missed_push"));
    let mut v = good();
    v.retrieved_pct = (Some(6.1), Some(5.0)); // +22%
    assert!(is(&v, 2, "not_validated")[0].contains("going back"));
    v.retrieved_pct = (Some(5.9), Some(5.0)); // +18%
    is(&v, 2, "validated");
    // an outcome with no event cannot fail (cited is rare, SPEC §5)
    let mut v = good();
    v.retained.cited = Rate::new(0, 0);
    is(&v, 2, "validated");
}

fn usage(repo: &str, session: &str, arm: &str, trigger: &str, min: u32, n: usize) -> String {
    let ids: Vec<String> = (0..n).map(|i| format!("\"R{i}\"")).collect();
    format!(
        "{{\"ts\":\"2026-10-05T{:02}:{:02}:00.000Z\",\"repo\":\"{repo}\",\"client\":\"claude\",\"session\":\"{session}\",\"event\":\"search\",\"trigger\":\"{trigger}\",\"arm\":\"{arm}\",\"policy\":\"touch@1\",\"files\":[\"a.rs\"],\"ids\":[{}]}}\n",
        min / 60,
        min % 60,
        ids.join(",")
    )
}

fn run(text: &str) -> Tune {
    let p = parse(
        text,
        Path::new("/w/state/usage.jsonl"),
        &[PathBuf::from("/tmp")],
    );
    let log = Log {
        rows: Vec::<Row>::new(),
        ..Log::default()
    };
    tune(&p, &HashMap::from([("/w/r".to_string(), log)]), 0)
}

#[test]
fn no_arm_means_no_validation_and_candidate_lines_stay_out_of_the_replay() {
    let t = run(&usage("/w/r", "s1", "all", "hitlist", 0, 1));
    assert!(t.validation.is_none());
    let t = run(&usage("/w/r", "s1", "candidate", "hitlist", 0, 1));
    assert_eq!(t.all.sizes.search_pushes, 0, "a gated arm is not replayed");
    assert!(t.validation.is_some());
}

#[test]
fn a_stratum_with_unlike_trigger_mixes_is_unbalanced_and_a_small_one_insufficient() {
    let mut text = String::new();
    // 12 sessions per arm, 10 pushes each (≥ 10 sessions, ≥ 100 pushes):
    // the candidate arm is all hitlists, the holdout all reader-args
    for i in 0..12 {
        for k in 0..10 {
            text += &usage(
                "/w/r",
                &format!("c{i}"),
                "candidate",
                "hitlist",
                i * 10 + k,
                2,
            );
            text += &usage(
                "/w/r",
                &format!("h{i}"),
                "holdout",
                "reader-arg",
                i * 10 + k,
                3,
            );
        }
    }
    text += &usage("/w/r2", "tiny", "holdout", "hitlist", 0, 1);
    let v = run(&text).validation.unwrap();
    let s = |repo: &str| v.strata.iter().find(|s| s.repo == repo).unwrap();
    assert_eq!(s("/w/r").status, Status::Unbalanced);
    assert!(s("/w/r").max_trigger_gap_pp > 90.0);
    assert_eq!(s("/w/r2").status, Status::Insufficient);
    assert_eq!(v.verdict.result, "insufficient_data", "{v:?}");
    assert_eq!(v.candidate, "touch@1");
    assert_eq!(v.candidate_all.sessions, 0, "only usable strata are pooled");
}
