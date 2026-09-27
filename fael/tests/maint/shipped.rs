//! `doctor` for shipped notes: open notes on branches that already merged
//! (squash-safe: judged by branch name + `mergedAt`, never by sha).

use super::{fael, repo, state_env};
use std::path::Path;
use std::process::Command;

fn git(d: &Path, args: &[&str]) {
    let o = Command::new("git")
        .args(args)
        .current_dir(d)
        .output()
        .unwrap();
    assert!(
        o.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&o.stderr)
    );
}

/// `fael doctor` with canned `gh pr list --state merged` output through
/// `FAEL_GH_MERGED_JSON` (same reason as orphan's `FAEL_GH_JSON`: no
/// shell/batch fake survives Windows or real-gh runners).
fn doctor(d: &Path, gh_json: &str) -> (bool, String) {
    let mut c = Command::new(env!("CARGO_BIN_EXE_fael"));
    c.args(["doctor"]).current_dir(d);
    state_env(&mut c, d);
    c.env("FAEL_GH_MERGED_JSON", gh_json);
    let o = c.output().unwrap();
    (
        o.status.success(),
        String::from_utf8_lossy(&o.stdout).into_owned(),
    )
}

/// A note plus a decision filed on `branch` (the stamp comes from git).
/// Decisions never count — only `kind = note` ships.
fn file_rows(d: &Path, branch: &str) {
    git(d, &["checkout", "-qb", branch]);
    std::fs::write(d.join("src/a.rs"), "").unwrap();
    let (ok, _, err) = fael(d, &["add", "note", "landed work", "--files", "src/a.rs"]);
    assert!(ok, "{err}");
    let (ok, _, err) = fael(
        d,
        &["add", "decision", "landed choice", "--files", "src/a.rs"],
    );
    assert!(ok, "{err}");
    let (ok, _, _) = fael(d, &["doctor", "--fix"]);
    assert!(ok);
}

#[test]
fn doctor_flags_shipped_notes() {
    let d = repo();
    file_rows(&d, "feat/shipped-work");
    // squash fixture (§3): the row's sha never reaches main, but the branch
    // name plus a `mergedAt` after the row's birth prove it landed
    let (ok, out) = doctor(
        &d,
        r#"[{"headRefName":"feat/shipped-work","mergedAt":"2099-01-01T00:00:00Z","number":43}]"#,
    );
    assert!(ok, "{out}");
    assert!(
        out.contains("note [Shipped]: 1 open note(s)")
            && out.contains("feat/shipped-work")
            && out.contains("shipped in #43")
            && out.contains("fael close"),
        "{out}"
    );
    // a branch with no merged PR is not flagged
    let (ok, out) = doctor(
        &d,
        r#"[{"headRefName":"feat/other","mergedAt":"2099-01-01T00:00:00Z"}]"#,
    );
    assert!(ok && !out.contains("[Shipped"), "{out}");
    // no merged PR at all: silent
    let (ok, out) = doctor(&d, "[]");
    assert!(ok && !out.contains("[Shipped"), "{out}");
    // unparseable answer: skipped silently
    let (ok, out) = doctor(&d, "not json");
    assert!(ok && !out.contains("[Shipped"), "{out}");
}

#[test]
fn doctor_silent_for_reused_branch_name() {
    let d = repo();
    file_rows(&d, "feat/reused");
    // every timed PR predates the row: a new branch under an old name
    let (ok, out) = doctor(
        &d,
        r#"[{"headRefName":"feat/reused","mergedAt":"2000-01-01T00:00:00Z","number":41}]"#,
    );
    assert!(ok && !out.contains("[Shipped"), "{out}");
}

#[test]
fn doctor_flags_shipped_maybe_from_git_only() {
    let d = repo();
    git(&d, &["commit", "-q", "--allow-empty", "-m", "init"]);
    // the temp repo's first branch is whatever `git init` made — shipped's
    // `git branch --merged` reads the default branch, so pin it to `main`
    let o = Command::new("git")
        .args(["symbolic-ref", "--short", "HEAD"])
        .current_dir(&d)
        .output()
        .unwrap();
    let head = String::from_utf8_lossy(&o.stdout).trim().to_string();
    if head != "main" {
        git(&d, &["branch", "-M", "main"]);
    }
    file_rows(&d, "feat/landed-note");
    git(&d, &["checkout", "-q", "main"]);
    git(&d, &["merge", "-q", "--no-ff", "feat/landed-note"]);
    // no PR on record: the branch is merged locally but there is no merge
    // time, so the note is unconfirmed, never `[Shipped]`
    let (ok, out) = doctor(&d, "[]");
    assert!(ok, "{out}");
    assert!(
        out.contains("note [Shipped?]: 1 open note(s)")
            && out.contains("feat/landed-note")
            && !out.contains("[Shipped]:"),
        "{out}"
    );
}
