//! The seams `chunk start` and `chunk done` close without an LLM (issue 01M5FMHQX): a brief
//! citing a closed row is refused before it claims, and `done` names what it leaves.

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;

fn fael(dir: &Path, args: &[&str]) -> (bool, String, String) {
    let o = Command::new(common::fael_bin())
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
    let d = std::env::temp_dir().join(format!("fael-gates-{}", fael_core::ulid()));
    std::fs::create_dir_all(&d).unwrap();
    let init = Command::new("git")
        .args(["init", "-q", "-b", "main"])
        .current_dir(&d)
        .status()
        .unwrap();
    assert!(init.success());
    assert!(fael(&d, &["plan", "new", "pl", "PL"]).0);
    d
}

fn chunk(d: &Path, title: &str, brief: &str, more: &[&str]) -> String {
    let args = [
        &["chunk", "add", title, "--plan", "pl", "--brief", brief][..],
        more,
    ]
    .concat();
    let (ok, uid, err) = fael(d, &args);
    assert!(ok, "{err}");
    uid.trim().to_string()
}

fn issue(d: &Path, text: &str) -> String {
    let (ok, out, err) = fael(d, &["add", "issue", text, "--files", "a.rs"]);
    assert!(ok, "{err}");
    out.split_whitespace().next().unwrap().to_string()
}

#[test]
fn start_refuses_a_brief_citing_a_closed_row_until_forced() {
    let d = repo();
    let id = issue(&d, "broken x");
    assert!(fael(&d, &["close", &id, "fixed in 5\nmore"]).0);
    let u = chunk(&d, "Fix", &format!("fix {id}"), &[]);
    let (ok, _, err) = fael(&d, &["chunk", "start", &u, "--no-worktree"]);
    assert!(!ok, "started");
    assert!(err.contains(&format!("{id} — closed: fixed in 5")), "{err}");
    assert!(!err.contains("more"), "first line only: {err}");
    assert!(
        err.contains(&format!("fael chunk note {u}")) && err.contains("never `chunk edit`"),
        "{err}"
    );
    let (_, shown, _) = fael(&d, &["chunk", "show", &u, "--json"]);
    assert!(
        shown.contains("\"state\": \"open\"") || shown.contains("\"state\":\"open\""),
        "claimed: {shown}"
    );
    // show only warns
    assert!(fael(&d, &["chunk", "show", &u]).0);
    let (ok, _, err) = fael(&d, &["chunk", "start", &u, "--no-worktree", "--force"]);
    assert!(ok, "{err}");
}

#[test]
fn done_names_its_leftovers_and_the_next_brief_prints_them() {
    let d = repo();
    let a = chunk(&d, "A", "do a", &[]);
    let b = chunk(&d, "B", "do b", &["--after", &a]);
    let open = issue(&d, "still broken");
    let shut = issue(&d, "was broken");
    assert!(fael(&d, &["close", &shut, "ok"]).0);
    assert!(fael(&d, &["chunk", "start", &a, "--no-worktree", "--run", "R1"]).0);
    let done = |h: &str, left: &[&str]| {
        let args = [
            &["chunk", "done", &a, h, "--run", "R1", "--pr", "7"][..],
            left,
        ]
        .concat();
        fael(&d, &args)
    };
    let (ok, _, err) = done("h", &[]);
    assert!(
        !ok && err.contains("needs --left") && err.contains("fael add issue"),
        "{err}"
    );
    let (ok, _, err) = done("h", &["--left", &shut]);
    assert!(
        !ok && err.contains(&shut),
        "closed row is no leftover: {err}"
    );
    let (ok, _, err) = done("h", &["--left", "none,nope"]);
    assert!(!ok, "{err}");
    let list = format!("{open},{b}");
    let (ok, _, err) = done("shipped", &["--left", &list]);
    assert!(ok, "{err}");
    let (ok, brief, err) = fael(&d, &["chunk", "start", &b, "--no-worktree"]);
    assert!(ok, "{err}");
    assert!(
        brief.contains("shipped") && brief.contains(&format!("left: {open},{b}")),
        "{brief}"
    );
}

#[test]
fn a_finished_note_carries_the_statement_as_a_left_line() {
    let d = repo();
    let a = chunk(&d, "A", "do a", &[]);
    assert!(fael(&d, &["chunk", "start", &a, "--no-worktree", "--run", "R1"]).0);
    let (ok, _, err) = fael(
        &d,
        &[
            "chunk",
            "done",
            &a,
            "x\nleft: none",
            "--run",
            "R1",
            "--pr",
            "7",
        ],
    );
    assert!(ok, "{err}");
    let (_, j, _) = fael(&d, &["chunk", "show", &a, "--json"]);
    assert!(j.contains("done"), "{j}");
    let (ok, _, err) = fael(
        &d,
        &["chunk", "done", &a, "x\nleft: none", "--left", "none"],
    );
    assert!(!ok && err.contains("once"), "{err}");
}
