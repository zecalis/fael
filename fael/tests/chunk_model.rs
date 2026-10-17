//! The board's `model` is the one the launcher runs (`opencode run --model`): a start
//! `--model` beats the approval, which beats the hint — the brief never prints stale.

use std::path::{Path, PathBuf};
use std::process::Command;

fn fael(dir: &Path, args: &[&str]) -> (bool, String, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_fael"))
        .args(args)
        .current_dir(dir)
        .env("FAEL_STATE_DIR", dir.join("state"))
        .env_remove("FAEL_DIR")
        .env_remove("CLAUDE_CODE_SESSION_ID").env_remove("FAEL_SESSION").env_remove("CODEX_THREAD_ID")
        .output()
        .unwrap();
    let s = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    (o.status.success(), s(&o.stdout), s(&o.stderr))
}

fn repo() -> PathBuf {
    let d = std::env::temp_dir().join(format!("fael-chunk-model-{}", fael_core::ulid()));
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

fn model_of(d: &Path, u: &str) -> (serde_json::Value, serde_json::Value) {
    let (_, json, _) = fael(d, &["board", "--json"]);
    let b: serde_json::Value = serde_json::from_str(&json).unwrap();
    let c = b["projects"][0]["chunks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["uid"] == u)
        .cloned()
        .unwrap();
    (c["model"].clone(), c["model_hint"].clone())
}

#[test]
fn the_board_model_is_what_the_start_runs() {
    let d = repo();
    // a start --model beats the hint, in the brief and on the board
    let (_, out, _) = fael(
        &d,
        &["chunk", "add", "Ui", "--brief", "b", "--model", "hint-m"],
    );
    let u = out.trim().to_string();
    assert_eq!(model_of(&d, &u).0, serde_json::json!("hint-m"));
    let (ok, brief, _) = fael(
        &d,
        &["chunk", "start", &u, "--run", "R1", "--model", "run-m"],
    );
    assert!(
        ok && brief.contains("model run-m") && !brief.contains("model hint-m"),
        "{brief}"
    );
    assert_eq!(
        model_of(&d, &u),
        (serde_json::json!("run-m"), serde_json::json!("hint-m"))
    );
    assert!(fael(&d, &["run", "end", "R1"]).0);
    // a bare start runs the approval's model, not the hint's
    let (_, out, _) = fael(
        &d,
        &["chunk", "add", "Api", "--brief", "b", "--model", "hint-m"],
    );
    let v = out.trim().to_string();
    assert!(fael(&d, &["chunk", "approve", &v, "--model", "ok-m"]).0);
    let (ok, brief, _) = fael(&d, &["chunk", "start", &v, "--run", "R2"]);
    assert!(
        ok && brief.contains("model ok-m") && !brief.contains("model hint-m"),
        "{brief}"
    );
    assert_eq!(
        model_of(&d, &v),
        (serde_json::json!("ok-m"), serde_json::json!("hint-m"))
    );
}
