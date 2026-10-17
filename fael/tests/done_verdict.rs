//! `fael chunk done --verdict` (plan fael-intake i3): a ticket chunk's verdict shows as
//! `ticket.verdict` on `fael board --json`; a plain chunk or an unknown word is rejected.

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn run(d: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fael"))
        .args(args)
        .current_dir(d)
        .env("FAEL_STATE_DIR", d.join("state"))
        .env_remove("FAEL_DIR")
        .env_remove("FAEL_SESSION")
        .env_remove("CLAUDE_CODE_SESSION_ID").env_remove("FAEL_SESSION").env_remove("CODEX_THREAD_ID")
        .env_remove("CODEX_THREAD_ID")
        .output()
        .unwrap()
}

fn fael(d: &Path, args: &[&str]) -> String {
    let o = run(d, args);
    assert!(
        o.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).into_owned()
}

/// A git repo whose intake brought in one open idea, `v-0042`.
fn repo() -> PathBuf {
    let d = std::env::temp_dir().join(format!("fael-verdict-{}", fael_core::ulid()));
    std::fs::create_dir_all(d.join(".fael")).unwrap();
    let ok = Command::new("git")
        .args(["init", "-q", "-b", "main"])
        .current_dir(&d)
        .status()
        .unwrap()
        .success();
    assert!(ok);
    let d = d.canonicalize().unwrap();
    let cat = if cfg!(windows) {
        "type list.jsonl"
    } else {
        "cat list.jsonl"
    };
    let s = serde_json::json!({ "t": { "list": cat, "read": "show {ref}", "draft": "draft {ref}",
                                       "open": "https://staff.test/i/{ref}" } });
    std::fs::write(d.join(".fael/sources.json"), s.to_string()).unwrap();
    let t = serde_json::json!({"ref": "V-0042", "kind": "idea", "title": "dark mode",
                               "reopen": 0, "closed": null});
    std::fs::write(d.join("list.jsonl"), format!("{t}\n")).unwrap();
    fael(&d, &["intake"]);
    d
}

fn chunk(d: &Path, uid: &str) -> Value {
    let v: Value = serde_json::from_str(&fael(d, &["board", "--json"])).unwrap();
    v["projects"][0]["chunks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["uid"] == uid)
        .cloned()
        .unwrap()
}

/// Starts `label` as run R1 and returns its uid.
fn start(d: &Path, label: &str) -> String {
    let v: Value = serde_json::from_str(&fael(d, &["chunk", "show", label, "--json"])).unwrap();
    let uid = v["uid"].as_str().unwrap().to_string();
    fael(d, &["chunk", "start", &uid, "--run", "R1", "--no-worktree"]);
    uid
}

fn done(d: &Path, uid: &str, verdict: &str) -> Output {
    run(
        d,
        &[
            "chunk",
            "done",
            uid,
            "drafted",
            "--verdict",
            verdict,
            "--run",
            "R1",
        ],
    )
}

#[test]
fn a_ticket_chunk_shows_its_verdict_on_the_board() {
    let d = repo();
    let uid = start(&d, "v-0042");
    assert!(chunk(&d, &uid)["ticket"]["verdict"].is_null());
    let o = done(&d, &uid, "decline");
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let c = chunk(&d, &uid);
    assert_eq!(c["ticket"]["verdict"], "decline");
    assert_eq!(c["state"], "review");
}

#[test]
fn a_plain_chunk_or_a_bad_word_is_rejected() {
    let d = repo();
    fael(
        &d,
        &["chunk", "add", "plain", "--label", "p", "--brief", "b"],
    );
    let plain = start(&d, "p");
    let o = done(&d, &plain, "fix");
    assert!(!o.status.success());
    let e = String::from_utf8_lossy(&o.stderr);
    assert!(e.contains("--verdict is for a ticket chunk"), "{e}");
    assert_eq!(chunk(&d, &plain)["state"], "running");

    let d = repo(); // one R per start: a fresh db for the second run
    let uid = start(&d, "v-0042");
    let o = done(&d, &uid, "wontfix");
    assert!(!o.status.success());
    let e = String::from_utf8_lossy(&o.stderr);
    assert!(e.contains("fix|answer|consider|plan|decline"), "{e}");
    assert_eq!(chunk(&d, &uid)["state"], "running");
}
