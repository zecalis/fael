//! `done --pr` (issue 01M4QFJ6V): the number, `owner/repo#N` or the PR URL; another
//! repo's PR is refused, never dropped, and a repo's own matches its origin in any case.

mod common;

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn run(dir: &Path, state: &Path, args: &[&str]) -> Output {
    Command::new(common::fael_bin())
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

fn git(d: &Path, args: &[&str]) {
    let s = Command::new("git").args(args).current_dir(d).status();
    assert!(s.unwrap().success(), "git {args:?}");
}

fn repo(tmp: &Path, name: &str) -> PathBuf {
    let d = tmp.join(name);
    std::fs::create_dir_all(&d).unwrap();
    git(&d, &["init", "-q", "-b", "main"]);
    d.canonicalize().unwrap()
}

#[test]
fn done_takes_the_pr_url_and_refuses_another_repos() {
    let t = std::env::temp_dir().join(format!("fael-pr-{}", fael_core::ulid()));
    let (d, state) = (repo(&t, "board"), t.join("state"));
    git(
        &d,
        &[
            "remote",
            "add",
            "origin",
            "git@github.com:zecalis/fael-board.git",
        ],
    );
    let f = |args: &[&str]| ok(&d, &state, args);
    let a = f(&["chunk", "add", "a", "--brief", "b"]);
    f(&["chunk", "start", &a, "--run", "R1", "--no-worktree"]);
    let done = |pr: &str| {
        run(
            &d,
            &state,
            &["chunk", "done", &a, "h", "--pr", pr, "--run", "R1"],
        )
    };
    let err = String::from_utf8_lossy(&done("zecalis/fael#7").stderr).into_owned();
    assert!(
        err.contains("is not this repo's (zecalis/fael-board)"),
        "{err}"
    );
    assert!(
        done("https://github.com/Zecalis/fael-board/pull/7")
            .status
            .success()
    );
    let c: Value = serde_json::from_str(&f(&["chunk", "show", &a, "--json"])).unwrap();
    assert_eq!(c["run"]["pr"], 7);
}
