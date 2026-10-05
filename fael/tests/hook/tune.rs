//! PLAN-fael-learn-loop chunk 4: `fael tune` replays the candidates over the
//! usage the real writers left. Read-only, no winner named, and `touch@1`'s
//! replay reproduces what the shadow recorded.

use super::working_set::{add, grep, id_of};
use super::{fael, fael_env, json, repo, state};
use serde_json::Value;
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
