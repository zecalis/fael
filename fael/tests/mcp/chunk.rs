//! MCP `chunk`: `show` follows a uid to the repo that holds it, and `done`
//! forwards `--pr`/`--left` — the opencode round-trip (2026-10-22), where a
//! `done` naming both silently dropped them and a `show` needed a retry.

use super::{main_and_worktree, mcp_tool};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

fn cli(dir: &Path, args: &[&str]) -> (bool, String, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_fael"))
        .args(args)
        .current_dir(dir)
        .env("FAEL_STATE_DIR", dir.join("state"))
        .output()
        .unwrap();
    let s = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    (o.status.success(), s(&o.stdout), s(&o.stderr))
}

/// A second, independent repo: its own journal, not this clone's.
fn other_repo() -> PathBuf {
    let d = std::env::temp_dir().join(format!("fael-mcp-chunk-{}", fael_core::ulid()));
    std::fs::create_dir_all(d.join("src")).unwrap();
    for args in [
        &["init", "-q", "-b", "main"][..],
        &["config", "user.name", "Chunk Test"],
        &["config", "user.email", "chunk@example.com"],
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
    std::fs::create_dir_all(d.join(".fael")).unwrap();
    d.canonicalize().unwrap()
}

/// Name `other` in this server's usage, as any call there would.
fn name_repo(state: &Path, other: &Path) {
    std::fs::create_dir_all(state).unwrap();
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(state.join("usage.jsonl"))
        .unwrap();
    writeln!(f, "{}", serde_json::json!({"event": "call", "repo": other})).unwrap();
}

fn text_of(r: &[serde_json::Value]) -> String {
    r[0]["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .to_string()
}

#[test]
fn chunk_show_follows_a_uid_in_another_repo() {
    let (main, wt) = main_and_worktree();
    let other = other_repo();
    let (ok, uid, err) = cli(&other, &["chunk", "add", "Post", "--brief", "Write it"]);
    assert!(ok, "{err}");
    let uid = uid.trim().to_string();
    name_repo(&main.parent().unwrap().join("state"), &other);
    let r = mcp_tool(
        &wt,
        "chunk",
        &[serde_json::json!({"verb": "show", "uid": uid})],
    );
    assert_eq!(r[0]["result"]["isError"], false, "{r:?}");
    let body = text_of(&r);
    assert!(body.contains("Post") && body.contains("Write it"), "{body}");
    assert!(body.contains(&other.display().to_string()), "{body}");
}

#[test]
fn chunk_done_forwards_pr_and_left() {
    let (_, wt) = main_and_worktree();
    let (ok, uid, err) = cli(&wt, &["chunk", "add", "Post", "--brief", "Write it"]);
    assert!(ok, "{err}");
    let uid = uid.trim().to_string();
    let done = |args: serde_json::Value| {
        let r = mcp_tool(&wt, "chunk", &[args]);
        (r[0]["result"]["isError"] == true, text_of(&r))
    };
    // a leftover no row owns: the CLI's `--left` reject, not a silent drop —
    // without forwarding this read "needs --left none or ..."
    let (is_err, out) = done(
        serde_json::json!({"verb": "done", "uid": uid, "text": "did it", "left": "01NOSUCHROW"}),
    );
    assert!(
        is_err && out.contains("--left 01NOSUCHROW is no open ledger row"),
        "{out}"
    );
    // a bad PR number: the CLI's `--pr` reject, so `pr` rides through too
    let (is_err, out) = done(
        serde_json::json!({"verb": "done", "uid": uid, "text": "did it", "left": "none", "pr": "xx"}),
    );
    assert!(is_err && out.contains("--pr takes a PR number"), "{out}");
    // the array form rides comma-joined, like the CLI spells it
    let (is_err, out) = done(
        serde_json::json!({"verb": "done", "uid": uid, "text": "did it", "left": ["01NOSUCHROW"]}),
    );
    assert!(
        is_err && out.contains("--left 01NOSUCHROW is no open ledger row"),
        "{out}"
    );
    // `review` rides through too: the CLI's own reject, not a silent drop
    let (is_err, out) = done(
        serde_json::json!({"verb": "done", "uid": uid, "text": "did it", "left": "none", "review": "bogus"}),
    );
    assert!(is_err && out.contains("--review"), "{out}");
}

/// A client that declares `roots` and names a worktree other than the server's launch dir
/// gives `chunk start` its seat with no `cwd` (issue 01M6BKHZP); the server asks for roots
/// after `initialized`.
#[test]
fn start_takes_its_seat_from_a_client_root_that_is_not_the_launch_dir() {
    use std::process::Stdio;
    let (main, wt) = main_and_worktree();
    let (ok, uid, err) = cli(&main, &["chunk", "add", "t", "--brief", "b"]);
    assert!(ok, "{err}");
    let uid = uid.trim().to_string();
    let lines = [
        serde_json::json!({"jsonrpc": "2.0", "id": 0, "method": "initialize", "params": {"protocolVersion": "2025-06-18", "capabilities": {"roots": {}}}}),
        serde_json::json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        serde_json::json!({"jsonrpc": "2.0", "id": "fael-roots", "result": {"roots": [{"uri": format!("file://{}", wt.display())}]}}),
        serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "chunk", "arguments": {"verb": "start", "uid": uid}}}),
    ];
    let mut c = Command::new(env!("CARGO_BIN_EXE_fael"))
        .arg("mcp")
        .env("FAEL_STATE_DIR", main.join("state"))
        .current_dir(&main)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = c.stdin.take().unwrap();
    let body: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
    stdin
        .write_all((body.join("\n") + "\n").as_bytes())
        .unwrap();
    drop(stdin);
    let out = String::from_utf8(c.wait_with_output().unwrap().stdout).unwrap();
    assert!(out.contains(r#""method":"roots/list""#), "{out}");
    assert!(out.contains(r#""isError":false"#), "{out}");
    let (_, show, _) = cli(&main, &["chunk", "show", &uid, "--json"]);
    let c: serde_json::Value = serde_json::from_str(&show).unwrap();
    assert_eq!(c["run"]["worktree"], wt.to_str().unwrap(), "{show}");
}
