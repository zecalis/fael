//! `done --pr` (issue 01M4QFJ6V): the number, `owner/repo#N` or the PR URL; another
//! repo's PR is refused, never dropped, and a repo's own matches its origin in any case.

use serde_json::Value;
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

fn git(d: &Path, args: &[&str]) {
    let s = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@t"])
        .args(args)
        .current_dir(d)
        .status();
    assert!(s.unwrap().success(), "git {args:?}");
}

fn repo(tmp: &Path, name: &str) -> PathBuf {
    let d = tmp.join(name);
    std::fs::create_dir_all(&d).unwrap();
    git(&d, &["init", "-q", "-b", "main"]);
    d.canonicalize().unwrap()
}

fn repo_with_origin(tmp: &Path, name: &str) -> (PathBuf, PathBuf) {
    let d = repo(tmp, name);
    let state = tmp.join("state");
    std::fs::create_dir_all(d.join("src")).unwrap();
    std::fs::write(d.join("src/x.rs"), "one\n").unwrap();
    git(&d, &["add", "-A"]);
    git(&d, &["commit", "-qm", "init x"]);
    git(&d, &["update-ref", "refs/remotes/origin/main", "main"]);
    git(
        &d,
        &[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/main",
        ],
    );
    assert!(
        run(&d, &state, &["plan", "new", "pl", "PL"])
            .status
            .success()
    );
    (d, state)
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
            &[
                "chunk", "done", &a, "h", "--left", "none", "--pr", pr, "--run", "R1",
            ],
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

/// `--pr 1503,1505` (issue 01M6BKHZH): the last is the run's PR, the earlier ride in the
/// hand-off; a `--pr` that is not the branch's PR gets the one-line fact, never a refusal.
#[test]
fn done_takes_several_prs_and_names_the_branchs_when_it_differs() {
    let t = std::env::temp_dir().join(format!("fael-prs-{}", fael_core::ulid()));
    let (d, state) = (repo(&t, "r"), t.join("state"));
    let f = |args: &[&str]| ok(&d, &state, args);
    let a = f(&["chunk", "add", "a", "--brief", "b"]);
    f(&["chunk", "start", &a, "--run", "R1", "--no-worktree"]);
    let done = |pr: &str| {
        Command::new(crate::common::fael_bin())
            .args([
                "chunk", "done", &a, "h", "--left", "none", "--pr", pr, "--run", "R1",
            ])
            .current_dir(&d)
            .env("FAEL_STATE_DIR", &state)
            .env("FAEL_GH_PR", "1505")
            .output()
            .unwrap()
    };
    let out = String::from_utf8_lossy(&done("1503").stdout).into_owned();
    assert!(
        out.starts_with("note: branch ") && out.contains("--pr 1503,1505"),
        "{out}"
    );
    let o = done("1503,1505");
    let out = String::from_utf8_lossy(&o.stdout).into_owned();
    assert!(!out.contains("note: branch"), "{out}");
    let c: Value = serde_json::from_str(&f(&["chunk", "show", &a, "--json"])).unwrap();
    assert_eq!(c["run"]["pr"], 1505);
    assert!(f(&["chunk", "show", &a]).contains("earlier PRs: #1503"));
}

/// pr-cross: a done chunk's still-open PR touching the same file is named at the
/// next `done`; once its squash commit lands on the base the line is gone. At most
/// 3 lines, uid-ordered. `FAEL_GH_PR_FILES` stands in for the open PR's file list.
#[test]
fn done_names_open_done_prs_touching_same_files_and_hides_merged() {
    let t = std::env::temp_dir().join(format!("fael-pr-cross-{}", fael_core::ulid()));
    let (d, state) = repo_with_origin(&t, "r");
    let f = |args: &[&str]| ok(&d, &state, args);
    let a = f(&[
        "chunk", "add", "a", "--plan", "pl", "--brief", "b", "--scope", "src/x.rs",
    ]);
    let b = f(&[
        "chunk", "add", "b", "--plan", "pl", "--brief", "b", "--scope", "src/x.rs",
    ]);
    f(&["chunk", "start", &a, "--run", "R1", "--no-worktree"]);
    git(&d, &["switch", "-q", "-c", "lane-a"]);
    std::fs::write(d.join("src/x.rs"), "a\n").unwrap();
    git(&d, &["add", "src/x.rs"]);
    git(&d, &["commit", "-qm", "a touch"]);
    f(&[
        "chunk", "done", &a, "did a", "--left", "none", "--pr", "7", "--run", "R1",
    ]);
    git(&d, &["switch", "-q", "main"]);
    git(&d, &["switch", "-q", "-c", "lane-b"]);
    f(&["chunk", "start", &b, "--run", "R2", "--no-worktree"]);
    std::fs::write(d.join("src/x.rs"), "b\n").unwrap();
    git(&d, &["add", "src/x.rs"]);
    git(&d, &["commit", "-qm", "b touch"]);
    let out = Command::new(crate::common::fael_bin())
        .args([
            "chunk", "done", &b, "did b", "--left", "none", "--pr", "8", "--run", "R2",
        ])
        .current_dir(&d)
        .env("FAEL_STATE_DIR", &state)
        .env("FAEL_GH_PR_FILES", "src/x.rs")
        .env_remove("FAEL_DIR")
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(
        out.contains(&format!("also in PR #7 ({a}): src/x.rs")),
        "{out}"
    );
    // A's PR merges: the squash subject lands on the base, so the next done hides it.
    git(&d, &["switch", "-q", "main"]);
    std::fs::write(d.join("src/other.txt"), "m\n").unwrap();
    git(&d, &["add", "src/other.txt"]);
    git(&d, &["commit", "-qm", "feat: land a (#7)"]);
    git(&d, &["update-ref", "refs/remotes/origin/main", "main"]);
    let c = f(&[
        "chunk", "add", "c", "--plan", "pl", "--brief", "b", "--scope", "src/x.rs",
    ]);
    f(&["chunk", "start", &c, "--run", "R3", "--no-worktree"]);
    git(&d, &["switch", "-q", "-c", "lane-c"]);
    std::fs::write(d.join("src/x.rs"), "c\n").unwrap();
    git(&d, &["add", "src/x.rs"]);
    git(&d, &["commit", "-qm", "c touch"]);
    let out = Command::new(crate::common::fael_bin())
        .args([
            "chunk", "done", &c, "did c", "--left", "none", "--pr", "9", "--run", "R3",
        ])
        .current_dir(&d)
        .env("FAEL_STATE_DIR", &state)
        .env("FAEL_GH_PR_FILES", "src/x.rs")
        .env_remove("FAEL_DIR")
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(!out.contains("#7"), "{out}");
    assert!(
        out.contains(&format!("also in PR #8 ({b}): src/x.rs")),
        "{out}"
    );
}
