//! Rejects and warnings land in usage.jsonl with their ask type — CLI and
//! MCP alike — and `stats` shows the split.

use super::{fael, repo, stats_json, usage};

#[test]
fn reject_cli_counts_with_command_event() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    let (ok, _, err) = fael(&d, &["add", "bogus", "zz", "--files", "src/a.rs"], "");
    assert!(!ok && err.contains("rejected: kind must be"), "{err}");
    let u = usage(&d);
    assert_eq!(u.len(), 1, "{u:?}");
    assert_eq!(u[0]["ask"], "reject", "{u:?}");
    assert_eq!(u[0]["event"], "add", "{u:?}");
    assert_eq!(u[0]["client"], "cli", "{u:?}");
    let v = stats_json(&d);
    assert_eq!(v["asks"]["reject"]["events"], 1, "{v}");
    assert!(v["asks"].get("stop-block").is_none(), "{v}");
    assert_eq!(v["asks"]["warning"]["events"], 0, "{v}");
}

#[test]
fn reject_missing_files_counts() {
    let d = repo();
    // no hook session, no --files: the old "files is required" reject
    let (ok, _, err) = fael(&d, &["add", "note", "nowhere"], "");
    assert!(!ok && err.contains("rejected: files is required"), "{err}");
    assert_eq!(stats_json(&d)["asks"]["reject"]["events"], 1);
}

#[test]
fn warning_cli_counts_once_per_line() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    // 70 words, no title, --force: exactly the no-title warning, filed anyway
    let text = vec!["word"; 70].join(" ");
    let (ok, _, err) = fael(
        &d,
        &["add", "note", &text, "--files", "src/a.rs", "--force"],
        "",
    );
    assert!(ok, "{err}");
    assert!(err.contains("no title"), "{err}");
    let u = usage(&d);
    assert_eq!(u.len(), 1, "{u:?}");
    assert_eq!(u[0]["ask"], "warning", "{u:?}");
    assert_eq!(stats_json(&d)["asks"]["warning"]["events"], 1);
}

#[test]
fn successful_add_records_nothing() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    // a clean write is no ask: usage stays empty, stats has nothing to split
    let (ok, _, err) = fael(&d, &["add", "note", "quiet row", "--files", "src/a.rs"], "");
    assert!(ok, "{err}");
    assert!(usage(&d).is_empty());
}

#[test]
fn reject_mcp_counts_with_tool_event() {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    // missing text: the MCP `need` reject (now `rejected:`-prefixed like CLI)
    let call = serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": "add", "arguments": {"kind": "note", "cwd": d}}})
    .to_string();
    let mut c = Command::new(env!("CARGO_BIN_EXE_fael"))
        .arg("mcp")
        .env("FAEL_STATE_DIR", d.join("state"))
        .env_remove("FAEL_SESSION")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env_remove("CODEX_THREAD_ID")
        .current_dir(&d)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    c.stdin
        .take()
        .unwrap()
        .write_all((call + "\n").as_bytes())
        .unwrap();
    let out = String::from_utf8(c.wait_with_output().unwrap().stdout).unwrap();
    let v: serde_json::Value = serde_json::from_str(out.lines().next().unwrap()).unwrap();
    let text = v["result"]["content"][0]["text"].as_str().unwrap();
    assert!(
        v["result"]["isError"] == true && text.contains("rejected:"),
        "{out}"
    );
    let u = usage(&d);
    assert_eq!(u.len(), 1, "{u:?}");
    assert_eq!(u[0]["ask"], "reject", "{u:?}");
    assert_eq!(u[0]["event"], "mcp-add", "{u:?}");
    assert_eq!(u[0]["client"], "mcp", "{u:?}");
    // the same call also leaves its friction line, session-less
    let c = super::all_usage(&d)
        .into_iter()
        .find(|v| v["event"] == "call")
        .unwrap();
    assert_eq!(
        (
            c["client"].as_str(),
            c["cmd"].as_str(),
            c["outcome"].as_str(),
            c["reason"].as_str()
        ),
        (Some("mcp"), Some("add"), Some("reject"), Some("bad_value")),
        "{c}"
    );
    assert!(c.get("session").is_none(), "{c}");
}

#[test]
fn phantom_info_line_counts_nothing() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    // the write path says the cited id has no row — an info line, not a
    // warning ask (PLAN-fael-id-refs chunk-2: no `warning:` prefix on purpose)
    let (ok, _, err) = fael(
        &d,
        &[
            "add",
            "note",
            "see 01DEFACED01 for context",
            "--files",
            "src/a.rs",
        ],
        "",
    );
    assert!(ok, "{err}");
    assert!(err.contains("no row with id 01DEFACED01"), "{err}");
    assert!(usage(&d).is_empty());
}

#[test]
fn stats_text_shows_asks_and_constants() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    let _ = fael(&d, &["add", "bogus", "zz", "--files", "src/a.rs"], "");
    let (ok, out, err) = fael(&d, &["stats"], "");
    assert!(ok, "{err}");
    assert!(out.contains("asks: reject ×1"), "{out}");
    assert!(out.contains("constants per session: SKILL.md"), "{out}");
}

#[test]
fn warning_add_carries_row_and_session() {
    use std::process::Command;
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    let text = vec!["word"; 70].join(" ");
    let mut c = Command::new(env!("CARGO_BIN_EXE_fael"));
    c.args([
        "add", "note", &text, "--files", "src/a.rs", "--json", "--force",
    ])
    .current_dir(&d)
    // FAEL_SESSION is set per command by the plugin, so it is always the
    // calling session — a client id no hook ever recorded stamps nothing
    // (01M47N67), it never borrows a stranger's session
    .env("FAEL_SESSION", "sess-add-1")
    .env_remove("CODEX_THREAD_ID")
    .env_remove("CLAUDE_CODE_SESSION_ID");
    super::state_env(&mut c, &d);
    let o = c.output().unwrap();
    assert!(o.status.success(), "{o:?}");
    let row: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    let u = usage(&d);
    assert_eq!(u.len(), 1, "{u:?}");
    assert_eq!(u[0]["row"], row["id"], "{u:?}");
    assert_eq!(u[0]["session"], "sess-add-1", "{u:?}");
    // `ids` means "pushed to the agent" — an add's own row never counts there
    assert!(u[0].get("ids").is_none(), "{u:?}");
}

#[test]
fn reject_unknown_id_carries_its_reason() {
    let d = repo();
    let (ok, _, err) = fael(&d, &["close", "ZZZZZZZZ", "done"], "");
    assert!(!ok && err.contains("rejected: no row with id"), "{err}");
    let u = usage(&d);
    let r: Vec<_> = u.iter().filter(|v| v["ask"] == "reject").collect();
    assert_eq!(r.len(), 1, "{u:?}");
    assert_eq!(r[0]["reason"], "unknown-id", "{u:?}");
}

#[test]
fn silent_session_start_is_counted_apart_from_injections() {
    let d = repo();
    let input = serde_json::json!({ "cwd": d, "session_id": "s1" }).to_string();
    let (ok, out, _) = fael(&d, &["hook", "session-start", "--client", "claude"], &input);
    assert!(ok && out.is_empty(), "{out}");
    let u = usage(&d);
    assert_eq!(u.len(), 1, "{u:?}");
    assert_eq!(
        (&u[0]["event"], &u[0]["bytes"]),
        (&"silent-start".into(), &0.into()),
        "{u:?}"
    );
    assert!(stats_json(&d)["by_event"].get("silent-start").is_none());
}
