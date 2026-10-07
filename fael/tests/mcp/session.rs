//! Issue 01M47N67: the MCP server's env can carry a session id the calling
//! session never was — a long-lived server inherits whoever spawned it (an
//! outer session when nested). Inside the server every env id is inherited at
//! spawn, never per call, so only a hook-recorded session stamps — not a
//! client var, and not `FAEL_SESSION` either (whose per-command freshness only
//! holds for a CLI call, not this process).

use super::{main_and_worktree, mcp_tool_env, texts};

/// A client id no hook event ever recorded stamps nothing, never a stranger's.
#[test]
fn mcp_add_with_an_unrecorded_session_tags_nothing() {
    let (_, wt) = main_and_worktree();
    let r = mcp_tool_env(
        &wt,
        "add",
        &[serde_json::json!({"kind": "note", "text": "stranger row", "files": ["src/a.rs"]})],
        &[("CLAUDE_CODE_SESSION_ID", "3735bde2")],
    );
    assert!(r[0]["result"]["isError"] == false, "{r:?}");
    let log = texts(&wt);
    assert!(log.contains("stranger row"), "{log}");
    assert!(!log.contains("\"session\":"), "{log}");
}

/// The MCP server cannot read `FAEL_SESSION` as the caller: it is inherited at
/// spawn, so an unrecorded one stamps nothing here (`shell.env` only freshens
/// a CLI call's env — `fael/tests/hook/writer.rs` pins that half).
#[test]
fn mcp_add_with_an_unrecorded_fael_session_tags_nothing() {
    let (_, wt) = main_and_worktree();
    let r = mcp_tool_env(
        &wt,
        "add",
        &[serde_json::json!({"kind": "note", "text": "inherited row", "files": ["src/a.rs"]})],
        &[("FAEL_SESSION", "2026-09-26T00:00:00.000Z")],
    );
    assert!(r[0]["result"]["isError"] == false, "{r:?}");
    let log = texts(&wt);
    assert!(log.contains("inherited row"), "{log}");
    assert!(!log.contains("\"session\":"), "{log}");
}
