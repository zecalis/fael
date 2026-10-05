//! PLAN-fael-learn-loop chunk 5: `push_policy = "touch@1"` is the validation
//! experiment. A candidate session's search push loses the rows the gate cuts
//! (`cut: gate`), a holdout session keeps `baseline@1` and sees everything;
//! with no gate configured nothing changes. The default config is the last.

use super::working_set::add;
use super::{fael, json, repo, state};
use serde_json::{Value, json};
use std::path::Path;

fn config(d: &Path, holdout: u32) {
    std::fs::write(
        d.join(".fael/config.toml"),
        format!("push_policy = \"touch@1\"\npush_holdout = {holdout}\n"),
    )
    .unwrap();
}

/// A Grep whose hit list names `a.rs`: what the agent was told, and the line.
fn grep(d: &Path) -> (String, Value) {
    let p = format!(
        r#"{{"cwd":{},"session_id":"s1","tool_name":"Grep","tool_input":{{"pattern":"x"}},"tool_response":{{"filenames":{}}}}}"#,
        json(d),
        json!(["src/a.rs"])
    );
    let (ok, out, err) = fael(d, &["hook", "search", "--client", "claude"], &p);
    assert!(ok, "{err}");
    let usage = std::fs::read_to_string(state(d).join("usage.jsonl")).unwrap();
    let l = usage
        .lines()
        .map(|l| serde_json::from_str::<Value>(l).expect(l))
        .rfind(|l| l["event"] == "search")
        .expect(&usage);
    (out, l)
}

fn seeded() -> std::path::PathBuf {
    let d = repo();
    add(&d, "decision", "keep the parser pure", "src/a.rs");
    add(&d, "issue", "parser loops", "src/a.rs");
    d
}

#[test]
fn a_candidate_session_loses_the_untouched_decision_and_the_line_says_why() {
    let d = seeded();
    config(&d, 0); // nobody held out
    let (out, l) = grep(&d);
    assert!(!out.contains("keep the parser pure"), "gated: {out}");
    assert!(
        out.contains("parser loops"),
        "an issue is never gated: {out}"
    );
    assert_eq!(
        (&l["policy"], &l["arm"]),
        (&json!("touch@1"), &json!("candidate"))
    );
    assert_eq!(l["cut"].as_array().unwrap().len(), 1, "{l}");
    assert_eq!(l["cut"][0]["r"], "gate", "{l}");
    assert!(l.get("would_drop").is_none(), "nothing left to drop: {l}");
}

#[test]
fn a_push_the_gate_left_silent_is_still_recorded() {
    let d = repo();
    add(&d, "decision", "keep the parser pure", "src/a.rs");
    config(&d, 0);
    let (out, l) = grep(&d);
    assert!(!out.contains("keep the parser pure"), "{out}");
    assert_eq!(l["ids"], json!([]), "{l}");
    assert_eq!(l["cut"][0]["r"], "gate", "{l}");
    assert_eq!(l["arm"], "candidate", "{l}");
}

#[test]
fn a_holdout_session_sees_everything_and_the_shadow_still_records() {
    let d = seeded();
    config(&d, 100); // everybody held out
    let (out, l) = grep(&d);
    assert!(out.contains("keep the parser pure"), "{out}");
    assert_eq!(
        (&l["policy"], &l["arm"]),
        (&json!("baseline@1"), &json!("holdout"))
    );
    assert!(l.get("cut").is_none(), "{l}");
    assert_eq!(l["would_drop"]["policy"], "touch@1", "{l}");
}

#[test]
fn the_default_config_changes_nothing() {
    let d = seeded();
    let (out, l) = grep(&d);
    assert!(out.contains("keep the parser pure"), "{out}");
    assert_eq!(
        (&l["policy"], &l["arm"]),
        (&json!("baseline@1"), &json!("all"))
    );
}

#[test]
fn an_unknown_policy_is_rejected_not_ignored() {
    let d = seeded();
    std::fs::write(d.join(".fael/config.toml"), "push_policy = \"touch@9\"\n").unwrap();
    let (_, _, err) = fael(&d, &["find", "parser"], "");
    assert!(err.contains("push_policy"), "{err}");
}

#[test]
fn tune_reports_the_experiment_and_says_insufficient_with_one_session() {
    let d = seeded();
    config(&d, 0);
    grep(&d);
    let (ok, out, err) = fael(&d, &["tune"], "");
    assert!(ok, "{err}");
    assert!(out.contains("validation · candidate touch@1"), "{out}");
    assert!(out.contains("verdict: insufficient_data"), "{out}");
    assert!(out.contains("dup is not measured"), "{out}");
    let (_, json, _) = fael(&d, &["tune", "--json"], "");
    let t: Value = serde_json::from_str(&json).unwrap();
    assert_eq!(
        t["validation"]["strata"][0]["candidate"]["gate_cuts"], 1,
        "{t}"
    );
    // no experiment, no block
    let d = seeded();
    grep(&d);
    let (_, out, _) = fael(&d, &["tune"], "");
    assert!(!out.contains("validation"), "{out}");
}
