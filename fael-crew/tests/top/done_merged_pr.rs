//! Issue 01M5QF7Q: `done --pr N` for a run with no branch claims the files of the squash
//! commit on the trunk titled `… (#N)`; with no such commit it says nothing was claimed.

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

fn fael(dir: &Path, state: &Path, args: &[&str]) -> String {
    let o = Command::new(crate::common::fael_bin())
        .args(args)
        .current_dir(dir)
        .env("FAEL_STATE_DIR", state)
        .env_remove("FAEL_DIR")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env_remove("FAEL_SESSION")
        .env_remove("CODEX_THREAD_ID")
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(o.status.success(), "{args:?}: {err}");
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

fn git(dir: &Path, args: &[&str]) {
    let id = ["-c", "user.name=t", "-c", "user.email=t@t"];
    let ok = Command::new("git")
        .args(id)
        .args(args)
        .current_dir(dir)
        .status()
        .unwrap()
        .success();
    assert!(ok, "git {args:?}");
}

/// A repo on `main` (origin/HEAD set) whose last commit is `title`, touching `file`.
fn init(title: &str, file: &str) -> (PathBuf, PathBuf) {
    let tmp = std::env::temp_dir().join(format!("fael-done-merged-{}", fael_core::ulid()));
    let (d, state) = (tmp.join("repo"), tmp.join("state"));
    std::fs::create_dir_all(&d).unwrap();
    git(&d, &["init", "-q", "-b", "main"]);
    std::fs::write(d.join("README"), "base").unwrap();
    git(&d, &["add", "README"]);
    git(&d, &["commit", "-qm", "init"]);
    let p = d.join(file);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(&p, "x").unwrap();
    git(&d, &["add", file]);
    git(&d, &["commit", "-qm", title]);
    git(&d, &["update-ref", "refs/remotes/origin/main", "main"]);
    let head = "refs/remotes/origin/HEAD";
    git(&d, &["symbolic-ref", head, "refs/remotes/origin/main"]);
    (d, state)
}

fn started(d: &Path, state: &Path) -> String {
    let args = ["--brief", "b", "--scope", "features/duties/"];
    let uid = fael(d, state, &[&["chunk", "add", "a"], &args[..]].concat());
    let start = ["chunk", "start", &uid, "--run", "R1", "--no-worktree"];
    fael(d, state, &start);
    uid
}

#[test]
fn a_run_without_a_branch_claims_the_merged_prs_files() {
    let (d, state) = init("feat: duty (#42)", "features/tax/duty.rs");
    let uid = started(&d, &state);
    let done = ["chunk", "done", &uid, "h", "--left", "none", "--pr", "42"];
    let out = fael(&d, &state, &done);
    assert!(!out.contains("no diff claimed"), "{out}");
    let show = fael(&d, &state, &["chunk", "show", &uid, "--json"]);
    let c: Value = serde_json::from_str(&show).unwrap();
    assert_eq!(c["scope"], serde_json::json!(["features/tax/duty.rs"]));
}

#[test]
fn a_later_commit_naming_the_pr_in_its_body_is_not_the_merge() {
    let (d, state) = init("feat: duty (#42)", "features/tax/duty.rs");
    std::fs::write(d.join("other.rs"), "x").unwrap();
    git(&d, &["add", "other.rs"]);
    git(
        &d,
        &["commit", "-qm", "chore: x", "-m", "follow-up to (#42)"],
    );
    git(&d, &["update-ref", "refs/remotes/origin/main", "main"]);
    let uid = started(&d, &state);
    let done = ["chunk", "done", &uid, "h", "--left", "none", "--pr", "42"];
    fael(&d, &state, &done);
    let show = fael(&d, &state, &["chunk", "show", &uid, "--json"]);
    let c: Value = serde_json::from_str(&show).unwrap();
    assert_eq!(c["scope"], serde_json::json!(["features/tax/duty.rs"]));
}

#[test]
fn no_branch_and_no_pr_commit_says_nothing_was_claimed() {
    let (d, state) = init("feat: duty (#41)", "features/tax/duty.rs");
    let uid = started(&d, &state);
    let done = ["chunk", "done", &uid, "h", "--left", "none", "--pr", "42"];
    let out = fael(&d, &state, &done);
    let want = "no diff claimed: no run branch and no commit on origin/main names (#42) — the next brief prints no touched files; name the paths in the handoff";
    assert!(out.lines().any(|l| l == want), "{out}");
}

/// Issue 01M62VY2: a review that changed nothing says so — the hand-off carries it, the
/// no-diff line names the flag when there is no PR, and a PR with `--no-change` is refused.
#[test]
fn no_change_records_a_review_that_changed_nothing() {
    let (d, state) = init("feat: duty (#41)", "features/tax/duty.rs");
    let uid = started(&d, &state);
    let plain = ["chunk", "done", &uid, "clean", "--left", "none"];
    let out = fael(&d, &state, &plain);
    assert!(out.contains("--no-change"), "{out}");
    let (d, state) = init("feat: duty (#41)", "features/tax/duty.rs");
    let uid = started(&d, &state);
    let done = [
        "chunk",
        "done",
        &uid,
        "clean",
        "--left",
        "none",
        "--no-change",
    ];
    let out = fael(&d, &state, &done);
    assert!(out.contains("recorded: reviewed, no change"), "{out}");
    assert!(!out.contains("no diff claimed"), "{out}");
    let show = fael(&d, &state, &["chunk", "show", &uid]);
    assert!(show.contains("reviewed, no change: clean"), "{show}");
}

#[test]
fn no_change_with_a_pr_is_refused() {
    let (d, state) = init("feat: duty (#41)", "features/tax/duty.rs");
    let uid = started(&d, &state);
    let bad = [
        "chunk",
        "done",
        &uid,
        "h",
        "--left",
        "none",
        "--pr",
        "42",
        "--no-change",
    ];
    let o = Command::new(crate::common::fael_bin())
        .args(bad)
        .current_dir(&d)
        .env("FAEL_STATE_DIR", &state)
        .output()
        .unwrap();
    assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stderr).contains("--no-change says nothing"));
}

/// Issue 01M62XEX: a uid-less `done --no-change` from a branch that has an open PR is not
/// refused — the PR `gh` finds is not the work reviewed.
// a shell `gh` stub: Unix only
#[cfg(unix)]
#[test]
fn no_change_ignores_the_pr_gh_finds_for_the_branch() {
    let (d, state) = init("feat: duty (#41)", "features/tax/duty.rs");
    let args = ["--brief", "b", "--scope", "features/duties/"];
    let uid = fael(&d, &state, &[&["chunk", "add", "a"], &args[..]].concat());
    fael(&d, &state, &["chunk", "start", &uid, "--run", "R1"]);
    let bin = d.parent().unwrap().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let gh = bin.join("gh");
    std::fs::write(&gh, "#!/bin/sh\necho 5\n").unwrap();
    std::fs::set_permissions(&gh, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());
    let o = Command::new(crate::common::fael_bin())
        .args(["chunk", "done", "clean", "--left", "none", "--no-change"])
        .current_dir(&d)
        .env("FAEL_STATE_DIR", &state)
        .env("PATH", path)
        .env_remove("FAEL_DIR")
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(
        o.status.success(),
        "{out}{}",
        String::from_utf8_lossy(&o.stderr)
    );
    assert!(out.contains("recorded: reviewed, no change"), "{out}");
    let show = fael(&d, &state, &["chunk", "show", &uid]);
    assert!(show.contains("reviewed, no change: clean"), "{show}");
}
