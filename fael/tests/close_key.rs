//! PLAN-fael-close-helpers chunk 2: `fael close --key <key> "<why>"` (and MCP
//! `close` with `key`) closes the one open row on a key — and never picks
//! between several.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn fael(dir: &Path, args: &[&str]) -> (bool, String, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_fael"))
        .args(args)
        .current_dir(dir)
        .env("FAEL_STATE_DIR", dir.join("../state"))
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .output()
        .unwrap();
    let s = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    (o.status.success(), s(&o.stdout), s(&o.stderr))
}

/// The same `close` call through the MCP server: (is_error, text).
fn mcp_close(dir: &Path, args: serde_json::Value) -> (bool, String) {
    let mut c = Command::new(env!("CARGO_BIN_EXE_fael"))
        .arg("mcp")
        .env("FAEL_STATE_DIR", dir.join("../state"))
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .current_dir(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let call = serde_json::json!({"jsonrpc": "2.0", "id": 0, "method": "tools/call",
        "params": {"name": "close", "arguments": args}});
    let mut stdin = c.stdin.take().unwrap();
    stdin.write_all(format!("{call}\n").as_bytes()).unwrap();
    drop(stdin);
    let out = String::from_utf8(c.wait_with_output().unwrap().stdout).unwrap();
    let v: serde_json::Value = serde_json::from_str(out.lines().next().unwrap()).unwrap();
    (
        v["result"]["isError"] == true,
        v["result"]["content"][0]["text"].as_str().unwrap().into(),
    )
}

fn repo() -> PathBuf {
    let base = std::env::temp_dir().join(format!("fael-close-key-{}", fael_core::ulid()));
    let d = base.join("r");
    std::fs::create_dir_all(d.join("src")).unwrap();
    for args in [
        &["init", "-q"][..],
        &["config", "user.name", "Test User"],
        &["config", "user.email", "t@example.com"],
    ] {
        assert!(
            Command::new("git")
                .args(args)
                .current_dir(&d)
                .status()
                .unwrap()
                .success()
        );
    }
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    std::fs::create_dir_all(d.join(".fael")).unwrap();
    std::fs::write(d.join(".fael/config.toml"), "store = \"tracked\"\n").unwrap();
    d
}

/// File a row on `src/a.rs` under `key`; its full id.
fn add(d: &Path, kind: &str, text: &str, key: &str) -> String {
    let (ok, out, err) = fael(d, &["add", kind, text, "--key", key, "--files", "src/a.rs"]);
    assert!(ok, "{err}");
    out.split_whitespace().next().unwrap().to_string()
}

fn open_titles(d: &Path, key: &str) -> String {
    fael(d, &["find", "--key", key]).1
}

#[test]
fn one_open_row_on_the_key_closes() {
    let d = repo();
    add(&d, "note", "plan handoff text", "plan:x:handoff");
    let (ok, out, err) = fael(&d, &["close", "--key", "plan:x:handoff", "chunk shipped"]);
    assert!(ok, "{err}");
    assert!(out.contains("→"), "{out}");
    assert!(!open_titles(&d, "plan:x:handoff").contains("plan handoff text"));
}

#[test]
fn no_open_row_is_rejected() {
    let d = repo();
    let (ok, _, err) = fael(&d, &["close", "--key", "nope", "why"]);
    assert!(!ok && err.contains("no open row on nope"), "{err}");
    // a closed row no longer counts as open
    add(&d, "note", "once", "k");
    assert!(fael(&d, &["close", "--key", "k", "done"]).0);
    let (ok, _, err) = fael(&d, &["close", "--key", "k", "again"]);
    assert!(!ok && err.contains("no open row on k"), "{err}");
}

#[test]
fn several_open_rows_list_each_and_close_nothing() {
    let d = repo();
    let (a, b) = (
        add(&d, "issue", "first broken", "k"),
        add(&d, "issue", "second broken", "k"),
    );
    let (ok, out, err) = fael(&d, &["close", "--key", "k", "fixed"]);
    let all = format!("{out}{err}");
    assert!(!ok, "{all}");
    for (id, title) in [(&a, "first broken"), (&b, "second broken")] {
        let line = all.lines().find(|l| l.contains(title)).expect(&all);
        assert!(
            line.contains("issue") && all.contains("fael close "),
            "{all}"
        );
        // the id the list shows is a prefix of the real one
        let shown = line.split(['[', ']']).nth(1).unwrap();
        assert!(id.starts_with(shown), "{line}");
    }
    let still = open_titles(&d, "k");
    assert!(
        still.contains("first broken") && still.contains("second broken"),
        "{still}"
    );
}

#[test]
fn key_and_id_together_are_rejected() {
    let d = repo();
    let id = add(&d, "note", "n", "k");
    let (ok, _, err) = fael(&d, &["close", "--key", "k", &id, "why"]);
    assert!(!ok && err.contains("not both"), "{err}");
    assert!(open_titles(&d, "k").contains('n'));
}

#[test]
fn mcp_close_with_key_behaves_the_same() {
    let d = repo();
    add(&d, "note", "handoff body", "plan:y:handoff");
    let (err, text) = mcp_close(
        &d,
        serde_json::json!({"key": "plan:y:handoff", "text": "done"}),
    );
    assert!(!err && text.contains("recorded"), "{text}");
    let (err, text) = mcp_close(
        &d,
        serde_json::json!({"key": "plan:y:handoff", "text": "again"}),
    );
    assert!(
        err && text.contains("no open row on plan:y:handoff"),
        "{text}"
    );

    add(&d, "issue", "one", "k");
    add(&d, "issue", "two", "k");
    let (err, text) = mcp_close(&d, serde_json::json!({"key": "k", "text": "fixed"}));
    assert!(
        err && text.contains("one") && text.contains("two"),
        "{text}"
    );

    let (err, text) = mcp_close(&d, serde_json::json!({"key": "k", "id": "x", "text": "w"}));
    assert!(err && text.contains("not both"), "{text}");
}
