//! `fael mcp` picks the repo per call — the server runs in the session's cwd,
//! an agent may be working in another worktree.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn git(d: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .args(args)
        .current_dir(d)
        .status()
        .unwrap()
        .success();
    assert!(ok, "git {args:?}");
}

/// `main/` with a commit, and `wt/` checked out from it as a git worktree.
fn main_and_worktree() -> (PathBuf, PathBuf) {
    let base = std::env::temp_dir().join(format!("fael-mcp-{}", fael_core::ulid()));
    let (main, wt) = (base.join("main"), base.join("wt"));
    std::fs::create_dir_all(main.join("src")).unwrap();
    git(&main, &["init", "-q"]);
    git(&main, &["config", "user.name", "Mcp Test"]);
    git(&main, &["config", "user.email", "mcp@example.com"]);
    std::fs::write(main.join("src/a.rs"), "// a\n").unwrap();
    git(&main, &["add", "."]);
    git(&main, &["commit", "-qm", "init"]);
    git(&main, &["worktree", "add", "-q", wt.to_str().unwrap()]);
    (main.canonicalize().unwrap(), wt.canonicalize().unwrap())
}

fn mcp(dir: &Path, calls: &[serde_json::Value]) -> Vec<serde_json::Value> {
    let mut c = Command::new(env!("CARGO_BIN_EXE_fael"))
        .arg("mcp")
        .env("FAEL_STATE_DIR", dir.join("../state"))
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .current_dir(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let lines: Vec<String> = calls
        .iter()
        .enumerate()
        .map(|(i, a)| {
            serde_json::json!({"jsonrpc": "2.0", "id": i, "method": "tools/call",
                "params": {"name": "add", "arguments": a}})
            .to_string()
        })
        .collect();
    let mut stdin = c.stdin.take().unwrap();
    stdin
        .write_all((lines.join("\n") + "\n").as_bytes())
        .unwrap();
    drop(stdin);
    let out = String::from_utf8(c.wait_with_output().unwrap().stdout).unwrap();
    out.lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

/// Every row text in `<root>/.fael/log`.
fn texts(root: &Path) -> String {
    let mut all = String::new();
    let log = root.join(".fael/log");
    for dir in std::fs::read_dir(&log).into_iter().flatten().flatten() {
        for f in std::fs::read_dir(dir.path())
            .into_iter()
            .flatten()
            .flatten()
        {
            all += &std::fs::read_to_string(f.path()).unwrap_or_default();
        }
    }
    all
}

#[test]
fn add_lands_in_the_worktree_the_call_names() {
    let (main, wt) = main_and_worktree();
    let r = mcp(
        &main,
        &[
            serde_json::json!({"kind": "note", "text": "by cwd", "files": ["src/a.rs"], "cwd": wt}),
            serde_json::json!({"kind": "note", "text": "by abs path", "files": [wt.join("src/a.rs")]}),
            serde_json::json!({"kind": "note", "text": "plain", "files": ["src/a.rs"]}),
        ],
    );
    assert!(r.iter().all(|v| v["result"]["isError"] == false), "{r:?}");
    let (m, w) = (texts(&main), texts(&wt));
    assert!(w.contains("by cwd") && w.contains("by abs path"), "{w}");
    assert!(!m.contains("by cwd") && !m.contains("by abs path"), "{m}");
    assert!(m.contains("plain") && !w.contains("plain"), "{m}");
    // the absolute path is stored repo-relative, like any other
    assert!(w.contains(r#""src/a.rs""#), "{w}");
}

/// Chunk 6b over MCP: `rows: [...]` with one bad row — it reports alone
/// (`rejected: row 1:`), the rest save, the call is an error.
#[test]
fn add_rows_batch_partial() {
    let (_, wt) = main_and_worktree();
    let r = mcp(
        &wt,
        &[serde_json::json!({"rows": [
            {"kind": "note", "text": "first mcp batch row", "files": ["src/a.rs"]},
            {"kind": "nope", "text": "bad kind row", "files": ["src/a.rs"]},
            {"kind": "issue", "text": "third mcp batch row", "files": ["src/a.rs"]},
        ]})],
    );
    assert_eq!(r.len(), 1);
    let body = r[0]["result"]["content"][0]["text"].as_str().unwrap();
    assert!(r[0]["result"]["isError"] == true, "{body}");
    assert!(body.contains("recorded"), "{body}");
    assert!(body.contains("rejected: row 1:"), "{body}");
    let log = texts(&wt);
    assert!(
        log.contains("first mcp batch row") && log.contains("third mcp batch row"),
        "{log}"
    );
    assert!(!log.contains("bad kind row"), "{log}");
}
