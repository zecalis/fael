//! The step commit asks for the chunk checkpoint note: once per run per
//! session, silent with no held chunk.

use super::{fael, json};
use std::path::Path;

fn commit_as(d: &Path, session: &str) -> String {
    let payload = format!(
        r#"{{"cwd":{},"session_id":{},"tool_name":"Bash","tool_input":{{"command":"git commit -m \"feat: step\""}},"tool_response":{{}}}}"#,
        json(d),
        serde_json::Value::String(session.to_string()),
    );
    let (ok, out, err) = fael(d, &["hook", "search", "--client", "claude"], &payload);
    assert!(ok, "{err}");
    out
}

fn commit(d: &Path) -> String {
    commit_as(d, "s1")
}

fn note(d: &Path, uid: &str, text: &str) {
    let (ok, _, err) = fael(d, &["chunk", "note", uid, text], "");
    assert!(ok, "{err}");
}

const ASK: &str = "checkpoint — after this commit";

#[test]
fn a_commit_asks_once_per_run_per_session() {
    let d = super::repo();
    assert!(!commit(&d).contains(ASK), "no crew.db: silent");
    let (ok, out, err) = fael(&d, &["chunk", "add", "T", "--brief", "b"], "");
    assert!(ok, "{err}");
    let uid = out.trim().to_string();
    assert!(!commit(&d).contains(ASK), "no live run: silent");
    let (ok, _, err) = fael(&d, &["chunk", "start", &uid, "--run", "R1"], "");
    assert!(ok, "{err}");
    let first = commit(&d);
    assert!(first.contains(ASK) && first.contains(&uid), "{first}");
    assert!(!commit(&d).contains(ASK), "no note between: key spent");
    note(&d, &uid, "step 1 done; next step 2");
    assert!(
        !commit(&d).contains(ASK),
        "a note then a commit in the same session stays silent"
    );
    let other = commit_as(&d, "s2");
    assert!(
        other.contains(ASK) && other.contains(&uid),
        "a new session on the same run asks again once: {other}"
    );
    assert!(
        !commit_as(&d, "s2").contains(ASK),
        "the new session asks only once"
    );
}
