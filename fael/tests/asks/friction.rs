//! PLAN-fael-agent-ergonomics chunk 1: every agent call leaves a `call` line,
//! and `fael stats` turns them into friction and first-call success.

use super::{all_usage, repo, stats_json};
use std::path::Path;
use std::process::Command;

/// `fael` inside an agent session (`FAEL_SESSION`), or outside one with `None`.
fn call(d: &Path, session: Option<&str>, args: &[&str]) -> (bool, String) {
    call_in(d, session, args, "")
}

fn call_in(d: &Path, session: Option<&str>, args: &[&str], stdin: &str) -> (bool, String) {
    use std::io::Write;
    use std::process::Stdio;
    let root = d.ancestors().find(|p| p.join(".git").exists()).unwrap();
    let mut c = Command::new(env!("CARGO_BIN_EXE_fael"));
    c.args(args)
        .current_dir(d)
        .env("FAEL_STATE_DIR", root.join("state"))
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env_remove("CODEX_THREAD_ID")
        .env_remove("FAEL_SESSION");
    if let Some(s) = session {
        c.env("FAEL_SESSION", s);
    }
    let mut child = c
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    let o = child.wait_with_output().unwrap();
    (
        o.status.success(),
        String::from_utf8_lossy(&o.stderr).into_owned(),
    )
}

fn calls(d: &Path) -> Vec<serde_json::Value> {
    let mut u = all_usage(d);
    u.retain(|v| v["event"] == "call");
    u
}

#[test]
fn one_reject_counts_one_and_a_clean_call_is_a_first_call_success() {
    let d = repo();
    let (ok, err) = call(&d, Some("s1"), &["find", "--stale"]);
    assert!(!ok && err.contains("unknown flag --stale"), "{err}");
    let c = calls(&d);
    assert_eq!(c.len(), 1, "{c:?}");
    assert_eq!(
        (
            c[0]["cmd"].as_str(),
            c[0]["outcome"].as_str(),
            c[0]["reason"].as_str()
        ),
        (Some("find"), Some("reject"), Some("unknown_flag"))
    );
    let f = &stats_json(&d)["friction"];
    assert_eq!(
        (f["calls"].as_u64(), f["rejects"].as_u64()),
        (Some(1), Some(1))
    );
    assert_eq!(f["reasons"]["unknown_flag"], 1, "{f}");
    assert_eq!(f["by_command"]["find"]["rejects"], 1, "{f}");

    // a lone success in another session counts as a first-call success
    let d = repo();
    let (ok, err) = call(&d, Some("s1"), &["find", "--files", "src/a.rs"]);
    assert!(ok, "{err}");
    let f = &stats_json(&d)["friction"];
    assert_eq!(
        (f["calls"].as_u64(), f["first_call_ok"].as_u64()),
        (Some(1), Some(1)),
        "{f}"
    );
}

#[test]
fn an_empty_find_then_another_find_is_one_repeat() {
    let d = repo();
    call(&d, Some("s1"), &["find", "nothing-matches-this"]);
    call(&d, Some("s1"), &["find", "nor-this"]);
    let c = calls(&d);
    assert_eq!(c[0]["empty"], true, "{c:?}");
    let f = &stats_json(&d)["friction"];
    assert_eq!(
        (f["find_repeat"].as_u64(), f["first_call_ok"].as_u64()),
        (Some(1), Some(0)),
        "{f}"
    );
}

#[test]
fn help_counts_and_a_call_outside_a_session_does_not() {
    let d = repo();
    call(&d, Some("s1"), &["find", "--help"]);
    let f = &stats_json(&d)["friction"];
    assert_eq!(
        (f["calls"].as_u64(), f["help"].as_u64()),
        (Some(1), Some(1)),
        "{f}"
    );
    let d = repo();
    call(&d, None, &["find", "--stale"]);
    call(&d, None, &["find", "--help"]);
    assert!(
        calls(&d).is_empty(),
        "a human at the keyboard is no agent friction"
    );
}

#[test]
fn hook_and_stats_calls_leave_no_line() {
    let d = repo();
    call(&d, Some("s1"), &["stats"]);
    call(&d, Some("s1"), &["doctor"]);
    assert!(calls(&d).is_empty());
}

