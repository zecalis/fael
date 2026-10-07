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
