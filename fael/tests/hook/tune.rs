//! PLAN-fael-learn-loop chunk 4: `fael tune` replays the candidates over the
//! usage the real writers left. Read-only, no winner named, and `touch@1`'s
//! replay reproduces what the shadow recorded.

use super::working_set::{add, grep, id_of};
use super::{fael, fael_env, json, repo, state};
use serde_json::{Value, json as jv};
use std::path::Path;

/// One search push says the decision (touch 0), the agent pulls it itself and
/// cites it in an edit: the SPEC §7 `missed_push`, through the CLI.
fn session_with_a_missed_push(d: &Path) -> String {
    add(d, "decision", "keep the parser pure", "src/a.rs");
    let id = id_of(d, "keep the parser pure");
    grep(d, "s1", &["src/a.rs"]);
    let (ok, _, err) = fael_env(d, &["find", &id], "", &[("FAEL_SESSION", "s1")]);
    assert!(ok, "{err}");
    let edit = format!(
        r#"{{"cwd":{},"session":"s1","files":["src/b.rs"],"tool_input":{{"new_string":"// per {}"}}}}"#,
        json(d),
        &id[..8]
    );
    let (ok, out, err) = fael(d, &["hook", "edit"], &edit);
    assert!(ok, "{out}{err}");
    id
}

fn tune(d: &Path, args: &[&str]) -> String {
    let mut a = vec!["tune"];
    a.extend(args);
    let (ok, out, err) = fael(d, &a, "");
    assert!(ok, "{out}{err}");
    out
}

#[test]
fn tune_replays_touch_and_matches_what_the_shadow_recorded() {
    let d = repo();
    session_with_a_missed_push(&d);
    let t: Value = serde_json::from_str(&tune(&d, &["--json"])).unwrap();
    let all = &t["all"];
    assert_eq!(t["outcomes_v"], 1, "{t}");
    assert_eq!(all["sizes"]["search_pushes"], 1, "{t}");
    assert_eq!(all["sizes"]["evaluable_rows"], 1, "{t}");
    let pol = |name: &str| {
        all["policies"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["policy"] == name)
            .unwrap_or_else(|| panic!("{name}: {t}"))
    };
    assert_eq!(pol("baseline@1")["dropped"], 0);
    let touch = pol("touch@1");
    // the replay reproduces the shadow's own would_drop
    assert_eq!(touch["dropped"], all["sizes"]["recorded_would_drop"], "{t}");
    assert_eq!(touch["dropped"], 1);
    assert_eq!(touch["missed_push"]["x"], 1, "{touch}");
    assert_eq!(touch["missed_push"]["n"], 1, "{touch}");
    assert!(
        touch["missed_push"]["hi"].as_f64().unwrap() > 0.5,
        "an upper bound, not 1/1"
    );
    // no history yet: touch-yield keeps what it has no evidence on
    assert_eq!(pol("touch-yield@1")["dropped"], 0);
    assert_eq!(all["decay_sweep"].as_array().unwrap().len(), 5);
}

#[test]
fn tune_prints_the_table_and_names_no_winner() {
    let d = repo();
    session_with_a_missed_push(&d);
    let out = tune(&d, &[]);
    for want in [
        "baseline@1",
        "touch@1",
        "touch-yield@1",
        "missed_push",
        "decay sweep",
        "coverage",
    ] {
        assert!(out.contains(want), "{want} missing:\n{out}");
    }
    let low = out.to_lowercase();
    for banned in ["best", "optimal", "recommend", "winner"] {
        assert!(!low.contains(banned), "{banned}:\n{out}");
    }
}

#[test]
fn tune_writes_nothing() {
    let d = repo();
    session_with_a_missed_push(&d);
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

/// Six sessions each search past `src/a.rs` and never touch the decision: the
/// first five are history, so `touch-yield@1` holds the sixth's row back from
/// the cut, and `touch@1` would have cut all six.
fn quiet_sessions(d: &Path, n: usize) {
    add(d, "decision", "keep the parser pure", "src/a.rs");
    for i in 0..n {
        grep(d, &format!("q{i}"), &["src/a.rs"]);
    }
}

#[test]
fn touch_yield_learns_across_sessions_written_by_the_real_hooks() {
    let d = repo();
    quiet_sessions(&d, 6);
    let t = tune_env(&d, &[], &[]);
    let dropped = |name: &str| {
        t["all"]["policies"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["policy"] == name)
            .unwrap()["dropped"]
            .clone()
    };
    assert_eq!(t["all"]["sizes"]["search_pushes"], 6, "{t}");
    assert_eq!(dropped("touch@1"), 6, "{t}");
    assert_eq!(dropped("touch-yield@1"), 1, "{t}");
    assert_eq!(t["all"]["fallback_used"], jv!([1, 0, 0, 0, 5]), "{t}");
}

#[test]
fn the_same_usage_prints_the_same_bytes() {
    let d = repo();
    quiet_sessions(&d, 7);
    for args in [&["--json"][..], &[][..]] {
        let (_, one, _) = fael(&d, &[&["tune"], args].concat(), "");
        let (_, two, _) = fael(&d, &[&["tune"], args].concat(), "");
        assert!(!one.is_empty() && one == two, "{one}\n--\n{two}");
    }
}
