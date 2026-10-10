//! PLAN-fael-learn-loop chunk 1: every edit push's usage line is its decision
//! record — the trigger, the files, the policy `baseline@1`, the features of
//! each row said or cut, and why a row was cut (`cap` / `hub_peek` /
//! `budget`). The record is usage-line only: what the agent sees is untouched
//! (the replay and `push_cap` tests pin that).

use super::{fael, json, repo, state};
use serde_json::{Value, json};
use std::path::Path;

fn add(d: &Path, kind: &str, text: &str, file: &str) {
    std::fs::write(d.join(file), "// x\n").unwrap();
    let (ok, _, err) = fael(d, &["add", kind, text, "--files", file], "");
    assert!(ok, "{err}");
}

/// An issue plus `n` decisions on `src/a.rs`; past 8 decisions it is a hub.
fn seed(d: &Path, n: usize) {
    add(d, "issue", "login loops", "src/a.rs");
    for i in 0..n {
        add(d, "decision", &format!("decision {i}"), "src/a.rs");
    }
}

/// The last usage line of `event`.
fn line(d: &Path, event: &str) -> Value {
    let usage = std::fs::read_to_string(state(d).join("usage.jsonl")).unwrap();
    usage
        .lines()
        .map(|l| serde_json::from_str::<Value>(l).expect(l))
        .rfind(|l| l["event"] == event)
        .expect(&usage)
}

fn push(d: &Path, event: &str, file: &str) {
    let input = format!(
        r#"{{"cwd":{},"session":"s1","files":["{file}"]}}"#,
        super::json(d)
    );
    let (ok, out, err) = fael(d, &["hook", event], &input);
    assert!(ok, "{out}{err}");
}

fn reasons(l: &Value) -> Vec<&str> {
    l["cut"].as_array().map_or(vec![], |c| {
        c.iter().map(|c| c["r"].as_str().unwrap()).collect()
    })
}

#[test]
fn an_edit_push_records_trigger_files_policy_and_row_features() {
    let d = repo();
    seed(&d, 1);
    push(&d, "edit", "src/a.rs");
    let l = line(&d, "edit");
    assert_eq!(l["trigger"], "edit", "{l}");
    assert_eq!(l["files"], json!(["src/a.rs"]), "{l}");
    assert_eq!(l["policy"], "baseline@1", "{l}");
    assert!(l.get("cut").is_none() && l.get("cut_n").is_none(), "{l}");
    assert_eq!(l["arm"], "all", "no gate configured, no experiment: {l}");
    // every said row carries its features
    let ids = l["ids"].as_array().unwrap();
    assert_eq!(ids.len(), 2, "{l}");
    for id in ids {
        let f = &l["feat"][id.as_str().unwrap()];
        assert_eq!(f["tier"], 0, "{l}");
        assert_eq!(f["hub"], false, "{l}");
        assert_eq!(f["age_d"], 0, "{l}");
        assert!(
            ["issue", "decision"].contains(&f["kind"].as_str().unwrap()),
            "{l}"
        );
    }
}

#[test]
fn the_row_cap_cuts_with_reason_cap() {
    let d = repo();
    seed(&d, 7); // 8 rows, push_rows 5
    push(&d, "edit", "src/a.rs");
    let l = line(&d, "edit");
    assert_eq!(l["ids"].as_array().unwrap().len(), 5, "{l}");
    assert_eq!(reasons(&l), ["cap"; 3], "{l}");
    assert_eq!(l["cut_n"], 3, "{l}");
    // a cut row has features too, and was never said
    let cut = l["cut"][0]["id"].as_str().unwrap();
    assert!(l["feat"][cut]["kind"] == "decision", "{l}");
    assert!(
        !l["ids"].as_array().unwrap().iter().any(|i| i == cut),
        "{l}"
    );
}

#[test]
fn a_hub_push_cuts_with_reason_hub_peek() {
    let d = repo();
    seed(&d, 15); // issue + 3 peek said, 12 cut
    push(&d, "edit", "src/a.rs");
    let l = line(&d, "edit");
    assert_eq!(l["ids"].as_array().unwrap().len(), 4, "{l}");
    // the cap leaves room for 4 File rows (5 less the issue): the hub peek
    // alone cut the 4th, the row cap would have cut the other 11 anyway
    let mut want = vec!["hub_peek"];
    want.extend(["cap"; 11]);
    assert_eq!(reasons(&l), want, "{l}");
    assert_eq!(l["cut_n"], 12, "{l}");
    let id = l["cut"][0]["id"].as_str().unwrap();
    assert_eq!(l["feat"][id]["hub"], true, "{l}");
    // the open issue is a Now row: the hub rule never applies to it
    let issue = l["ids"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| l["feat"][i.as_str().unwrap()]["kind"] == "issue")
        .unwrap();
    assert_eq!(l["feat"][issue.as_str().unwrap()]["hub"], false, "{l}");
}

#[test]
fn the_record_caps_cut_at_twenty_and_counts_them_all() {
    let d = repo();
    seed(&d, 29); // issue + 3 peek said, 26 cut
    push(&d, "edit", "src/a.rs");
    let l = line(&d, "edit");
    assert_eq!(l["cut"].as_array().unwrap().len(), 20, "{l}");
    assert_eq!(l["cut_n"], 26, "{l}");
}

#[test]
fn the_token_budget_cuts_with_reason_budget() {
    let d = repo();
    seed(&d, 3);
    std::fs::write(d.join(".fael/config.toml"), "[budget]\npush_tokens = 40\n").unwrap();
    push(&d, "edit", "src/a.rs");
    let l = line(&d, "edit");
    let said = l["ids"].as_array().unwrap().len();
    assert!(said > 0 && said < 4, "{l}");
    assert_eq!(reasons(&l), vec!["budget"; 4 - said], "{l}");
}

/// One `PostToolUse` call through `fael hook search`; the last `event` line.
/// A fresh session per call: rows are said once per session.
fn call(d: &Path, session: &str, event: &str, tool: &str, input: &str, resp: &str) -> Value {
    let p = format!(
        r#"{{"cwd":{},"session_id":"{session}","tool_name":"{tool}","tool_input":{input},"tool_response":{resp}}}"#,
        json(d)
    );
    let (ok, _, err) = fael(d, &["hook", "search", "--client", "claude"], &p);
    assert!(ok, "{err}");
    line(d, event)
}

#[test]
fn an_edit_names_its_trigger() {
    let d = repo();
    seed(&d, 1);
    push(&d, "edit", "src/a.rs");
    assert_eq!(line(&d, "edit")["trigger"], "edit");
}

#[test]
fn a_shell_write_names_its_trigger() {
    let d = repo();
    seed(&d, 1);
    // a file just written, named by a shell call: that call edited it
    let sed = r#"{"command":"sed -i s/x/y/ src/a.rs"}"#;
    let l = call(&d, "t1", "shell-edit", "Bash", sed, r#"{"stdout":""}"#);
    assert_eq!(l["trigger"], "shell-edit", "{l}");
}

#[test]
fn no_row_cap_records_no_cut() {
    let d = repo();
    seed(&d, 15);
    std::fs::write(d.join(".fael/config.toml"), "[budget]\npush_rows = 0\n").unwrap();
    push(&d, "edit", "src/a.rs");
    let l = line(&d, "edit");
    assert_eq!(l["ids"].as_array().unwrap().len(), 16, "{l}");
    assert!(l.get("cut").is_none(), "{l}");
    assert_eq!(l["feat"].as_object().unwrap().len(), 16, "{l}");
}
