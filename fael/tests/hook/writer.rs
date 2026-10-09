//! A row records the session that filed it (`session`), so a later push can tell
//! "written by session A, used by session B" from "written and used by A".

use super::{fael_env, json, repo};

fn log_text(d: &std::path::Path) -> String {
    let mut out = String::new();
    let mut stack = vec![d.join(".fael/log")];
    while let Some(p) = stack.pop() {
        for e in std::fs::read_dir(&p).unwrap().flatten() {
            let q = e.path();
            if q.is_dir() {
                stack.push(q);
            } else {
                out += &std::fs::read_to_string(q).unwrap();
            }
        }
    }
    out
}

#[test]
fn a_row_carries_the_writer_session_id_never_a_path() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "//\n").unwrap();
    // the hook records edits only in an adopted repo (one row exists), so seed
    // one first — then the transcript path below is a recorded session
    let (ok, _, err) = fael_env(&d, &["add", "note", "seed", "--files", "src/a.rs"], "", &[]);
    assert!(ok, "{err}");
    // the edit hook keys the session by transcript path; the env holds the stem
    let transcript = d.join("t/abc-123.jsonl");
    let input = format!(
        r#"{{"cwd":{},"session_id":"x","transcript_path":{},"tool_input":{{"file_path":{}}}}}"#,
        json(&d),
        json(&transcript),
        json(&d.join("src/a.rs"))
    );
    let (ok, _, err) = fael_env(&d, &["hook", "edit", "--client", "claude"], &input, &[]);
    assert!(ok, "{err}");
    let (ok, _, err) = fael_env(
        &d,
        &["add", "note", "first", "--files", "src/a.rs"],
        "",
        &[("CLAUDE_CODE_SESSION_ID", "abc-123")],
    );
    assert!(ok, "{err}");
    let log = log_text(&d);
    assert!(log.contains(r#""session":"abc-123""#), "{log}");
    assert!(!log.contains("/t/"), "no local path in a shared row: {log}");
}

/// Issue 01M4GEDR: a row filed before the session's first edit still carries
/// the session, since session-start saw the id in this worktree.
#[test]
fn a_row_before_the_first_edit_carries_the_session() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "//\n").unwrap();
    let (ok, _, err) = fael_env(&d, &["add", "note", "seed", "--files", "src/a.rs"], "", &[]);
    assert!(ok, "{err}");
    let input = format!(
        r#"{{"cwd":{},"session_id":"x","transcript_path":{}}}"#,
        json(&d),
        json(&d.join("t/abc-123.jsonl"))
    );
    let (ok, _, err) = fael_env(
        &d,
        &["hook", "session-start", "--client", "claude"],
        &input,
        &[],
    );
    assert!(ok, "{err}");
    let (ok, _, err) = fael_env(
        &d,
        &["add", "note", "first", "--files", "src/a.rs"],
        "",
        &[("CLAUDE_CODE_SESSION_ID", "abc-123")],
    );
    assert!(ok, "{err}");
    let log = log_text(&d);
    assert!(log.contains(r#""session":"abc-123""#), "{log}");
}

#[test]
fn fael_session_tags_a_row_like_claude_code_session_id() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "//\n").unwrap();
    let (ok, _, err) = fael_env(&d, &["add", "note", "seed", "--files", "src/a.rs"], "", &[]);
    assert!(ok, "{err}");
    let session = "2026-09-26T00:00:00.000Z";
    let input = format!(
        r#"{{"cwd":{},"session":{},"tool_input":{{"file_path":{}}}}}"#,
        json(&d),
        serde_json::Value::String(session.into()),
        json(&d.join("src/a.rs"))
    );
    let (ok, _, err) = fael_env(&d, &["hook", "edit", "--client", "opencode"], &input, &[]);
    assert!(ok, "{err}");
    let (ok, _, err) = fael_env(
        &d,
        &["add", "note", "first", "--files", "src/a.rs"],
        "",
        &[("FAEL_SESSION", session)],
    );
    assert!(ok, "{err}");
    let log = log_text(&d);
    assert!(log.contains(&format!(r#""session":"{session}""#)), "{log}");
}

#[test]
fn fael_session_wins_over_claude_code_session_id() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "//\n").unwrap();
    // the plugin sets FAEL_SESSION only inside OpenCode's own shells, so it
    // is fresher than an inherited CLAUDE_CODE_SESSION_ID from an outer shell
    let (ok, _, err) = fael_env(
        &d,
        &["add", "note", "first", "--files", "src/a.rs"],
        "",
        &[
            ("CLAUDE_CODE_SESSION_ID", "abc-123"),
            ("FAEL_SESSION", "2026-09-26T00:00:00.000Z"),
        ],
    );
    assert!(ok, "{err}");
    let log = log_text(&d);
    assert!(
        log.contains(r#""session":"2026-09-26T00:00:00.000Z""#),
        "{log}"
    );
}

#[test]
fn codex_thread_id_tags_a_row_like_claude_code_session_id() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "//\n").unwrap();
    let (ok, _, err) = fael_env(&d, &["add", "note", "seed", "--files", "src/a.rs"], "", &[]);
    assert!(ok, "{err}");
    // Codex exposes CODEX_THREAD_ID to its shell tool executions (but not to
    // stdio MCP servers: openai/codex#19937); the edit hook keys the session
    // by the hook payload's session id, so a `fael add` from that shell joins
    let thread = "019dba93-8214-7d50-a089-9690b4ce6b9e";
    // the path inside `command` is raw patch text, never JSON-quoted: a quoted
    // path names no file, so the hook would record nothing. Build the whole
    // payload through serde so a Windows path's backslashes stay a valid JSON
    // string while the parsed `command` still holds the bare path.
    let command = format!("*** Update File: {}", d.join("src/a.rs").display());
    let input = serde_json::json!({
        "cwd": d.to_string_lossy(),
        "session_id": thread,
        "tool_input": {"command": command},
    })
    .to_string();
    let (ok, _, err) = fael_env(&d, &["hook", "edit", "--client", "codex"], &input, &[]);
    assert!(ok, "{err}");
    let (ok, _, err) = fael_env(
        &d,
        &["add", "note", "first", "--files", "src/a.rs"],
        "",
        &[("CODEX_THREAD_ID", thread)],
    );
    assert!(ok, "{err}");
    let log = log_text(&d);
    assert!(log.contains(&format!(r#""session":"{thread}""#)), "{log}");
}

#[test]
fn outside_a_session_a_row_has_no_session() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "//\n").unwrap();
    let mut c = std::process::Command::new(env!("CARGO_BIN_EXE_fael"));
    let o = c
        .args(["add", "note", "first", "--files", "src/a.rs"])
        .current_dir(&d)
        .env("FAEL_STATE_DIR", d.join("state"))
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env_remove("FAEL_SESSION")
        .env_remove("CODEX_THREAD_ID")
        .output()
        .unwrap();
    assert!(o.status.success());
    assert!(!log_text(&d).contains("session"));
}

