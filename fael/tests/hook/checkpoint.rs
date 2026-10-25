//! The step commit asks for the chunk checkpoint note: once per gap between
//! notes, silent with no held chunk.

use super::{fael, json};
use std::path::Path;

fn commit(d: &Path) -> String {
    let payload = format!(
        r#"{{"cwd":{},"session_id":"s1","tool_name":"Bash","tool_input":{{"command":"git commit -m \"feat: step\""}},"tool_response":{{}}}}"#,
        json(d),
    );
    let (ok, out, err) = fael(d, &["hook", "search", "--client", "claude"], &payload);
    assert!(ok, "{err}");
    out
}

fn note(d: &Path, uid: &str, text: &str) {
    let (ok, _, err) = fael(d, &["chunk", "note", uid, text], "");
    assert!(ok, "{err}");
}

const ASK: &str = "checkpoint — after this commit";

#[test]
fn a_commit_asks_once_per_gap_between_notes() {
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
    assert!(commit(&d).contains(ASK), "a note then a commit asks again");
}
