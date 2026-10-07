//! PLAN-fael-agent-ergonomics chunk 1: every agent call leaves a `call` line,
//! and `fael stats` turns them into friction and first-call success.

use super::{all_usage, repo, stats_json};
use std::path::Path;
use std::process::Command;

/// `fael` inside an agent session (`FAEL_SESSION`), or outside one with `None`.
fn call(d: &Path, session: Option<&str>, args: &[&str]) -> (bool, String) {
    let root = d.ancestors().find(|p| p.join(".git").exists()).unwrap();
    let mut c = Command::new(env!("CARGO_BIN_EXE_fael"));
    c.args(args)
        .current_dir(d)
        .env("FAEL_STATE_DIR", root.join("state"))
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env_remove("CODEX_THREAD_ID")
        .env_remove("FAEL_SESSION");
    if let Some(s) = session {
        c.env("FAEL_SESSION", s);
    }
    let o = c.output().unwrap();
    (
        o.status.success(),
        String::from_utf8_lossy(&o.stderr).into_owned(),
    )
}

fn calls(d: &Path) -> Vec<serde_json::Value> {
    let mut u = all_usage(d);
    u.retain(|v| v["event"] == "call");
    u
}

#[test]
fn one_reject_counts_one_and_a_clean_call_is_a_first_call_success() {
    let d = repo();
    let (ok, err) = call(&d, Some("s1"), &["find", "--stale"]);
    assert!(!ok && err.contains("unknown flag --stale"), "{err}");
    let c = calls(&d);
    assert_eq!(c.len(), 1, "{c:?}");
    assert_eq!(
        (
            c[0]["cmd"].as_str(),
            c[0]["outcome"].as_str(),
            c[0]["reason"].as_str()
        ),
        (Some("find"), Some("reject"), Some("unknown_flag"))
    );
    let f = &stats_json(&d)["friction"];
    assert_eq!(
        (f["calls"].as_u64(), f["rejects"].as_u64()),
        (Some(1), Some(1))
    );
    assert_eq!(f["reasons"]["unknown_flag"], 1, "{f}");
    assert_eq!(f["by_command"]["find"]["rejects"], 1, "{f}");

    // a lone success in another session counts as a first-call success
    let d = repo();
    let (ok, err) = call(&d, Some("s1"), &["find", "--files", "src/a.rs"]);
    assert!(ok, "{err}");
    let f = &stats_json(&d)["friction"];
    assert_eq!(
        (f["calls"].as_u64(), f["first_call_ok"].as_u64()),
        (Some(1), Some(1)),
        "{f}"
    );
}

#[test]
fn an_empty_find_then_another_find_is_one_repeat() {
    let d = repo();
    call(&d, Some("s1"), &["find", "nothing-matches-this"]);
    call(&d, Some("s1"), &["find", "nor-this"]);
    let c = calls(&d);
    assert_eq!(c[0]["empty"], true, "{c:?}");
    let f = &stats_json(&d)["friction"];
    assert_eq!(
        (f["find_repeat"].as_u64(), f["first_call_ok"].as_u64()),
        (Some(1), Some(0)),
        "{f}"
    );
}

#[test]
fn help_counts_and_a_call_outside_a_session_does_not() {
    let d = repo();
    call(&d, Some("s1"), &["find", "--help"]);
    let f = &stats_json(&d)["friction"];
    assert_eq!(
        (f["calls"].as_u64(), f["help"].as_u64()),
        (Some(1), Some(1)),
        "{f}"
    );
    let d = repo();
    call(&d, None, &["find", "--stale"]);
    call(&d, None, &["find", "--help"]);
    assert!(
        calls(&d).is_empty(),
        "a human at the keyboard is no agent friction"
    );
}

#[test]
fn hook_and_stats_calls_leave_no_line() {
    let d = repo();
    call(&d, Some("s1"), &["stats"]);
    call(&d, Some("s1"), &["doctor"]);
    assert!(calls(&d).is_empty());
}
