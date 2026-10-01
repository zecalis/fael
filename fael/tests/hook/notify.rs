//! The user channel (PLAN-fael-visible-secretary chunk 4): Claude's
//! `systemMessage` carries one line per beat — brief, reminder, receipt —
//! only when fael did something, at most one reminder per file per session,
//! never for codex, and `additionalContext` stays byte-identical with the
//! channel on or off.

use super::{fael, fael_env, json, repo};
use serde_json::Value;
use std::path::Path;

fn hook(d: &Path, event: &str, client: &str, input: &str) -> Option<Value> {
    let (ok, out, err) = fael(d, &["hook", event, "--client", client], input);
    assert!(ok, "{err}");
    (!out.trim().is_empty()).then(|| serde_json::from_str(out.trim()).unwrap())
}

fn read(d: &Path, client: &str) -> Option<Value> {
    let input = format!(
        r#"{{"cwd":{},"session_id":"s1","tool_name":"Read","tool_input":{{"file_path":"src/a.rs"}}}}"#,
        json(d)
    );
    hook(d, "read", client, &input)
}

fn stop(d: &Path) -> Option<Value> {
    let input = format!(r#"{{"cwd":{},"session_id":"s1"}}"#, json(d));
    hook(d, "stop", "claude", &input)
}

fn add(d: &Path, args: &[&str]) {
    let (ok, _, err) = fael_env(d, args, "", &[("CLAUDE_CODE_SESSION_ID", "s1")]);
    assert!(ok, "{err}");
}

#[test]
fn reminder_once_per_file_then_the_turn_receipt() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    // a note alone is no reminder: rows for the agent, nothing for the user
    let (ok, _, err) = fael(
        &d,
        &["add", "note", "just context", "--files", "src/a.rs"],
        "",
    );
    assert!(ok, "{err}");
    let out = read(&d, "claude").unwrap();
    assert!(out["systemMessage"].is_null(), "{out}");
    // a decision is: one line naming its key and title
    let (ok, _, err) = fael(
        &d,
        &[
            "add",
            "decision",
            "keep the cache",
            "--files",
            "src/a.rs",
            "--key",
            "cache:keep",
        ],
        "",
    );
    assert!(ok, "{err}");
    let out = read(&d, "claude").unwrap();
    assert_eq!(
        out["systemMessage"], r#"fael: reminded agent — #cache:keep "keep the cache" (src/a.rs)"#,
        "{out}"
    );
    assert!(out["hookSpecificOutput"]["additionalContext"].is_string());
    // the same file again, with a new decision: rows for the agent, the user
    // already heard about this file
    let (ok, _, err) = fael(
        &d,
        &["add", "decision", "second thought", "--files", "src/a.rs"],
        "",
    );
    assert!(ok, "{err}");
    let out = read(&d, "claude").unwrap();
    assert!(out["systemMessage"].is_null(), "{out}");
    // filed in the session (the shell `add` knows it by CLAUDE_CODE_SESSION_ID)
    add(
        &d,
        &[
            "add",
            "issue",
            "cache misses",
            "--files",
            "src/a.rs",
            "--key",
            "cache:miss",
        ],
    );
    let out = stop(&d).unwrap();
    let line = out["systemMessage"].as_str().unwrap();
    assert!(
        line.starts_with("fael: this turn — filed 1 (#cache:miss)"),
        "{line}"
    );
    assert!(line.contains("reminded 2 (#cache:keep, "), "{line}");
    assert!(out.get("decision").is_none(), "{out}");
    // the next turn did nothing: silence, not "nothing new"
    assert!(stop(&d).is_none());
}

#[test]
fn codex_and_notify_off_print_no_user_line() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    let (ok, _, err) = fael(
        &d,
        &["add", "decision", "keep the cache", "--files", "src/a.rs"],
        "",
    );
    assert!(ok, "{err}");
    let on = read(&d, "codex").unwrap();
    assert!(on["systemMessage"].is_null(), "{on}");
    std::fs::write(
        d.join(".fael/config.toml"),
        "store = \"tracked\"\n[notify]\nuser = false\n",
    )
    .unwrap();
    // a fresh session: the seen list would hide the row otherwise
    let input = format!(
        r#"{{"cwd":{},"session_id":"s2","tool_name":"Read","tool_input":{{"file_path":"src/a.rs"}}}}"#,
        json(&d)
    );
    let off = hook(&d, "read", "claude", &input).unwrap();
    assert!(off["systemMessage"].is_null(), "{off}");
    assert_eq!(
        on["hookSpecificOutput"]["additionalContext"],
        off["hookSpecificOutput"]["additionalContext"],
        "the agent's context never changes with the user channel"
    );
}

#[test]
fn session_start_briefs_the_user_on_issues_said() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    let start = |session: &str| {
        let input = format!(r#"{{"cwd":{},"session_id":"{session}"}}"#, json(&d));
        hook(&d, "session-start", "claude", &input).unwrap()
    };
    let (ok, _, err) = fael(
        &d,
        &[
            "add",
            "issue",
            "login loops",
            "--files",
            "src/a.rs",
            "--key",
            "auth:loop",
        ],
        "",
    );
    assert!(ok, "{err}");
    // filed on this branch: tied, so said in full, so the user hears it
    let on = start("s1");
    assert_eq!(
        on["systemMessage"], "fael: briefed agent — 1 open issue (#auth:loop)",
        "{on}"
    );
    std::fs::write(
        d.join(".fael/config.toml"),
        "store = \"tracked\"\n[notify]\nuser = false\n",
    )
    .unwrap();
    let off = start("s2");
    assert!(off["systemMessage"].is_null(), "{off}");
    assert_eq!(
        on["hookSpecificOutput"]["additionalContext"],
        off["hookSpecificOutput"]["additionalContext"]
    );
}
