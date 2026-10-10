//! PLAN-fael-learn-loop chunk 4: `fael tune` replays the candidates over the
//! search pushes in the usage log. Read-only, deterministic, local-zone days.
//! No hook writes a search push any more (rows come at the edit): what is
//! left to replay is usage written before that.

use super::{fael, fael_env, json, repo, state};
use serde_json::{Value, json as jv};
use std::path::Path;

fn tune(d: &Path, args: &[&str]) -> String {
    let mut a = vec!["tune"];
    a.extend(args);
    let (ok, out, err) = fael(d, &a, "");
    assert!(ok, "{out}{err}");
    out
}

#[test]
fn tune_writes_nothing() {
    let d = repo();
    usage_at(
        &d,
        &["2026-10-05T09:00:00.000Z", "2026-10-06T09:00:00.000Z"],
    );
    let snap = |d: &Path| {
        let mut files: Vec<(String, Vec<u8>)> = vec![];
        for root in [state(d), d.join(".fael")] {
            let mut stack = vec![root];
            while let Some(p) = stack.pop() {
                if p.is_dir() {
                    stack.extend(std::fs::read_dir(&p).unwrap().map(|e| e.unwrap().path()));
                } else {
                    files.push((p.display().to_string(), std::fs::read(&p).unwrap()));
                }
            }
        }
        files.sort();
        files
    };
    let before = snap(&d);
    tune(&d, &[]);
    tune(&d, &["--json"]);
    assert!(before == snap(&d), "tune changed a file");
}

#[test]
fn tune_says_so_when_there_is_nothing_to_replay() {
    let d = repo();
    let out = tune(&d, &[]);
    assert!(out.contains("nothing to replay"), "{out}");
    let (ok, _, err) = fael(&d, &["tune", "--since", "yesterday"], "");
    assert!(!ok && err.contains("--since"), "{err}");
    let (ok, _, err) = fael(&d, &["tune", "--rows"], "");
    assert!(!ok && err.contains("tune takes no --rows"), "{err}");
}

/// Search pushes stamped `ts`, written straight into the usage log: the clock
/// is the one thing the writers cannot be told.
fn usage_at(d: &Path, stamps: &[&str]) {
    std::fs::create_dir_all(state(d)).unwrap();
    let lines: String = stamps
        .iter()
        .enumerate()
        .map(|(i, ts)| {
            format!(
                "{{\"ts\":\"{ts}\",\"repo\":{},\"client\":\"claude\",\"session\":\"u{i}\",\"event\":\"search\",\"bytes\":0,\"est_tokens\":0,\"ids\":[]}}\n",
                json(d)
            )
        })
        .collect();
    std::fs::write(state(d).join("usage.jsonl"), lines).unwrap();
}

fn tune_env(d: &Path, args: &[&str], env: &[(&str, &str)]) -> Value {
    let mut a = vec!["tune", "--json"];
    a.extend(args);
    let (ok, out, err) = fael_env(d, &a, "", env);
    assert!(ok, "{out}{err}");
    serde_json::from_str(&out).unwrap()
}

#[test]
fn coverage_days_follow_the_local_zone() {
    let d = repo();
    usage_at(
        &d,
        &["2026-10-05T23:30:00.000Z", "2026-10-06T00:30:00.000Z"],
    );
    let utc = tune_env(&d, &[], &[("FAEL_TZ_OFFSET", "Z")]);
    assert_eq!(utc["all"]["coverage"]["distinct_days"], 2, "{utc}");
    assert_eq!(utc["days"], jv!(["2026-10-05", "2026-10-06"]));
    let bkk = tune_env(&d, &[], &[("FAEL_TZ_OFFSET", "+07:00")]);
    assert_eq!(bkk["all"]["coverage"]["distinct_days"], 1, "{bkk}");
    assert_eq!(bkk["days"], jv!(["2026-10-06", "2026-10-06"]));
}

#[test]
fn since_cuts_the_window_tune_reads() {
    let d = repo();
    usage_at(
        &d,
        &["2026-10-01T09:00:00.000Z", "2026-10-05T09:00:00.000Z"],
    );
    let all = tune_env(&d, &[], &[]);
    assert_eq!(all["all"]["sizes"]["search_pushes"], 2, "{all}");
    let cut = tune_env(&d, &["--since", "2026-10-03"], &[]);
    assert_eq!(cut["all"]["sizes"]["search_pushes"], 1, "{cut}");
    assert_eq!(cut["days"], jv!(["2026-10-05", "2026-10-05"]));
    let none = tune_env(&d, &["--since", "2099-01-01"], &[]);
    assert_eq!(none["all"]["sizes"]["search_pushes"], 0, "{none}");
    assert_eq!(none["days"], Value::Null);
}

#[test]
fn the_same_usage_prints_the_same_bytes() {
    let d = repo();
    usage_at(
        &d,
        &[
            "2026-10-01T09:00:00.000Z",
            "2026-10-03T09:00:00.000Z",
            "2026-10-05T09:00:00.000Z",
        ],
    );
    for args in [&["--json"][..], &[][..]] {
        let (_, one, _) = fael(&d, &[&["tune"], args].concat(), "");
        let (_, two, _) = fael(&d, &[&["tune"], args].concat(), "");
        assert!(!one.is_empty() && one == two, "{one}\n--\n{two}");
    }
}

fn put_stage(d: &Path, stage: &str) {
    let gate = d.join(".git/fael/cache/push-gate.json");
    std::fs::create_dir_all(gate.parent().unwrap()).unwrap();
    let s = jv!({"policy": "touch@1", "stage": stage, "usage_at": 0, "pending": 0});
    std::fs::write(gate, s.to_string()).unwrap();
}

#[test]
fn tune_names_a_shadow_replay_screening_and_the_arm_stages_validation() {
    let d = repo();
    let (ok, _, err) = fael(
        &d,
        &[
            "add",
            "decision",
            "keep the parser pure",
            "--files",
            "src/a.rs",
        ],
        "",
    );
    assert!(ok, "{err}");
    let stage_line = |d: &Path, stage: &str| {
        let out = tune(d, &[]);
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
    put_stage(&d, "canary");
    let canary = stage_line(&d, "canary");
    assert!(
        canary.contains("validation") && !canary.contains("screening"),
        "{canary}"
    );
}
