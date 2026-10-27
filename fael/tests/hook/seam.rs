//! The read seam: hooks stay silent with no crew.db — no chunk line, no
//! error, and no db created. Core works with no crew installed.

use super::{fael, json};
use std::path::Path;

fn db_of(d: &Path) -> std::path::PathBuf {
    d.join(".git/fael/crew.db")
}

#[test]
fn hooks_with_no_fael_db_stay_silent() {
    let d = super::repo();
    assert!(
        !db_of(&d).exists(),
        "fixture has no crew.db: {}",
        db_of(&d).display()
    );
    // session start: no brief, no chunk line, still ok
    let input = format!(r#"{{"cwd":{}}}"#, json(&d));
    let (ok, out, err) = fael(&d, &["hook", "session-start", "--client", "claude"], &input);
    assert!(ok, "{err}");
    assert!(!out.contains("chunk"), "no db = no chunk line: {out}");
    // stop: stamping a live run with no db is a silent no-op
    let (ok, out, err) = fael(&d, &["hook", "stop", "--client", "claude"], &input);
    assert!(ok, "{err}");
    assert!(out.is_empty(), "stop with no db says nothing: {out}");
    // search (shell): a commit with no db names no chunk
    let payload = format!(
        r#"{{"cwd":{},"session_id":"s1","tool_name":"Bash","tool_input":{{"command":"git commit -m \"fix: x\""}},"tool_response":{{}}}}"#,
        json(&d),
    );
    let (ok, out, err) = fael(&d, &["hook", "search", "--client", "claude"], &payload);
    assert!(ok, "{err}");
    assert!(!out.contains("chunk"), "no db = no held chunk: {out}");
    // the seam never creates the db it could not read
    assert!(
        !db_of(&d).exists(),
        "hooks never create crew.db: {}",
        db_of(&d).display()
    );
}
