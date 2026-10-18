//! h1: the fix-commit line speaks only in a worktree holding a live chunk
//! run — a `fix:` commit naming no row is otherwise silent, and the silent
//! commit spends nothing, so holding a chunk after still speaks the one line.

use super::{fael, json};
use std::path::Path;

fn commit(d: &Path, msg: &str) -> String {
    let payload = format!(
        r#"{{"cwd":{},"session_id":"s1","tool_name":"Bash","tool_input":{{"command":{}}},"tool_response":{{}}}}"#,
        json(d),
        serde_json::to_string(&format!("git commit -m \"{msg}\"")).unwrap(),
    );
    let (ok, out, err) = fael(d, &["hook", "search", "--client", "claude"], &payload);
    assert!(ok, "{err}");
    out
}

/// Hold a live chunk run in this worktree, so the fix-commit line may speak.
fn hold(d: &Path) {
    let (ok, out, err) = fael(d, &["chunk", "add", "T", "--brief", "b"], "");
    assert!(ok, "{err}");
    let uid = out.trim().to_string();
    let (ok, _, err) = fael(d, &["chunk", "start", &uid, "--run", "R1"], "");
    assert!(ok, "{err}");
}

const FIX: &str = "commit names no fael row";

#[test]
fn a_fix_commit_without_a_held_chunk_is_silent() {
    let d = super::repo();
    let out = commit(&d, "fix(hook): cap retries at 3");
    assert!(
        !out.contains(FIX) && !out.contains("context"),
        "no held chunk = silence: {out}"
    );
    // the silent commit spent nothing: holding a chunk after speaks
    hold(&d);
    let out = commit(&d, "fix: another");
    assert!(
        out.contains(FIX) && out.contains("MCP: add"),
        "held chunk = the one-line text: {out}"
    );
}