/// Issue 01M47N67: a client session id no hook event ever recorded is a
/// stranger's — a long-lived MCP server inherits whoever spawned it (an outer
/// session when nested). The row files without a session rather than under
/// the wrong one; `FAEL_SESSION` (set per command by the plugin) still reads
/// back raw — see `fael_session_wins_over_claude_code_session_id`.
#[test]
fn unrecorded_client_session_tags_nothing() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "//\n").unwrap();
    let (ok, _, err) = fael_env(
        &d,
        &["add", "note", "stranger", "--files", "src/a.rs"],
        "",
        &[("CLAUDE_CODE_SESSION_ID", "3735bde2")],
    );
    assert!(ok, "{err}");
    assert!(!log_text(&d).contains("\"session\":"), "{}", log_text(&d));
}

/// A Claude session-start in `d` under `id`, keyed by its transcript path.
fn start(d: &std::path::Path, id: &str, source: &str) {
    let input = format!(
        r#"{{"cwd":{},"session_id":"{id}","transcript_path":{},"source":"{source}"}}"#,
        json(d),
        json(&d.join(format!("t/{id}.jsonl")))
    );
    let (ok, _, err) = fael_env(
        d,
        &["hook", "session-start", "--client", "claude"],
        &input,
        &[],
    );
    assert!(ok, "{err}");
}

