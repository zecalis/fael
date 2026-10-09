//! `fael mcp --pin` — a server exposed over HTTP acts on its own cwd only:
//! no `cwd` in the schema, a `cwd` arg is rejected, absolute `files` never
//! reroute to another repo on the host.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn workspace(base: &Path, name: &str) -> PathBuf {
    let d = base.join(name);
    std::fs::create_dir_all(d.join(".fael")).unwrap();
    std::fs::create_dir_all(d.join(".git")).unwrap();
    d.canonicalize().unwrap()
}

fn serve(dir: &Path, reqs: &[serde_json::Value]) -> Vec<serde_json::Value> {
    let mut c = Command::new(env!("CARGO_BIN_EXE_fael"))
        .args(["mcp", "--pin"])
        .env("FAEL_STATE_DIR", dir.join("../state"))
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .current_dir(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = c.stdin.take().unwrap();
    for (i, r) in reqs.iter().enumerate() {
        let mut r = r.clone();
        r["jsonrpc"] = "2.0".into();
        r["id"] = i.into();
        writeln!(stdin, "{r}").unwrap();
    }
    drop(stdin);
    let out = String::from_utf8(c.wait_with_output().unwrap().stdout).unwrap();
    out.lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

fn find(args: serde_json::Value) -> serde_json::Value {
    serde_json::json!({"method": "tools/call", "params": {"name": "find", "arguments": args}})
}

#[test]
fn pinned_server_never_leaves_its_workspace() {
    let base = std::env::temp_dir().join(format!("fael-mcp-pin-{}", fael_core::ulid()));
    let (mine, other) = (workspace(&base, "mine"), workspace(&base, "other"));
    let ok = Command::new(env!("CARGO_BIN_EXE_fael"))
        .env("FAEL_STATE_DIR", base.join("state"))
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .args(["add", "note", "secret of other", "--files", "doc:x"])
        .current_dir(&other)
        .status()
        .unwrap()
        .success();
    assert!(ok);

    let out = serve(
        &mine,
        &[
            serde_json::json!({"method": "tools/list"}),
            find(serde_json::json!({"cwd": other})),
            find(serde_json::json!({"files": [other.join("doc.md")]})),
        ],
    );
    let tools = out[0]["result"]["tools"].to_string();
    assert!(!tools.contains("\"cwd\""), "{tools}");
    assert_eq!(out[1]["result"]["isError"], true, "{}", out[1]);
    assert!(out[1].to_string().contains("pinned"), "{}", out[1]);
    assert!(
        !out[2].to_string().contains("secret of other"),
        "{}",
        out[2]
    );
}
