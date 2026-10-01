//! `fael hook` + `fael stats` against throwaway git repos. Each child gets its
//! own `FAEL_STATE_DIR` through `Command::env`, so the tests run in parallel.
//!
//! Thin entry only — the suites sit next to this file:
//! `stop` (turn-end blocks), `autosync` (once-per-session `fael sync`), `stop_lang` ([lang] marker/rows packs),
//! `session` (session-start + read push), `clients` (codex/claude shapes), `stats` (usage accounting),
//! `stats_golden` (PLAN-fael-sync chunk 2 golden pin),
//! `day` (PLAN-fael-sync chunk 3: `fael stats --day`),
//! `push_cap` (read-push row cap + omitted line),
//! `precision` (PLAN-fael-moat-token chunk 2: push footer + branch tag fixture),
//! `focus` (session Focus: focus.json written at start, read by the push),
//! `memory_line` (the `memory: ~used/budget tokens · n rows` line),
//! `tied` (session start lists issues tied to the branch), `notify` (the
//! user channel: brief, reminder, receipt).

mod adopted;
mod autosync;
mod capture;
mod clients;
mod day;
mod focus;
mod memory_line;
mod notify;
mod precision;
mod prompt;
mod push_cap;
mod search;
mod seen;
mod session;
mod stats;
mod stats_golden;
mod stop;
mod stop_lang;
mod stop_risk;
mod tags;
mod tied;
mod writer;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn fael(dir: &Path, args: &[&str], stdin: &str) -> (bool, String, String) {
    fael_at(&state(dir), dir, args, stdin)
}

/// `fael` with extra env — for tests that file rows inside a hook session
/// (`CLAUDE_CODE_SESSION_ID`), which plain `fael` never sets.
fn fael_env(
    dir: &Path,
    args: &[&str],
    stdin: &str,
    envs: &[(&str, &str)],
) -> (bool, String, String) {
    fael_at_env(&state(dir), dir, args, stdin, envs)
}

/// `fael` with an explicit state dir — for tests that switch to a fresh one.
fn fael_at(state: &Path, dir: &Path, args: &[&str], stdin: &str) -> (bool, String, String) {
    fael_at_env(state, dir, args, stdin, &[])
}

/// `fael` with an explicit state dir AND extra env — for tests that need
/// both (e.g. a child `TMPDIR` to place the temp-dir boundary somewhere the
/// test controls).
fn fael_at_env(
    state: &Path,
    dir: &Path,
    args: &[&str],
    stdin: &str,
    envs: &[(&str, &str)],
) -> (bool, String, String) {
    let mut c = Command::new(env!("CARGO_BIN_EXE_fael"));
    // a HOME with no client config: session-start's wiring check reads
    // nothing of this machine's ~/.claude
    c.args(args)
        .current_dir(dir)
        .env("FAEL_STATE_DIR", state)
        .env("HOME", state.join("home"));
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
    let d = std::env::temp_dir().join(format!("fael-hook-{}", fael_core::ulid()));
    std::fs::create_dir_all(d.join("src")).unwrap();
    for args in [
        &["init", "-q"][..],
        &["config", "user.name", "Hook Test"],
        &["config", "user.email", "hook@example.com"],
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
    // these tests exercise the tree log: pin it over the `local` default
    std::fs::create_dir_all(d.join(".fael")).unwrap();
    std::fs::write(d.join(".fael/config.toml"), "store = \"tracked\"\n").unwrap();
    d
}

/// A repo that opted into the enforcement mode (`[capture] block = true`) —
/// the Stop-block rules only apply there; the default mode never blocks.
fn repo_blocking() -> PathBuf {
    let d = repo();
    std::fs::create_dir_all(d.join(".fael")).unwrap();
    std::fs::write(d.join(".fael/config.toml"), "[capture]\nblock = true\n").unwrap();
    d
}

fn state(d: &Path) -> PathBuf {
    d.join("state")
}

fn git(d: &Path, args: &[&str]) -> String {
    let o = Command::new("git")
        .args(args)
        .current_dir(d)
        .output()
        .unwrap();
    assert!(o.status.success(), "git {args:?}");
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

fn commit(d: &Path, msg: &str) {
    std::fs::write(d.join("src/a.rs"), format!("// {msg}\n")).unwrap();
    assert!(
        Command::new("git")
            .args(["add", "-A"])
            .current_dir(d)
            .status()
            .unwrap()
            .success()
    );
    assert!(
        Command::new("git")
            .args(["commit", "-q", "-m", msg])
            .current_dir(d)
            .status()
            .unwrap()
            .success()
    );
}

/// A transcript file created strictly after the previous row and before the next commit, so the commit
/// is newer than the session start even at 1-second git granularity.
fn transcript(d: &Path, name: &str) -> PathBuf {
    // a row filed just before must land in an earlier ms than the birthtime,
    // or the hook (`>=` at ms precision) counts it as this session's row
    std::thread::sleep(std::time::Duration::from_millis(5));
    let p = d.join(name);
    std::fs::write(&p, "").unwrap();
    std::thread::sleep(std::time::Duration::from_millis(1100));
    p
}

fn json(v: &Path) -> String {
    serde_json::Value::String(v.to_string_lossy().into_owned()).to_string()
}
