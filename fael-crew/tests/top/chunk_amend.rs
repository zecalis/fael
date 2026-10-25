//! `chunk done` on a done chunk amends its hand-off, and `chunk note|done` print the brief
//! drift line a hook-less client would otherwise miss (plan fael-chunk-drift, chunk amend).

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn run(dir: &Path, state: &Path, args: &[&str]) -> Output {
    Command::new(crate::common::fael_bin())
        .args(args)
        .current_dir(dir)
        .env("FAEL_STATE_DIR", state)
        .env_remove("FAEL_DIR")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env_remove("FAEL_SESSION")
        .env_remove("CODEX_THREAD_ID")
        .output()
        .unwrap()
}

fn ok(dir: &Path, state: &Path, args: &[&str]) -> String {
    let o = run(dir, state, args);
    assert!(
        o.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

fn repo(tmp: &Path) -> PathBuf {
    let d = tmp.join("r");
    std::fs::create_dir_all(&d).unwrap();
    let s = Command::new("git")
        .args(["init", "-q", "-b", "main"])
        .current_dir(&d)
        .status();
    assert!(s.unwrap().success());
    d.canonicalize().unwrap()
}

#[test]
fn a_second_done_amends_and_a_brief_edit_after_start_shows_in_note_and_done() {
    let t = std::env::temp_dir().join(format!("fael-amend-{}", fael_core::ulid()));
    let (d, state) = (repo(&t), t.join("state"));
    let f = |args: &[&str]| ok(&d, &state, args);
    let a = f(&["chunk", "add", "a", "--brief", "b"]);
    f(&["chunk", "start", &a, "--run", "R1", "--no-worktree"]);
    f(&["chunk", "edit", &a, "--brief", "changed under you"]);
    let note = f(&["chunk", "note", &a, "checkpoint"]);
    let line = format!("chunk {a} brief changed since you started (brief)");
    assert!(note.contains(&line), "{note}");
    let done = f(&[
        "chunk", "done", &a, "first", "--left", "none", "--pr", "7", "--run", "R1",
    ]);
    assert!(done.contains(&line), "{done}");
    let amended = f(&["chunk", "done", &a, "second", "--left", "none"]);
    assert!(
        amended.contains(&format!("handoff amended for {a}")),
        "{amended}"
    );
    let shown = f(&["chunk", "show", &a, "--json"]);
    assert!(
        shown.contains("\"pr\":7") || shown.contains("\"pr\": 7"),
        "{shown}"
    );
    assert!(shown.contains("second"), "{shown}");
}

#[test]
fn a_dropped_chunk_is_not_amended() {
    let t = std::env::temp_dir().join(format!("fael-amend-{}", fael_core::ulid()));
    let (d, state) = (repo(&t), t.join("state"));
    let a = ok(&d, &state, &["chunk", "add", "a", "--brief", "b"]);
    ok(&d, &state, &["chunk", "drop", &a, "no"]);
    let o = run(&d, &state, &["chunk", "done", &a, "x", "--left", "none"]);
    assert!(!o.status.success());
}
