//! MCP `add` (single and `rows`) and `bump` say which files got no `fh` key —
//! one info line after the id, never a reject. Split from mcp.rs at the
//! 400-line ratchet.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const NOTE: &str = "fael: not stamped (no file-hash verdict at push): ";

fn repo() -> PathBuf {
    let d = std::env::temp_dir().join(format!("fael-mcp-unstamped-{}", fael_core::ulid()));
    std::fs::create_dir_all(d.join("src")).unwrap();
    for args in [
        &["init", "-q"][..],
        &["config", "user.name", "Mcp Test"],
        &["config", "user.email", "mcp@example.com"],
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
    d.canonicalize().unwrap()
}

/// One call to `tool`: the is-error flag and the result text.
fn call(dir: &Path, tool: &str, args: serde_json::Value) -> (bool, String) {
    let mut c = Command::new(env!("CARGO_BIN_EXE_fael"))
        .arg("mcp")
        .env("FAEL_STATE_DIR", dir.join("state"))
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .current_dir(dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let req = serde_json::json!({"jsonrpc": "2.0", "id": 0, "method": "tools/call",
        "params": {"name": tool, "arguments": args}});
    writeln!(c.stdin.take().unwrap(), "{req}").unwrap();
    let out = String::from_utf8(c.wait_with_output().unwrap().stdout).unwrap();
    let v: serde_json::Value = serde_json::from_str(out.lines().next().unwrap()).unwrap();
    let text = v["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .to_string();
    (v["result"]["isError"].as_bool().unwrap_or(false), text)
}

fn id_of(text: &str) -> String {
    text.lines().next().unwrap()["recorded ".len()..].to_string()
}

fn sparse_over_cap(d: &Path, rel: &str) {
    let big = std::fs::File::create(d.join(rel)).unwrap();
    big.set_len(16 * 1024 * 1024 + 1).unwrap();
}

fn filed(d: &Path, needle: &str) -> bool {
    let o = Command::new(env!("CARGO_BIN_EXE_fael"))
        .args(["find", needle, "--all"])
        .env("FAEL_STATE_DIR", d.join("state"))
        .current_dir(d)
        .output()
        .unwrap();
    String::from_utf8_lossy(&o.stdout).contains(needle)
}

#[test]
fn add_reports_an_oversize_file_after_the_id() {
    let d = repo();
    sparse_over_cap(&d, "src/big.bin");
    let (err, text) = call(
        &d,
        "add",
        serde_json::json!({"kind": "note", "text": "sized row", "files": ["src/big.bin"]}),
    );
    assert!(!err, "{text}");
    let lines: Vec<&str> = text.lines().collect();
    assert!(
        lines[0].starts_with("recorded ") && lines[0].split(' ').count() == 2,
        "{text}"
    );
    assert_eq!(
        lines[1],
        format!("{NOTE}src/big.bin (over 16 MiB)"),
        "{text}"
    );
    assert!(filed(&d, "sized row"));
}

#[test]
fn rows_batch_names_the_ninth_file_and_a_normal_row_stays_quiet() {
    let d = repo();
    for i in 0..9 {
        std::fs::write(d.join(format!("src/f{i}.rs")), format!("// {i}\n")).unwrap();
    }
    std::fs::write(d.join("src/solo.rs"), "//\n").unwrap();
    std::fs::write(d.join("src/plain.rs"), "//\n").unwrap();
    let nine: Vec<String> = (0..9).map(|i| format!("src/f{i}.rs")).collect();
    let (err, text) = call(
        &d,
        "add",
        serde_json::json!({"rows": [
            {"kind": "note", "text": "nine files", "files": nine},
            {"kind": "note", "text": "one file", "files": ["src/solo.rs"]},
        ]}),
    );
    assert!(!err, "{text}");
    assert!(text.starts_with("recorded "), "{text}");
    assert!(
        text.contains(&format!("{NOTE}src/f8.rs (past the 8-file cap)")),
        "{text}"
    );
    assert_eq!(text.matches("not stamped").count(), 1, "{text}");
    assert_eq!(text.matches("recorded ").count(), 2, "{text}");
    assert!(filed(&d, "nine files") && filed(&d, "one file"));

    let (err, text) = call(
        &d,
        "add",
        serde_json::json!({"kind": "note", "text": "plain", "files": ["src/plain.rs"]}),
    );
    assert!(!err && text.lines().count() == 1, "{text}");
}

#[test]
fn bare_bump_reports_and_a_routing_bump_does_not() {
    let d = repo();
    sparse_over_cap(&d, "src/big.bin");
    let (_, text) = call(
        &d,
        "add",
        serde_json::json!({"kind": "issue", "text": "big one", "files": ["src/big.bin"]}),
    );
    let id = id_of(&text);
    let (err, text) = call(&d, "bump", serde_json::json!({"id": id, "urgent": true}));
    assert!(!err && !text.contains("not stamped"), "{text}");
    let id = id_of(&text);
    let (err, text) = call(&d, "bump", serde_json::json!({"id": id}));
    assert!(!err, "{text}");
    assert!(text.starts_with("recorded "), "{text}");
    assert!(
        text.contains(&format!("{NOTE}src/big.bin (over 16 MiB)")),
        "{text}"
    );
}
