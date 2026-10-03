//! Chunk 4 (PLAN-fael-path-integrity): `fael add` without `--files` inherits
//! the session's edited files, and mistyped paths are rejected with the
//! closest name.
//!
//! Thin entry only — the suites sit next to this file:
//! `derive` (session-derive), `paths` (path evidence + warnings),
//! `refs` (id citations with no row behind them), `urgent` (urgent queue +
//! bump round trip).

mod derive;
mod paths;
mod receipt;
mod refs;
mod replace;
mod urgent;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Per-child `FAEL_STATE_DIR` at `<repo root>/state`, so a real session on this
/// machine never leaks in and tests run in parallel without a global env lock.
fn state_env(c: &mut Command, dir: &Path) {
    let root = dir.ancestors().find(|p| p.join(".git").exists()).unwrap();
    c.env("FAEL_STATE_DIR", root.join("state"));
}

fn fael(dir: &Path, args: &[&str], stdin: &str) -> (bool, String, String) {
    fael_as(dir, args, stdin, None)
}

/// `session` = the `CLAUDE_CODE_SESSION_ID` the caller runs under; `None`
/// clears it, so the agent running these tests never leaks its own id in.
fn fael_as(
    dir: &Path,
    args: &[&str],
    stdin: &str,
    session: Option<&str>,
) -> (bool, String, String) {
    let mut c = Command::new(env!("CARGO_BIN_EXE_fael"));
    c.args(args).current_dir(dir);
    state_env(&mut c, dir);
    match session {
        Some(s) => c.env("CLAUDE_CODE_SESSION_ID", s),
        None => c.env_remove("CLAUDE_CODE_SESSION_ID"),
    };
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
    let d = std::env::temp_dir().join(format!("fael-write-{}", fael_core::ulid()));
    std::fs::create_dir_all(d.join("src")).unwrap();
    for args in [
        &["init", "-q"][..],
        &["config", "user.name", "Write Test"],
        &["config", "user.email", "write@example.com"],
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
    // these tests exercise the tree log: pin it over the `local` default
    std::fs::create_dir_all(d.join(".fael")).unwrap();
    std::fs::write(d.join(".fael/config.toml"), "store = \"tracked\"\n").unwrap();
    // the edit hook only records for adopted repos, and the tests below edit
    // strictly after the first row, so the derive filter (`at > last row`)
    // never ties at ms precision
    let (ok, _, err) = fael(&d, &["add", "note", "seed", "--files", "doc:seed"], "");
    assert!(ok, "{err}");
    std::thread::sleep(std::time::Duration::from_millis(5));
    d
}

/// Record an edit-hook event for `session` touching `files` (absolute paths).
fn edit(d: &Path, session: &str, files: &[PathBuf]) {
    let input = serde_json::json!({"cwd": d, "session": session, "files": files}).to_string();
    let (ok, _, err) = fael(d, &["hook", "edit"], &input);
    assert!(ok, "hook must always exit 0: {err}");
    std::thread::sleep(std::time::Duration::from_millis(5));
}
fn row_files(d: &Path, needle: &str) -> Vec<String> {
    row_json(d, needle)
        .get("files")
        .and_then(|a| a.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|f| f.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

/// The newest version of the row whose text contains `needle` — bumps supersede,
/// so the highest id wins.
fn row_json(d: &Path, needle: &str) -> serde_json::Value {
    let (_, out, _) = fael(d, &["find", "--json", "--all"], "");
    out.lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|v| v["text"].as_str().is_some_and(|t| t.contains(needle)))
        .max_by_key(|v| v["id"].as_str().unwrap_or("").to_string())
        .unwrap()
}
