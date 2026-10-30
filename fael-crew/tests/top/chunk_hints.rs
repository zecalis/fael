//! Situational chunk rules said by the verb that raises them (issue 01M5Y6H4): the wait
//! format when the question lacks `Recommend:`, the lane rules when a lane is seated.

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
    let d = std::env::temp_dir().join(format!("fael-hints-{}", fael_core::ulid()));
    std::fs::create_dir_all(&d).unwrap();
    let mut git = Command::new("git");
    git.args(["init", "-q", "-b", "main"]).current_dir(&d);
    assert!(git.status().unwrap().success());
    d
}

fn add(d: &Path, title: &str, scope: &str) -> String {
    let args = ["chunk", "add", title, "--brief", "b", "--scope", scope];
    let (ok, out, err) = fael(d, &args);
    assert!(ok, "{err}");
    out.trim().to_string()
}

fn started(d: &Path) -> String {
    let u = add(d, "Post", "src/a.rs");
    assert!(fael(d, &["chunk", "start", &u, "--run", "R1"]).0);
    u
}

#[test]
fn a_wait_on_the_owner_without_a_recommend_prints_the_format_line() {
    let d = repo();
    let u = started(&d);
    let (ok, out, err) = fael(&d, &["chunk", "wait", &u, "A or B?", "--on", "owner"]);
    assert!(ok, "{err}");
    assert!(
        out.contains("hint: a wait --on owner is one question"),
        "{out}"
    );
    assert!(
        out.contains("Recommend: <option> because <evidence>"),
        "{out}"
    );
}

#[test]
fn a_wait_on_the_owner_with_a_recommend_prints_no_hint() {
    let d = repo();
    let u = started(&d);
    let q = "A or B?\nRecommend: A because it is smaller";
    let (ok, out, err) = fael(&d, &["chunk", "wait", &u, q, "--on", "owner"]);
    assert!(ok, "{err}");
    assert!(!out.contains("hint:"), "{out}");
}

#[test]
fn a_wait_on_data_prints_what_it_must_name() {
    let d = repo();
    let u = started(&d);
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
    assert!(
        out.contains("hint: a wait --on data names what is measured"),
        "{out}"
    );
}

#[test]
fn a_lane_start_prints_the_lane_rules_and_a_plain_start_does_not() {
    let d = repo();
    let a = add(&d, "A", "src/");
    let b = add(&d, "B", "src/b.rs");
    let (ok, out, err) = fael(&d, &["chunk", "start", &a, "--run", "RA"]);
    assert!(ok && !out.contains("lane:"), "{out}{err}");
    let args = [
        "chunk", "start", &b, "--run", "RB", "--with", &a, "--as", "lane",
    ];
    let (ok, out, err) = fael(&d, &args);
    assert!(ok, "{err}");
    assert!(out.contains("lane: one scope per worktree"), "{out}");
    assert!(out.contains("`fael run end RB`"), "the run named: {out}");
}
