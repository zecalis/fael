//! Chunk 3b (PLAN-fael-durable-log): a repeated note on the same writer +
//! branch + files supersedes the open one itself; several open notes file and
//! list, never ask. Thin entry only — suites sit next to this file.

mod note;

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
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}
