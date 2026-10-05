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
        repo: "/w/r".into(),
        candidate: "touch@1".into(),
        strata: vec![],
        candidate_all: size(40, 400, 250),
        holdout_all: size(40, 400, 0),
        bars: Bars {
            exposure_cut_pct: Some(50.0),
            retained: Retained {
                cited: kept,
                pulled: kept,
                acted: kept,
            },
            retrieved_pct: (Some(5.0), Some(5.0)),
            dup_pct: (Some(2.0), Some(2.0)),
        },
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
    // one eligible stratum is enough: a single-client repo can validate
    is(&good(), 1, "validated");
    // too little data is never a fail
    is(&good(), 0, "insufficient_data");
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
    v.bars.exposure_cut_pct = Some(39.9);
    assert!(is(&v, 2, "not_validated")[0].contains("exposure"));
    let mut v = good();
    v.bars.retained.acted = Rate::new(84, 100);
    assert!(is(&v, 2, "not_validated")[0].contains("acted"));
    let mut v = good();
    v.candidate_all.missed_push = Rate::new(2, 250); // upper bound well over 2%
    assert!(is(&v, 2, "not_validated")[0].contains("missed_push"));
    let mut v = good();
    v.bars.retrieved_pct = (Some(6.1), Some(5.0)); // +22%
    assert!(is(&v, 2, "not_validated")[0].contains("going back"));
    v.bars.retrieved_pct = (Some(5.9), Some(5.0)); // +18%
    is(&v, 2, "validated");
    // sessions that filed a duplicate of an unseen row: same bar, same +20%
    let mut v = good();
    v.bars.dup_pct = (Some(2.5), Some(2.0)); // +25%
    assert!(is(&v, 2, "not_validated")[0].contains("duplicates"));
    v.bars.dup_pct = (Some(2.3), Some(2.0)); // +15%
    is(&v, 2, "validated");
    // an outcome with no event cannot fail (cited is rare, SPEC §5)
    let mut v = good();
    v.bars.retained.cited = Rate::new(0, 0);
    is(&v, 2, "validated");
}

/// A used client stratum whose own bars are `b`.
fn stratum(client: &str, b: Bars) -> StratumArms {
    StratumArms {
        repo: "/w/r".into(),
        client: client.into(),
        status: Status::Used,
        max_trigger_gap_pp: 0.0,
        candidate: size(20, 200, 125),
        holdout: size(20, 200, 0),
        bars: Some(b),
    }
}

#[test]
fn a_client_that_fails_fails_the_repo_though_the_pool_passes() {
    let mut v = good();
    let ok = v.bars.clone();
    let bad = Bars {
        exposure_cut_pct: Some(10.0),
        ..ok.clone()
    };
    v.strata = vec![stratum("claude", ok), stratum("opencode", bad)];
    let why = is(&v, 2, "not_validated");
    assert!(why[0].starts_with("client opencode: exposure"), "{why:?}");
    // one stratum is the pool: a miss is said once, not twice
    v.strata.truncate(1);
    v.strata[0].bars = Some(Bars {
        exposure_cut_pct: Some(10.0),
        ..v.bars.clone()
    });
    is(&v, 1, "validated");
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
    run_in(text, &|r| r.to_string())
}

fn run_in(text: &str, scope: &dyn Fn(&str) -> String) -> Tune {
    let p = parse(
        text,
        Path::new("/w/state/usage.jsonl"),
        &[PathBuf::from("/tmp")],
    );
    let log = Log {
        rows: Vec::<Row>::new(),
        ..Log::default()
    };
    tune(&p, &HashMap::from([("/w/r".to_string(), log)]), 0, scope)
}

#[test]
fn no_arm_means_no_validation_and_candidate_lines_stay_out_of_the_replay() {
    let t = run(&usage("/w/r", "s1", "all", "hitlist", 0, 1));
    assert!(t.validation.is_empty());
    let t = run(&usage("/w/r", "s1", "candidate", "hitlist", 0, 1));
    assert_eq!(t.all.sizes.search_pushes, 0, "a gated arm is not replayed");
    assert_eq!(t.validation.len(), 1);
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
    let t = run(&text);
    // two repos, two verdicts: /w/r2 borrows nothing from /w/r
    assert_eq!(t.validation.len(), 2);
    let of = |repo: &str| t.validation.iter().find(|v| v.repo == repo).unwrap();
    let (v, w) = (of("/w/r"), of("/w/r2"));
    assert_eq!(v.strata[0].status, Status::Unbalanced);
    assert!(v.strata[0].max_trigger_gap_pp > 90.0);
    assert_eq!(w.strata[0].status, Status::Insufficient);
    assert_eq!(w.strata.len(), 1, "{w:?}");
    assert_eq!(v.verdict.result, "insufficient_data", "{v:?}");
    assert_eq!(v.candidate, "touch@1");
    assert_eq!(v.candidate_all.sessions, 0, "only usable strata are pooled");
}

#[test]
fn worktrees_of_one_repo_are_one_stratum_and_scope_decides_what_pools() {
    let mut text = String::new();
    for wt in ["/w/r-a", "/w/r-b", "/w/r-c"] {
        text += &usage(wt, &format!("c{wt}"), "candidate", "hitlist", 0, 2);
        text += &usage(wt, &format!("h{wt}"), "holdout", "hitlist", 1, 2);
    }
    let t = run_in(&text, &|r| {
        r.rsplit_once('-').map_or(r, |(a, _)| a).to_string()
    });
    assert_eq!(t.validation.len(), 1);
    let v = &t.validation[0];
    assert_eq!(v.repo, "/w/r");
    assert_eq!(v.strata.len(), 1, "one client in one repo = one stratum");
    assert_eq!(v.strata[0].candidate.sessions, 3);
    // without that scope the same lines are three repos
    assert_eq!(run(&text).validation.len(), 3);
}

#[test]
fn a_dup_line_counts_once_per_session_in_its_arm() {
    let dup = |session: &str| {
        format!(
            "{{\"ts\":\"2026-10-05T09:00:00.000Z\",\"repo\":\"/w/r\",\"session\":\"{session}\",\"event\":\"outcome\",\"ids\":[],\"dup\":[\"X\"]}}\n"
        )
    };
    let mut text = usage("/w/r", "c1", "candidate", "hitlist", 0, 2)
        + &usage("/w/r", "h1", "holdout", "hitlist", 1, 2)
        + &usage("/w/r", "h2", "holdout", "hitlist", 2, 2);
    // c1 twice, h1 once, a session no push carries an arm for: only the first two count
    text += &(dup("c1") + &dup("c1") + &dup("h1") + &dup("nobody"));
    let v = &run(&text).validation[0];
    assert_eq!(v.strata[0].candidate.dup_sessions, Rate::new(1, 1));
    assert_eq!(v.strata[0].holdout.dup_sessions, Rate::new(1, 2));
}
