//! A pull's usage row (`found`) carries the bytes the agent saw (issue 01M5HBR6),
//! and stats still keeps it out of the injection totals.

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
    let d = std::env::temp_dir().join(format!("fael-found-bytes-{}", fael_core::ulid()));
    std::fs::create_dir_all(&d).unwrap();
    let git = Command::new("git")
        .args(["init", "-q", "-b", "main"])
        .current_dir(&d)
        .status()
        .unwrap();
    assert!(git.success());
    d
}

#[test]
fn a_find_row_records_the_bytes_shown() {
    let d = repo();
    let (ok, _, err) = fael(&d, &["add", "note", "pull cost probe", "--files", "a.rs"]);
    assert!(ok, "{err}");
    let (ok, out, err) = fael(&d, &["find", "--files", "a.rs"]);
    assert!(ok && !out.is_empty(), "{out}{err}");
    let text = std::fs::read_to_string(d.join("state/usage.jsonl")).unwrap_or_default();
    let row = text
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .find(|v| v["event"] == "find" && v.get("found").is_some())
        .unwrap_or_else(|| panic!("no found row in {text}"));
    assert_eq!(row["bytes"], out.len(), "{row}");
    assert!(row["est_tokens"].as_u64().unwrap() > 0, "{row}");
}
