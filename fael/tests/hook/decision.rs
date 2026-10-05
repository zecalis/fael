//! PLAN-fael-learn-loop chunk 1: every push's usage line is its decision
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
fn a_read_push_records_trigger_files_policy_and_row_features() {
    let d = repo();
    seed(&d, 1);
    push(&d, "read", "src/a.rs");
    let l = line(&d, "read");
    assert_eq!(l["trigger"], "read", "{l}");
    assert_eq!(l["files"], json!(["src/a.rs"]), "{l}");
    assert_eq!(l["policy"], "baseline@1", "{l}");
    assert!(l.get("cut").is_none() && l.get("cut_n").is_none(), "{l}");
    assert!(l.get("arm").is_none(), "no arm before the holdout: {l}");
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
    push(&d, "read", "src/a.rs");
    let l = line(&d, "read");
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
    push(&d, "read", "src/a.rs");
    let l = line(&d, "read");
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
    push(&d, "read", "src/a.rs");
    let l = line(&d, "read");
    assert_eq!(l["cut"].as_array().unwrap().len(), 20, "{l}");
    assert_eq!(l["cut_n"], 26, "{l}");
}

#[test]
fn the_token_budget_cuts_with_reason_budget() {
    let d = repo();
    seed(&d, 3);
    std::fs::write(d.join(".fael/config.toml"), "[budget]\npush_tokens = 40\n").unwrap();
    push(&d, "read", "src/a.rs");
    let l = line(&d, "read");
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

/// A minute old: past the window that makes a named file a shell edit.
fn backdate(f: &Path) {
    let old = std::time::SystemTime::now() - std::time::Duration::from_secs(60);
    let file = std::fs::File::options().write(true).open(f).unwrap();
    file.set_modified(old).unwrap();
}

#[test]
fn an_edit_names_its_trigger() {
    let d = repo();
    seed(&d, 1);
    push(&d, "edit", "src/a.rs");
    assert_eq!(line(&d, "edit")["trigger"], "edit");
}

#[test]
fn a_search_names_what_made_the_file_a_touch() {
    let d = repo();
    seed(&d, 1);
    // a file just written, named by a shell call: that call edited it
    let sed = r#"{"command":"sed -i s/x/y/ src/a.rs"}"#;
    let l = call(&d, "t1", "shell-edit", "Bash", sed, r#"{"stdout":""}"#);
    assert_eq!(l["trigger"], "shell-edit", "{l}");
    backdate(&d.join("src/a.rs"));
    // a reader command that names it
    let cat = r#"{"command":"cat src/a.rs"}"#;
    let l = call(&d, "t2", "search", "Bash", cat, r#"{"stdout":""}"#);
    assert_eq!(l["trigger"], "reader-arg", "{l}");
    // a grep whose hit list names one file
    let l = call(
        &d,
        "t3",
        "search",
        "Grep",
        r#"{"pattern":"x"}"#,
        r#"{"filenames":["src/a.rs"]}"#,
    );
    assert_eq!(l["trigger"], "hitlist", "{l}");
    // the same from a glob
    let l = call(
        &d,
        "t4",
        "search",
        "Glob",
        r#"{"pattern":"src/*.rs"}"#,
        r#"{"filenames":["src/a.rs"]}"#,
    );
    assert_eq!(l["trigger"], "glob", "{l}");
    // a named file wins over the hit list that repeats it
    let grep = r#"{"command":"grep x src/a.rs"}"#;
    let l = call(
        &d,
        "t5",
        "search",
        "Bash",
        grep,
        r#"{"stdout":"src/a.rs:1:x"}"#,
    );
    assert_eq!(l["trigger"], "reader-arg", "{l}");
}

#[test]
fn no_row_cap_records_no_cut() {
    let d = repo();
    seed(&d, 15);
    std::fs::write(d.join(".fael/config.toml"), "[budget]\npush_rows = 0\n").unwrap();
    push(&d, "read", "src/a.rs");
    let l = line(&d, "read");
    assert_eq!(l["ids"].as_array().unwrap().len(), 16, "{l}");
    assert!(l.get("cut").is_none(), "{l}");
    assert_eq!(l["feat"].as_object().unwrap().len(), 16, "{l}");
}

fn add_files(d: &Path, kind: &str, text: &str, files: &str) {
    for f in files.split(',') {
        std::fs::write(d.join(f), "// x\n").unwrap();
    }
    let (ok, _, err) = fael(d, &["add", kind, text, "--files", files], "");
    assert!(ok, "{err}");
}

/// The id of the row whose text is `text` (`find --json` prints row lines).
fn id_of(d: &Path, text: &str) -> String {
    let (_, out, _) = fael(d, &["find", text, "--json"], "");
    out.lines()
        .map(|l| serde_json::from_str::<Value>(l).expect(l))
        .find(|r| r["text"] == text)
        .expect(&out)["id"]
        .as_str()
        .unwrap()
        .to_string()
}

/// PLAN-fael-learn-loop chunk 3: `feat.touch` counts a row's files already in
/// the session's working set, and `would_drop` names the said rows `touch@1`
/// would have cut — recorded only, the agent still saw every one.
#[test]
fn touch_counts_the_working_set_and_would_drop_names_the_untouched() {
    let d = repo();
    add_files(&d, "issue", "login loops", "src/a.rs");
    push(&d, "read", "src/a.rs");
    let first = line(&d, "read");
    // nothing touched before the first push: the issue stays (never dropped)
    let issue = first["ids"][0].as_str().unwrap().to_string();
    assert_eq!(first["feat"][&issue]["touch"], 0, "{first}");
    assert!(first.get("would_drop").is_none(), "{first}");

    // filed after a.rs was touched: one row spans it, one is b.rs alone
    add_files(&d, "decision", "spans both", "src/a.rs,src/b.rs");
    add_files(&d, "decision", "only b", "src/b.rs");
    push(&d, "read", "src/b.rs");
    let l = line(&d, "read");
    let span = id_of(&d, "spans both");
    let only = id_of(&d, "only b");
    assert_eq!(l["feat"][&span]["touch"], 1, "{l}");
    assert_eq!(l["feat"][&only]["touch"], 0, "{l}");
    assert_eq!(
        l["would_drop"],
        json!({"policy": "touch@1", "ids": [only]}),
        "{l}"
    );
    // shadow only: both rows were said
    let said = l["ids"].as_array().unwrap();
    assert!(
        said.contains(&json!(span)) && said.contains(&json!(only)),
        "{l}"
    );
}

#[test]
fn no_session_means_no_working_set() {
    let d = repo();
    seed(&d, 1);
    let input = format!(r#"{{"cwd":{},"files":["src/a.rs"]}}"#, json(&d));
    let (ok, out, err) = fael(&d, &["hook", "read"], &input);
    assert!(ok, "{out}{err}");
    let l = line(&d, "read");
    let id = l["ids"][0].as_str().unwrap();
    assert!(l["feat"][id].get("touch").is_none(), "{l}");
    assert!(l.get("would_drop").is_none(), "{l}");
}