#[test]
fn the_same_call_in_two_processes_has_the_same_sig() {
    // an identical empty find again is a re-poll, not a fix — which only holds
    // if the argument hash survives the process (no per-process random seed)
    let d = repo();
    call(&d, Some("s1"), &["find", "nothing-matches-this"]);
    call(&d, Some("s1"), &["find", "nothing-matches-this"]);
    let c = calls(&d);
    assert_eq!(c[0]["sig"], c[1]["sig"], "{c:?}");
    assert_eq!(stats_json(&d)["friction"]["find_repeat"], 0);
    call(&d, Some("s1"), &["find", "nothing-matches-that"]);
    assert_ne!(calls(&d)[2]["sig"], c[0]["sig"]);
    assert_eq!(stats_json(&d)["friction"]["find_repeat"], 1);
}

#[test]
fn a_hook_leaves_no_call_line_but_a_find_in_the_same_session_does() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    let input = serde_json::json!({"cwd": d, "session_id": "s1",
        "tool_input": {"file_path": d.join("src/a.rs")}})
    .to_string();
    for ev in ["read", "edit"] {
        let (ok, err) = call_in(&d, Some("s1"), &["hook", ev, "--client", "claude"], &input);
        assert!(ok, "{err}");
    }
    assert!(calls(&d).is_empty(), "{:?}", calls(&d));
    call(&d, Some("s1"), &["find", "--files", "src/a.rs"]);
    assert_eq!(calls(&d).len(), 1);
}

/// One `fael mcp` process fed these tool calls, session-less like a real server's env.
fn mcp(d: &Path, calls: &[(&str, serde_json::Value)]) {
    use std::io::Write;
    use std::process::Stdio;
    let mut c = Command::new(env!("CARGO_BIN_EXE_fael"))
        .arg("mcp")
        .env("FAEL_STATE_DIR", d.join("state"))
        .env_remove("FAEL_SESSION")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env_remove("CODEX_THREAD_ID")
        .current_dir(d)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = c.stdin.take().unwrap();
    for (i, (name, args)) in calls.iter().enumerate() {
        let rpc = serde_json::json!({"jsonrpc": "2.0", "id": i, "method": "tools/call",
            "params": {"name": name, "arguments": args}});
        writeln!(stdin, "{rpc}").unwrap();
    }
    drop(stdin);
    let out = c.wait_with_output().unwrap();
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).lines().count(),
        calls.len()
    );
}

#[test]
fn mcp_calls_leave_a_line_each_unless_fael_itself_failed() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    mcp(
        &d,
        &[
            (
                "find",
                serde_json::json!({"text": "nothing-matches-this", "cwd": d}),
            ),
            (
                "add",
                serde_json::json!({"kind": "note", "text": "x", "files": ["src/a.rs"], "cwd": d}),
            ),
            ("nope", serde_json::json!({"cwd": d})),
            // fael's own failure (no such cwd), not a `rejected:` — no line
            ("find", serde_json::json!({"cwd": "/definitely/not/here"})),
            (
                "add",
                serde_json::json!({"kind": "note", "text": "y", "files": ["src/a.rs"], "cwd": d}),
            ),
        ],
    );
    let c = calls(&d);
    let got: Vec<_> = c
        .iter()
        .map(|v| {
            (
                v["cmd"].as_str(),
                v["outcome"].as_str(),
                v["empty"].as_bool(),
                v["reason"].as_str(),
            )
        })
        .collect();
    assert_eq!(
        got,
        [
            (Some("find"), Some("ok"), Some(true), None),
            (Some("add"), Some("ok"), None, None),
            (Some("?"), Some("reject"), None, Some("unknown_command")),
            (Some("add"), Some("ok"), None, None),
        ],
        "an empty find's flag must not leak into later calls: {c:?}"
    );
    assert!(
        c.iter()
            .all(|v| v["client"] == "mcp" && v.get("session").is_none())
    );
}

#[test]
fn reasons_come_from_the_real_reject_paths() {
    let d = repo();
    for args in [
        &["find", "--stale"][..],
        &["find", "-k", "x"],
        &["bump", "01ABC", "--title", "x"],
        &["add", "note", "x", "--files"],
        &["add"],
        &["find", "a b", "c"],
        &["frobnicate"],
    ] {
        let (ok, err) = call(&d, Some("s1"), args);
        assert!(!ok && err.contains("rejected:"), "{args:?}: {err}");
    }
    let f = &stats_json(&d)["friction"];
    assert_eq!(
        f["reasons"],
        serde_json::json!({
            // `-k` is a flag now, not an id
            "unknown_flag": 2, "bad_id": 1, "flag_not_taken": 1, "bad_value": 2, "unknown_command": 1
        }),
        "{f}"
    );
    // the bare unknown command has no command to blame
    assert_eq!(f["by_command"]["?"]["rejects"], 1, "{f}");
    assert_eq!(f["by_command"]["find"]["rejects"], 3, "{f}");
    assert_eq!(f["shape_gate"], 0, "{f}");
}

