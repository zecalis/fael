//! c5: same-session `chunk start` returns the live R short; precedence
//! resume → wait → hint → push is deterministic rendering only.
//!
//! Lifecycle truth is crew.db (run/event tables via c1/c2), never log text:
//! a retry in the holding session reuses the live run row and prints short
//! unless `--full`.

use std::path::{Path, PathBuf};
use std::process::Command;

fn fael(dir: &Path, args: &[&str]) -> (bool, String, String) {
    let o = Command::new(crate::common::fael_bin())
        .args(args)
        .current_dir(dir)
        .env("FAEL_STATE_DIR", dir.join("state"))
        .env_remove("FAEL_DIR")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env_remove("FAEL_SESSION")
        .env_remove("CODEX_THREAD_ID")
        .output()
        .unwrap();
    let s = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    (o.status.success(), s(&o.stdout), s(&o.stderr))
}

fn repo() -> PathBuf {
    let d = std::env::temp_dir().join(format!("fael-resume-{}", fael_core::ulid()));
    std::fs::create_dir_all(&d).unwrap();
    assert!(
        Command::new("git")
            .args(["init", "-q", "-b", "main"])
            .current_dir(&d)
            .status()
            .unwrap()
            .success()
    );
    d
}

fn add(d: &Path) -> String {
    let (ok, out, err) = fael(d, &["chunk", "add", "Post", "--brief", "Write it"]);
    assert!(ok, "{err}");
    out.trim().to_string()
}

/// Same-session retry returns the live R, short: resume + TLDR + `--full`
/// pointer, no sections, no held rows. `--full` reprints the whole brief.
#[test]
fn start_resume_same_session_returns_live_r_short() {
    let d = repo();
    let u = add(&d);
    let (ok, first, err) = fael(&d, &["chunk", "start", &u, "--session", "S1"]);
    assert!(ok, "{err}");
    let run = first
        .lines()
        .next()
        .unwrap_or_default()
        .split_whitespace()
        .nth(2)
        .unwrap_or_default()
        .to_string();
    assert!(!run.is_empty() && first.contains("Write it"), "{first}");
    let (ok, second, err) = fael(&d, &["chunk", "start", &u, "--session", "S1"]);
    assert!(ok, "{err}");
    assert!(
        second.contains(&format!("fael run {run}")),
        "same live R: {second}"
    );
    assert!(second.contains("(resumed)"), "{second}");
    // short: TLDR + resume pointer, no plan sections, no held rows
    assert!(second.contains("TLDR:"), "{second}");
    assert!(
        second.contains("--full"),
        "short points at --full: {second}"
    );
    assert!(
        !second.contains("## 1. Goal") && !second.contains("## chunk rules"),
        "short, not the whole brief: {second}"
    );
    assert!(
        second.len() < first.len(),
        "second print is short: {} vs {}",
        second.len(),
        first.len()
    );
    let (ok, full, err) = fael(&d, &["chunk", "start", &u, "--session", "S1", "--full"]);
    assert!(ok, "{err}");
    assert!(
        full.contains(&format!("fael run {run}")) && full.contains("## chunk rules"),
        "full reprints the brief: {full}"
    );
}

/// Precedence is rendering order only: run → resume/TLDR → `--full` pointer.
/// A resumed short suppresses sections and held rows by the session invariant
/// (already shown at the first start), never by rank.
#[test]
fn start_resume_precedence_run_resume_tldr_pointer() {
    let d = repo();
    let u = add(&d);
    assert!(fael(&d, &["chunk", "start", &u, "--session", "S9"]).0);
    // a note + owner answer exist, but the resumed short still prints neither:
    // they stay in the first brief (session invariant).
    assert!(fael(&d, &["chunk", "note", &u, "parser done"]).0);
    let (_, second, _) = fael(&d, &["chunk", "start", &u, "--session", "S9"]);
    let (run, tldr, ptr) = (
        second.find("fael run "),
        second.find("TLDR:"),
        second.find("--full"),
    );
    assert!(run < tldr && tldr < ptr, "run → TLDR → pointer: {second}");
    assert!(
        !second.contains("parser done"),
        "notes stay in the first brief: {second}"
    );
}

/// Shape-check `--on data` before record: no `--until` rejects, writes nothing,
/// prints no hint. The hint prints only on valid shape, after the receipt.
#[test]
fn start_resume_wait_data_needs_until_before_record() {
    let d = repo();
    let u = add(&d);
    assert!(fael(&d, &["chunk", "start", &u, "--run", "R1"]).0);
    let (ok, out, err) = fael(&d, &["chunk", "wait", &u, "hits", "--on", "data"]);
    assert!(!ok && err.contains("--until"), "shape first: {err} {out}");
    assert!(!out.contains("hint:"), "no hint on invalid shape: {out}");
    let (_, show, _) = fael(&d, &["chunk", "show", &u, "--json"]);
    assert!(
        show.contains("\"state\":\"running\"") || !show.contains("\"state\":\"waiting\""),
        "nothing recorded: {show}"
    );
    let (ok, out, err) = fael(
        &d,
        &[
            "chunk",
            "wait",
            &u,
            "hits",
            "--on",
            "data",
            "--until",
            "2026-12-01",
        ],
    );
    assert!(ok, "{err}");
    // receipt first, hint second — deterministic rendering, not rank.
    let (receipt, hint) = (out.find("waiting on data"), out.find("hint:"));
    assert!(receipt < hint, "receipt before hint: {out}");
}
