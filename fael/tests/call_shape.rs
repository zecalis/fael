//! shape-log: a reject/help call row carries the call's shape (`verb`, flag
//! names, positional count) and never a value.

use std::path::{Path, PathBuf};
use std::process::Command;

fn fael(dir: &Path, args: &[&str]) -> (bool, String, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_fael"))
        .args(args)
        .current_dir(dir)
        .env("FAEL_STATE_DIR", dir.join("state"))
        .env("FAEL_SESSION", "s1")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env_remove("CODEX_THREAD_ID")
        .output()
        .unwrap();
    let s = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    (o.status.success(), s(&o.stdout), s(&o.stderr))
}

fn repo() -> PathBuf {
    let d = std::env::temp_dir().join(format!("fael-call-shape-{}", fael_core::ulid()));
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

/// The call rows one fael run wrote, oldest first.
fn calls(d: &Path) -> Vec<serde_json::Value> {
    let text = std::fs::read_to_string(d.join("state/usage.jsonl")).unwrap_or_default();
    text.lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .filter(|v: &serde_json::Value| v["event"] == "call")
        .collect()
}

#[test]
fn a_reject_carries_shape_never_values() {
    let d = repo();
    let (ok, _, err) = fael(&d, &["chunk", "show", "a", "b", "--json"]);
    assert!(!ok, "{err}");
    let rows = calls(&d);
    assert_eq!(rows.len(), 1, "{rows:?}");
    let r = &rows[0];
    assert_eq!(r["outcome"], "reject");
    assert_eq!(r["verb"], "show");
    assert_eq!(r["flags"], serde_json::json!(["json"]));
    assert_eq!(r["npos"], 2);
    // the operands stay out of the row — quoted, since the sig hash may
    // carry the same letters unquoted
    let line = r.to_string();
    assert!(!line.contains("\"a\"") && !line.contains("\"b\""), "{line}");
}

#[test]
fn an_ok_row_carries_flag_names_never_values() {
    let d = repo();
    let (ok, _, err) = fael(&d, &["find", "--recall", "wide", "src/a.rs"]);
    assert!(ok, "{err}");
    let rows = calls(&d);
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0]["outcome"], "ok");
    assert_eq!(rows[0]["flags"], serde_json::json!(["recall"]));
    assert_eq!(rows[0]["npos"], 1);
    assert!(rows[0].get("who").is_none(), "{rows:?}");
    let line = rows[0].to_string();
    assert!(
        !line.contains("\"wide\"") && !line.contains("a.rs"),
        "{line}"
    );
}

#[test]
fn a_call_with_no_session_is_a_human_row_outside_friction() {
    let d = repo();
    let o = Command::new(env!("CARGO_BIN_EXE_fael"))
        .args(["stats", "--json"])
        .current_dir(&d)
        .env("FAEL_STATE_DIR", d.join("state"))
        .env_remove("FAEL_SESSION")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env_remove("CODEX_THREAD_ID")
        .output()
        .unwrap();
    assert!(o.status.success());
    let rows = calls(&d);
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0]["who"], "human");
    assert_eq!(rows[0]["cmd"], "stats");
    assert_eq!(rows[0]["flags"], serde_json::json!(["json"]));
    // an agent call in the same state; stats counts both as usage, only the agent as friction
    let (ok, _, err) = fael(&d, &["find", "--recall", "wide"]);
    assert!(ok, "{err}");
    let o = Command::new(env!("CARGO_BIN_EXE_fael"))
        .args(["stats", "--json"])
        .current_dir(&d)
        .env("FAEL_STATE_DIR", d.join("state"))
        .env("FAEL_SESSION", "s1")
        .output()
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["friction"]["calls"], 1, "{v}");
    let by = v["usage"]["by_flag"].as_array().unwrap();
    let f = by
        .iter()
        .find(|u| u["cmd"] == "find" && u["flag"] == "recall")
        .unwrap();
    assert_eq!(
        (f["agent"].clone(), f["human"].clone()),
        (1.into(), 0.into())
    );
    assert!(
        by.iter()
            .any(|u| u["cmd"] == "stats" && u["flag"] == "json" && u["human"] == 1)
    );
}

#[test]
fn a_help_row_carries_shape() {
    let d = repo();
    let (ok, _, _) = fael(&d, &["chunk", "approve", "--help"]);
    assert!(ok);
    let rows = calls(&d);
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0]["outcome"], "help");
    assert_eq!(rows[0]["verb"], "approve");
    assert_eq!(rows[0]["flags"], serde_json::json!(["help"]));
    assert_eq!(rows[0]["npos"], 0);
}

#[test]
fn flag_values_and_positions_never_log() {
    let d = repo();
    let (ok, _, err) = fael(&d, &["find", "--files", "src/a.rs", "--bogus", "v"]);
    assert!(!ok, "{err}");
    let rows = calls(&d);
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0]["outcome"], "reject");
    assert_eq!(rows[0]["flags"], serde_json::json!(["bogus", "files"]));
    assert_eq!(rows[0]["npos"], 1);
    let line = rows[0].to_string();
    for secret in ["\"src/a.rs\"", "\"v\""] {
        assert!(!line.contains(secret), "{line}");
    }
}
