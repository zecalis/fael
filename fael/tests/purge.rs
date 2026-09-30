//! `fael purge` through the real binary: removing an open row and a closed
//! row (closes go with it), the close-event redirect, rejects, and --json.

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
    let d = std::env::temp_dir().join(format!("fael-purge-{}", fael_core::ulid()));
    std::fs::create_dir_all(d.join("src")).unwrap();
    for args in [
        &["init", "-q"][..],
        &["config", "user.name", "Purge Test"],
        &["config", "user.email", "purge@example.com"],
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
    std::fs::write(d.join("src/a.rs"), "// a.rs\n").unwrap();
    std::fs::write(d.join(".gitattributes"), "*.jsonl merge=union\n").unwrap();
    // these tests exercise the tree log: pin it over the `local` default
    std::fs::create_dir_all(d.join(".fael")).unwrap();
    std::fs::write(d.join(".fael/config.toml"), "store = \"tracked\"\n").unwrap();
    d
}

/// The id `fael add` just filed (stdout starts with it).
fn add(dir: &Path, args: &[&str]) -> String {
    let (ok, out, err) = fael(dir, args, "");
    assert!(ok, "{err}");
    out.split_whitespace().next().unwrap().to_string()
}

/// The close-event id `fael close` just filed (stdout starts with it).
fn close(dir: &Path, id: &str) -> String {
    let (ok, out, err) = fael(dir, &["close", id, "done"], "");
    assert!(ok, "{err}");
    out.split_whitespace().next().unwrap().to_string()
}

/// `find --all` still lists this id (`--json` carries full ids; an
/// id-shaped query for a missing row rejects, which also reads as gone).
fn shows(dir: &Path, id: &str) -> bool {
    let (ok, out, _) = fael(dir, &["find", "--all", "--json", id], "");
    ok && out.contains(&format!("\"id\":\"{id}\""))
}

#[test]
fn purge_removes_open_row_and_second_purge_rejects() {
    let d = repo();
    let id = add(&d, &["add", "note", "leaked probe", "--files", "src/a.rs"]);
    let prefix = id[..8].to_string();
    let (ok, out, err) = fael(&d, &["purge", &prefix], "");
    assert!(ok, "{err}");
    assert!(out.starts_with("purged"), "{out}");
    assert!(out.contains(&id), "names the full id: {out}");
    assert!(!shows(&d, &id), "row is gone");
    let (ok, _, err) = fael(&d, &["purge", &id], "");
    assert!(!ok, "second purge rejects");
    assert!(err.contains("no row"), "{err}");
}

#[test]
fn purge_closed_row_takes_closes_with_it() {
    let d = repo();
    let id = add(&d, &["add", "note", "leaked probe", "--files", "src/a.rs"]);
    close(&d, &id);
    let (ok, out, err) = fael(&d, &["purge", &id], "");
    assert!(ok, "{err}");
    assert!(!out.contains("0 close(s)"), "closes went with it: {out}");
    assert!(!shows(&d, &id), "row and closes are gone");
    let (ok, out, err) = fael(&d, &["doctor", "--json"], "");
    assert!(ok, "{err}");
    assert!(!out.contains(&id), "no phantom left behind: {out}");
}

#[test]
fn purge_close_event_id_points_at_restore() {
    let d = repo();
    let id = add(&d, &["add", "note", "keeper", "--files", "src/a.rs"]);
    let cid = close(&d, &id);
    let (ok, _, err) = fael(&d, &["purge", &cid], "");
    assert!(!ok, "close events are not purged alone");
    assert!(err.contains("restore"), "{err}");
    assert!(shows(&d, &id), "target row untouched");
}

#[test]
fn purge_unknown_rejects_and_json_reports() {
    let d = repo();
    let (ok, _, err) = fael(&d, &["purge", "01M3QA00NOPE"], "");
    assert!(!ok);
    assert!(err.contains("no row"), "{err}");
    let id = add(&d, &["add", "note", "leaked probe", "--files", "src/a.rs"]);
    let (ok, out, err) = fael(&d, &["purge", "--json", &id], "");
    assert!(ok, "{err}");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["purged"], id);
    assert_eq!(v["rows"], 2, "tree + journal: {v}");
}
