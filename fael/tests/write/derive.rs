//! Session-derive: `fael add` without `--files` inherits the caller's own
//! session edits, and nothing else.

use super::{edit, fael, fael_as, repo, row_files};
use std::io::Write as _;
use std::process::{Command, Stdio};

#[test]
fn add_without_files_derives_session_edits() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    edit(&d, "s1", &[d.join("src/a.rs")]);
    let (ok, _, err) = fael(&d, &["add", "note", "derived row"], "");
    assert!(ok, "{err}");
    assert_eq!(row_files(&d, "derived row"), ["src/a.rs"]);
}

#[test]
fn add_without_files_or_edits_still_requires_files() {
    let d = repo();
    let (ok, _, err) = fael(&d, &["add", "note", "nothing to derive from"], "");
    assert!(!ok && err.contains("files is required"), "{err}");
}

#[test]
fn edits_before_the_last_row_do_not_derive() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    edit(&d, "s1", &[d.join("src/a.rs")]);
    let (ok, _, err) = fael(
        &d,
        &["add", "note", "covers a.rs", "--files", "src/a.rs"],
        "",
    );
    assert!(ok, "{err}");
    // no edits since that row — the older edit must not leak into the next row
    let (ok, _, err) = fael(&d, &["add", "note", "nothing new"], "");
    assert!(!ok && err.contains("files is required"), "{err}");
}

#[test]
fn edits_from_another_worktree_do_not_derive() {
    let d = repo();
    let other = repo();
    std::fs::write(other.join("src/b.rs"), "// b\n").unwrap();
    // the edit happened in `other`, filed from `d` — same state dir, same
    // session id, different worktree
    edit(&other, "shared", &[other.join("src/b.rs")]);
    let (ok, _, err) = fael(&d, &["add", "note", "nothing here"], "");
    assert!(!ok && err.contains("files is required"), "{err}");
}

#[test]
fn mcp_add_without_files_derives_too() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    edit(&d, "s1", &[d.join("src/a.rs")]);
    let msgs = [
        r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"add","arguments":{"kind":"note","text":"via mcp"}}}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"add","arguments":{"kind":"note","text":"nope"}}}"#,
    ];
    let mut c = Command::new(env!("CARGO_BIN_EXE_fael"))
        .arg("mcp")
        .env("FAEL_STATE_DIR", d.join("state"))
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env_remove("FAEL_SESSION")
        .env_remove("CODEX_THREAD_ID")
        .current_dir(&d)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    c.stdin
        .take()
        .unwrap()
        .write_all((msgs.join("\n") + "\n").as_bytes())
        .unwrap();
    let out = String::from_utf8(c.wait_with_output().unwrap().stdout).unwrap();
    let r: Vec<serde_json::Value> = out
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(r.len(), 2, "{out}");
    // first call derives src/a.rs; the second finds no new edits and fails
    assert_eq!(r[0]["result"]["isError"], false, "{out}");
    assert!(
        r[1]["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("files is required"),
        "{out}"
    );
    assert_eq!(row_files(&d, "via mcp"), ["src/a.rs"]);
}

#[test]
fn concurrent_sessions_never_derive_each_others_files() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    std::fs::write(d.join("src/b.rs"), "// b\n").unwrap();
    edit(&d, "s1", &[d.join("src/a.rs")]);
    // Claude's hook keys the session by transcript path; the CLI sees the id
    edit(
        &d,
        "/home/u/.claude/projects/p/s2.jsonl",
        &[d.join("src/b.rs")],
    );
    // two active sessions and no id: ambiguous, so no derive
    let (ok, _, err) = fael(&d, &["add", "note", "whose"], "");
    assert!(!ok && err.contains("files is required"), "{err}");
    // the caller's own session only, matched through the transcript stem
    let (ok, _, err) = fael_as(&d, &["add", "note", "mine is b"], "", Some("s2"));
    assert!(ok, "{err}");
    assert_eq!(row_files(&d, "mine is b"), ["src/b.rs"]);
}

#[test]
fn more_derived_files_than_the_cap_reject_with_the_list() {
    let d = repo();
    let paths: Vec<_> = (0..9).map(|i| d.join(format!("src/f{i}.rs"))).collect();
    for p in &paths {
        std::fs::write(p, "// f\n").unwrap();
    }
    edit(&d, "s1", &paths);
    let (ok, _, err) = fael(&d, &["add", "note", "too wide"], "");
    assert!(
        !ok && err.contains("--files") && err.contains("src/f8.rs"),
        "{err}"
    );
    let (_, out, _) = fael(&d, &["find", "too wide"], "");
    assert!(!out.contains("too wide"), "nothing written: {out}");
    let (ok, _, err) = fael(&d, &["add", "note", "too wide", "--force"], "");
    assert!(ok, "{err}");
    assert_eq!(row_files(&d, "too wide").len(), 9);
}