/// Issue 01M4GEDR: a session's first CLI row, before any edit, carries the
/// session its session-start registered here. The start is no edit (`add`
/// without --files still asks for them), a second start is harmless, and
/// another session's start vouches for nobody else.
#[test]
fn a_session_start_lets_the_first_cli_row_carry_the_session() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "//\n").unwrap();
    let (ok, _, err) = fael_env(&d, &["add", "note", "seed", "--files", "src/a.rs"], "", &[]);
    assert!(ok, "{err}");
    start(&d, "abc-123", "startup");
    start(&d, "abc-123", "compact");
    start(&d, "other-9", "startup");
    let me = [("CLAUDE_CODE_SESSION_ID", "abc-123")];
    let (ok, _, err) = fael_env(&d, &["add", "note", "no files named"], "", &me);
    assert!(!ok, "a session-start is not an edit: {err}");
    for (text, id) in [("first", "abc-123"), ("stranger", "zzz-000")] {
        let env = [("CLAUDE_CODE_SESSION_ID", id)];
        let (ok, _, err) = fael_env(&d, &["add", "note", text, "--files", "src/a.rs"], "", &env);
        assert!(ok, "{err}");
    }
    let log = log_text(&d);
    assert!(log.contains(r#""session":"abc-123""#), "{log}");
    assert_eq!(log.matches(r#""session":"#).count(), 1, "{log}");
}

/// 01M47N67 still holds over MCP: the server's env may name a session it
/// outlived, so a registered session-start does not vouch for it there.
#[test]
fn a_session_start_does_not_vouch_for_the_mcp_server() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "//\n").unwrap();
    let (ok, _, err) = fael_env(&d, &["add", "note", "seed", "--files", "src/a.rs"], "", &[]);
    assert!(ok, "{err}");
    start(&d, "abc-123", "startup");
    let rpc = serde_json::json!({"jsonrpc": "2.0", "id": 0, "method": "tools/call",
        "params": {"name": "add", "arguments": {"kind": "note", "text": "over mcp",
        "files": ["src/a.rs"], "cwd": d}}});
    let env = [("CLAUDE_CODE_SESSION_ID", "abc-123")];
    let (ok, out, err) = fael_env(&d, &["mcp"], &format!("{rpc}\n"), &env);
    assert!(ok && out.contains("recorded"), "{out}{err}");
    assert!(!log_text(&d).contains(r#""session":"#), "{}", log_text(&d));
}

/// The edit ask never asks a session about the row it just filed: with the
/// session stamped at its start, `own_row` knows the row is its own.
#[test]
fn the_edit_ask_skips_the_row_the_session_filed_before_its_first_edit() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// v1\n").unwrap();
    let (ok, _, err) = fael_env(&d, &["add", "note", "seed", "--files", "src/a.rs"], "", &[]);
    assert!(ok, "{err}");
    start(&d, "abc-123", "startup");
    let me = [("CLAUDE_CODE_SESSION_ID", "abc-123")];
    let (ok, _, err) = fael_env(
        &d,
        &["add", "issue", "login loops here", "--files", "src/a.rs"],
        "",
        &me,
    );
    assert!(ok, "{err}");
    std::fs::write(d.join("src/a.rs"), "// v2 the fix\n").unwrap();
    let input = format!(
        r#"{{"cwd":{},"session_id":"abc-123","transcript_path":{},"tool_input":{{"file_path":{}}}}}"#,
        json(&d),
        json(&d.join("t/abc-123.jsonl")),
        json(&d.join("src/a.rs"))
    );
    let (ok, out, err) = fael_env(&d, &["hook", "edit", "--client", "claude"], &input, &[]);
    assert!(ok, "{err}");
    assert!(!out.contains("changed since"), "{out}");
}

/// A detached HEAD names no branch, so the row carries the commit and no
/// branch — never a guessed one (a commit can sit on many branches).
#[test]
fn a_detached_head_stamps_the_sha_and_no_branch() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "//\n").unwrap();
    let git = |args: &[&str]| {
        let o = std::process::Command::new("git")
            .args(args)
            .current_dir(&d)
            .output()
            .unwrap();
        assert!(o.status.success(), "{o:?}");
        String::from_utf8_lossy(&o.stdout).trim().to_string()
    };
    git(&["switch", "-q", "--detach", "HEAD"]);
    let sha = git(&["rev-parse", "--short", "HEAD"]);
    let (ok, _, err) = fael_env(
        &d,
        &["add", "note", "on detached", "--files", "src/a.rs"],
        "",
        &[],
    );
    assert!(ok, "{err}");
    let log = log_text(&d);
    assert!(log.contains(&format!(r#""sha":"{sha}"#)), "{log}");
    assert!(!log.contains(r#""branch":"#), "{log}");
}
