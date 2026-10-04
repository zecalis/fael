//! Chunk 3b–e (PLAN-fael-durable-log): a repeated note on the same writer +
//! branch + files supersedes the open one itself, a caller-supplied key
//! supersedes the single open row with the same kind + key, the text sets or
//! rescues `--supersedes`, and the one key these files already carry is
//! reused — several matches file the row and list what was kept, never
//! asking. Thin entry only — suites sit next to this file.

mod autokey;
mod corpus;
mod key;
mod note;
mod policy;
mod text;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Per-child `FAEL_STATE_DIR` at `<repo root>/state`, so a real session on
/// this machine never leaks in and tests run in parallel.
fn fael(dir: &Path, args: &[&str], stdin: &str) -> (bool, String, String) {
    let root = dir.ancestors().find(|p| p.join(".git").exists()).unwrap();
    let mut c = Command::new(env!("CARGO_BIN_EXE_fael"));
    c.args(args)
        .current_dir(dir)
        .env("FAEL_STATE_DIR", root.join("state"))
        .env_remove("CLAUDE_CODE_SESSION_ID");
    if !stdin.is_empty() {
        c.stdin(Stdio::piped());
    }
    let mut c = c
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    if !stdin.is_empty() {
        c.stdin.take().unwrap().write_all(stdin.as_bytes()).unwrap();
    }
    let o = c.wait_with_output().unwrap();
    let s = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    (o.status.success(), s(&o.stdout), s(&o.stderr))
}

fn repo() -> PathBuf {
    let d = std::env::temp_dir().join(format!("fael-heal-{}", fael_core::ulid()));
    std::fs::create_dir_all(d.join("src")).unwrap();
    for args in [
        &["init", "-q"][..],
        &["config", "user.name", "Heal Test"],
        &["config", "user.email", "heal@example.com"],
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
    Command::new("git")
        .args(["commit", "-q", "--allow-empty", "-m", "init"])
        .current_dir(&d)
        .status()
        .unwrap();
    for f in ["src/a.rs", "src/b.rs"] {
        std::fs::write(d.join(f), format!("// {f}\n")).unwrap();
    }
    d
}

/// Did `err` name `id` right after `lead` — as the abbreviated, still-unique
/// prefix render prints (≥ 8 chars), not the full id or a fixed `[..8]`?
fn names(err: &str, lead: &str, id: &str) -> bool {
    err.match_indices(lead).any(|(i, _)| {
        let tok = err[i + lead.len()..]
            .split(|c: char| c.is_whitespace() || c == ',')
            .next()
            .unwrap_or("");
        tok.len() >= 8 && tok.len() < id.len() && id.starts_with(tok)
    })
}

/// Full ids of the currently listed open notes.
fn open_notes(d: &Path) -> Vec<String> {
    let (ok, out, err) = fael(d, &["find", "--kind", "note", "--json"], "");
    assert!(ok, "{err}");
    out.lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter_map(|v| v["id"].as_str().map(String::from))
        .collect()
}

/// Usage rows recorded so far in this repo's scratch state dir.
fn usage(d: &Path) -> Vec<serde_json::Value> {
    let root = d.ancestors().find(|p| p.join(".git").exists()).unwrap();
    std::fs::read_to_string(root.join("state/usage.jsonl"))
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        // a find's outcome line (`found`) is no ask
        .filter(|v| v.get("found").is_none())
        .collect()
}

/// One `add` over MCP; returns (isError, text). Every suite runs the same
/// self-heal through here too — CLI and MCP share `write::add_row`, and this
/// is what proves it.
fn mcp_add(d: &Path, args: serde_json::Value) -> (bool, String) {
    let root = d.ancestors().find(|p| p.join(".git").exists()).unwrap();
    let call = serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
        "params": {"name": "add", "arguments": args}})
    .to_string();
    let mut c = Command::new(env!("CARGO_BIN_EXE_fael"))
        .arg("mcp")
        .env("FAEL_STATE_DIR", root.join("state"))
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .current_dir(d)
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
    (
        v["result"]["isError"] == true,
        v["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
    )
}
