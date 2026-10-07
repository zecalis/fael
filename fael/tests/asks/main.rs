//! Chunk 3a (PLAN-fael-durable-log): every ask fael costs the agent — a
//! `rejected:` write or a warning line — is counted in
//! usage.jsonl with its type, and `stats` shows the split. `replay` is the
//! fixed baseline sequence the plan requires before self-heal (3b–e): real
//! cases (5-note debt, Supersedes-in-text, key candidates) plus the reject
//! and warning paths. Thin entry only — suites sit next to this file.

mod concurrent;
mod counting;
mod friction;
mod replay;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Per-child `FAEL_STATE_DIR` at `<repo root>/state`, so a real session on
/// this machine never leaks in and tests run in parallel.
fn state_env(c: &mut Command, dir: &Path) {
    let root = dir.ancestors().find(|p| p.join(".git").exists()).unwrap();
    c.env("FAEL_STATE_DIR", root.join("state"));
}

fn fael(dir: &Path, args: &[&str], stdin: &str) -> (bool, String, String) {
    fael_env(dir, args, stdin, &[])
}

/// `fael` with extra env — for tests that need a knob the child reads (e.g.
/// `FAEL_BURST_MS` to file past the same-burst window without a real sleep).
fn fael_env(
    dir: &Path,
    args: &[&str],
    stdin: &str,
    envs: &[(&str, &str)],
) -> (bool, String, String) {
    let mut c = Command::new(env!("CARGO_BIN_EXE_fael"));
    c.args(args).current_dir(dir);
    state_env(&mut c, dir);
    c.env_remove("FAEL_SESSION");
    c.env_remove("CLAUDE_CODE_SESSION_ID");
    c.env_remove("CODEX_THREAD_ID");
    for (k, v) in envs {
        c.env(k, v);
    }
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
    let d = std::env::temp_dir().join(format!("fael-asks-{}", fael_core::ulid()));
    std::fs::create_dir_all(d.join("src")).unwrap();
    for args in [
        &["init", "-q"][..],
        &["config", "user.name", "Ask Test"],
        &["config", "user.email", "ask@example.com"],
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
    d
}

/// Usage rows recorded so far in this repo's scratch state dir, minus the
/// `call` lines (friction.rs reads those).
fn usage(d: &Path) -> Vec<serde_json::Value> {
    let mut u = all_usage(d);
    u.retain(|v| v["event"] != "call");
    u
}

fn all_usage(d: &Path) -> Vec<serde_json::Value> {
    let root = d.ancestors().find(|p| p.join(".git").exists()).unwrap();
    std::fs::read_to_string(root.join("state/usage.jsonl"))
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}

fn stats_json(d: &Path) -> serde_json::Value {
    let (ok, out, err) = fael(d, &["stats", "--json"], "");
    assert!(ok, "{err}");
    serde_json::from_str(&out).unwrap()
}