#[test]
fn a_shape_gate_reject_is_counted_apart_not_as_friction() {
    let d = repo();
    let long = "word ".repeat(70);
    let (ok, err) = call(&d, Some("s1"), &["add", "note", &long, "--files", "a.rs"]);
    assert!(!ok && err.starts_with("rejected: nothing written"), "{err}");
    let f = &stats_json(&d)["friction"];
    assert_eq!(f["shape_gate"], 1, "{f}");
    assert_eq!((&f["calls"], &f["rejects"]), (&0.into(), &0.into()), "{f}");
    assert_eq!(f["reasons"], serde_json::json!({}), "{f}");
}

/// A batch whose every rejected row met the gate is the gate too, CLI and MCP;
/// one row rejected for another reason makes the call a `bad_value` (01M4GD6C).
#[test]
fn a_batch_rejected_only_by_the_gate_is_counted_as_the_gate() {
    let d = repo();
    let long = "word ".repeat(70);
    let gated = serde_json::json!({"kind": "note", "text": long, "files": ["a.rs"]});
    let ok = serde_json::json!({"kind": "note", "text": "short", "files": ["a.rs"]});
    let bad = serde_json::json!({"kind": "nope", "text": "short", "files": ["a.rs"]});
    let batch = serde_json::json!([gated, ok]).to_string();
    let (saved, err) = call_in(&d, Some("s1"), &["add", "--json", "-"], &batch);
    let last = err.lines().last().unwrap_or_default();
    assert!(
        !saved && last.starts_with("rejected: nothing written"),
        "{err}"
    );
    mcp(
        &d,
        &[
            ("add", serde_json::json!({"rows": [gated, ok], "cwd": d})),
            ("add", serde_json::json!({"rows": [gated, bad], "cwd": d})),
        ],
    );
    let f = &stats_json(&d)["friction"];
    assert_eq!(f["shape_gate"], 2, "{f}");
    assert_eq!(f["reasons"], serde_json::json!({"bad_value": 1}), "{f}");
}

#[test]
fn since_cuts_call_lines_like_every_other_line() {
    let d = repo();
    let line = |ts: &str, outcome: &str| {
        format!(
            "{{\"ts\":\"{ts}\",\"repo\":{},\"client\":\"cli\",\"event\":\"call\",\"session\":\"s\",\"cmd\":\"find\",\"outcome\":\"{outcome}\",\"reason\":\"bad_value\",\"sig\":\"x\"}}\n",
            serde_json::json!(d)
        )
    };
    let state = d.join("state");
    std::fs::create_dir_all(&state).unwrap();
    std::fs::write(
        state.join("usage.jsonl"),
        line("2026-10-01T00:00:00.000Z", "reject") + &line("2026-10-06T00:00:00.000Z", "ok"),
    )
    .unwrap();
    let stats = |extra: &[&str]| {
        let mut args = vec!["stats", "--json"];
        args.extend(extra);
        let (ok, out, err) = super::fael(&d, &args, "");
        assert!(ok, "{err}");
        serde_json::from_str::<serde_json::Value>(&out).unwrap()["friction"].clone()
    };
    let all = stats(&[]);
    assert_eq!(
        (all["calls"].as_u64(), all["rejects"].as_u64()),
        (Some(2), Some(1)),
        "{all}"
    );
    let since = stats(&["--since", "2026-10-05"]);
    assert_eq!(
        (since["calls"].as_u64(), since["rejects"].as_u64()),
        (Some(1), Some(0)),
        "{since}"
    );
}

/// A synonym is counted as the command it stands for — `decision` is an `add`,
/// `show` a `find` — not as a `?` that no command's numbers would hold.
#[test]
fn a_synonym_counts_under_the_real_command() {
    let d = repo();
    let (ok, err) = call(&d, Some("s1"), &["decision", "x", "--file", "src/a.rs"]);
    assert!(ok, "{err}");
    let (ok, err) = call(&d, Some("s1"), &["show", "--stale"]);
    assert!(!ok && err.contains("unknown flag --stale"), "{err}");
    let got: Vec<_> = calls(&d)
        .iter()
        .map(|v| {
            (
                v["cmd"].as_str().map(String::from),
                v["outcome"].as_str().map(String::from),
            )
        })
        .collect();
    assert_eq!(
        got,
        [
            (Some("add".into()), Some("ok".into())),
            (Some("find".into()), Some("reject".into())),
        ]
    );
}
