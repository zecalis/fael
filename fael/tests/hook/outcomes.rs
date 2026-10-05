//! PLAN-fael-learn-loop chunk 2: observed outcomes. A said id typed into a
//! tool input or the closing reply is `cited` once per session; a `fael …`
//! command, a tool response and an id fael never said are not. `fael stats
//! --rows` joins the lines into per-row outcomes under `outcomes_v: 1`.

use super::{fael, fael_env, json, repo};
use serde_json::{Value, json};
use std::path::Path;

fn call(d: &Path, event: &str, session: &str, rest: &str) {
    let input = format!(r#"{{"cwd":{},"session":"{session}",{rest}}}"#, json(d));
    let (ok, out, err) = fael(d, &["hook", event], &input);
    assert!(ok, "{out}{err}");
}

fn usage(d: &Path) -> Vec<Value> {
    std::fs::read_to_string(d.join("state/usage.jsonl"))
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

fn cited(d: &Path) -> Vec<Value> {
    usage(d)
        .into_iter()
        .filter(|l| l["event"] == "outcome")
        .map(|l| l["cited"].clone())
        .collect()
}

/// One decision on `src/a.rs`, pushed to session `s1` by a read: its id.
fn said(d: &Path) -> String {
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    let (ok, _, err) = fael(
        d,
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
    call(d, "read", "s1", r#""files":["src/a.rs"]"#);
    let read = usage(d).into_iter().find(|l| l["event"] == "read").unwrap();
    read["ids"][0].as_str().unwrap().to_string()
}

fn row(d: &Path, id: &str) -> Value {
    let (ok, out, err) = fael(d, &["stats", "--json", "--rows"], "");
    assert!(ok, "{err}");
    let s: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(s["outcomes_v"], 1, "{out}");
    s["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == id)
        .unwrap_or_else(|| panic!("{id} not in {out}"))
        .clone()
}

#[test]
fn a_said_id_in_a_tool_input_is_cited_once() {
    let d = repo();
    let id = said(&d);
    let short = &id[..8];
    let edit = format!(r#""files":["src/b.rs"],"tool_input":{{"new_string":"// per {short}"}}"#);
    call(&d, "edit", "s1", &edit);
    assert_eq!(cited(&d), [json!([id])]);
    call(&d, "edit", "s1", &edit);
    assert_eq!(cited(&d).len(), 1, "once per session");
    let r = row(&d, &id);
    assert_eq!(r["outcomes"]["shown"], 1, "{r}");
    assert_eq!(r["outcomes"]["cited"], 1, "{r}");
}

#[test]
fn a_fael_command_a_response_and_an_unsaid_id_are_no_cite() {
    let d = repo();
    let id = said(&d);
    let short = &id[..8];
    // `fael find <id>` is a pull
    let cmd = format!(r#""tool":"Bash","tool_input":{{"command":"fael find {short}"}}"#);
    call(&d, "search", "s1", &cmd);
    // the id only in the tool's response
    let resp =
        format!(r#""tool":"Bash","tool_input":{{"command":"ls"}},"tool_response":"{short}""#);
    call(&d, "search", "s1", &resp);
    // another session was never told it
    let edit = format!(r#""files":["src/b.rs"],"tool_input":{{"new_string":"{short}"}}"#);
    call(&d, "edit", "s2", &edit);
    assert!(cited(&d).is_empty(), "{:?}", usage(&d));
    assert_eq!(row(&d, &id)["outcomes"]["cited"], 0);
}

#[test]
fn the_closing_reply_cites_a_said_id() {
    let d = repo();
    let id = said(&d);
    call(
        &d,
        "stop",
        "s1",
        &format!(r#""reply":"fixed, as {id} says""#),
    );
    assert_eq!(cited(&d), [json!([id])]);
}

#[test]
fn a_pull_is_agent_initiated_unless_fael_pointed_at_it() {
    let d = repo();
    let id = said(&d);
    let env = [("FAEL_SESSION", "s1")];
    let (ok, _, err) = fael_env(&d, &["find", &id], "", &env);
    assert!(ok, "{err}");
    let o = &row(&d, &id)["outcomes"];
    assert_eq!(
        o["pulled"],
        json!({"agent_initiated": 1, "fael_induced": 0})
    );
    assert_eq!(o["cited"], 0, "a pull is no cite: {o}");
}
