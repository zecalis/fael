//! Issue 01M47N67: the MCP server's env can carry a session id the calling
//! session never was — a long-lived server inherits whoever spawned it (an
//! outer session when nested). Inside the server every env id is inherited at
//! spawn, never per call, so no inherited session identifies the caller, even if it recorded an edit.
//! Explicit files remain available without a session stamp.

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

/// A recorded edit proves that the editor existed, not that it called MCP.
#[test]
fn mcp_never_uses_an_editors_identity_or_files_as_caller_evidence() {
    let (_, wt) = main_and_worktree();
    std::fs::write(wt.join("src/b.rs"), "// b\n").unwrap();
    let seed = mcp_tool_env(
        &wt,
        "add",
        &[serde_json::json!({"kind":"note", "text":"seed",
        "files":["src/a.rs"]})],
        &[],
    );
    assert_eq!(seed[0]["result"]["isError"], false);
    let input = serde_json::json!({"cwd":wt, "session":"editor", "files":["src/b.rs"]});
    let mut c = std::process::Command::new(env!("CARGO_BIN_EXE_fael"));
    use std::io::Write as _;
    let mut c = c
        .args(["hook", "edit"])
        .current_dir(&wt)
        .env("FAEL_STATE_DIR", wt.join("../state"))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    c.stdin
        .take()
        .unwrap()
        .write_all(input.to_string().as_bytes())
        .unwrap();
    assert!(c.wait().unwrap().success());
    for key in [
        "FAEL_SESSION",
        "CLAUDE_CODE_SESSION_ID",
        "CODEX_THREAD_ID",
        "",
    ] {
        let env = if key.is_empty() {
            vec![]
        } else {
            vec![(key, "editor")]
        };
        let r = mcp_tool_env(
            &wt,
            "add",
            &[
                serde_json::json!({"kind":"note", "text":"unbound"}),
                serde_json::json!({"kind":"note", "text":format!("explicit {key}"), "files":["src/a.rs"]}),
            ],
            &env,
        );
        assert_eq!(r[0]["result"]["isError"], true, "{r:?}");
        assert_eq!(r[1]["result"]["isError"], false, "{r:?}");
    }
    let log = texts(&wt);
    assert!(!log.contains("\"session\":"), "{log}");
    assert!(!log.contains("unbound"), "{log}");
}
