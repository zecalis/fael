//! PLAN-fael-agent-ergonomics chunk 3 (experiment): `tools/list` shows the
//! core only, but hidden properties still run when called.

use super::{main_and_worktree, texts};
use std::io::Write;
use std::process::{Command, Stdio};

#[test]
fn core_schema_hides_but_hidden_properties_still_run() {
    let (_, wt) = main_and_worktree();
    let raw = |lines: &[String]| -> Vec<serde_json::Value> {
        let mut c = Command::new(env!("CARGO_BIN_EXE_fael"))
            .arg("mcp")
            .env("FAEL_STATE_DIR", wt.join("../state"))
            .env_remove("CLAUDE_CODE_SESSION_ID")
            .current_dir(&wt)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        c.stdin
            .take()
            .unwrap()
            .write_all((lines.join("\n") + "\n").as_bytes())
            .unwrap();
        String::from_utf8(c.wait_with_output().unwrap().stdout)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    };
    let req = |i: usize, method: &str, params: serde_json::Value| {
        serde_json::json!({"jsonrpc": "2.0", "id": i, "method": method, "params": params})
            .to_string()
    };
    let out = raw(&[
        req(0, "tools/list", serde_json::json!({})),
        req(
            1,
            "tools/call",
            serde_json::json!({"name": "find", "arguments": {"groups": true}}),
        ),
        req(
            2,
            "tools/call",
            serde_json::json!({"name": "find", "arguments": {"since": "2026-01"}}),
        ),
        req(
            3,
            "tools/call",
            serde_json::json!({"name": "add", "arguments": {
                "kind": "note", "text": "peek", "files": ["src/a.rs"], "dry_run": true,
            }}),
        ),
    ]);
    // the served surface is the core
    let listed = out[0]["result"]["tools"].to_string();
    for hidden in [
        "\"since\"",
        "\"branches\"",
        "\"groups\"",
        "\"urgent_before\"",
        "\"dry_run\"",
        "\"force\"",
    ] {
        assert!(!listed.contains(hidden), "{hidden} leaks: {listed}");
    }
    for core in ["\"limit\"", "\"offset\"", "\"text\"", "\"supersedes\""] {
        assert!(listed.contains(core), "{core} missing: {listed}");
    }
    // ... but every hidden property still answers
    for (i, want_err) in [(1, false), (2, false), (3, false)] {
        assert_eq!(out[i]["result"]["isError"], want_err, "{}", out[i]);
    }
    assert!(!texts(&wt).contains("peek"), "dry_run wrote a row");
}
