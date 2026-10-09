//! `kickoff` marks a branch tag ` (merged)` once that branch's work is in
//! HEAD — squash merges included — and never for a branch that has no
//! commits of its own. `find plan:x` reads the anchor like `kickoff plan:x`.

use std::path::{Path, PathBuf};
use std::process::Command;

fn git(d: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .args(args)
        .current_dir(d)
        .status()
        .unwrap()
        .success();
    assert!(ok, "git {args:?}");
}

fn fael(d: &Path, args: &[&str]) -> String {
    let o = Command::new(env!("CARGO_BIN_EXE_fael"))
        // never the developer's real usage log or session (fael:01M4F3G0)
        .env(
            "FAEL_STATE_DIR",
            std::env::temp_dir().join(format!("fael-test-state-{}", std::process::id())),
        )
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env_remove("FAEL_SESSION")
        .args(args)
        .current_dir(d)
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn repo() -> PathBuf {
    let d = std::env::temp_dir().join(format!("fael-kickoff-merged-{}", fael_core::ulid()));
    std::fs::create_dir_all(&d).unwrap();
    git(&d, &["init", "-q", "-b", "main"]);
    git(&d, &["config", "user.name", "Merge Test"]);
    git(&d, &["config", "user.email", "merge@example.com"]);
    git(&d, &["commit", "-q", "--allow-empty", "-m", "init"]);
    d
}

/// The kickoff line of the row whose text holds `needle`.
fn line<'a>(out: &'a str, needle: &str) -> &'a str {
    out.lines()
        .find(|l| l.contains(needle))
        .unwrap_or_else(|| panic!("no {needle:?} in:\n{out}"))
}

#[test]
fn kickoff_marks_squash_merged_branch_and_not_an_empty_one() {
    let d = repo();
    git(&d, &["switch", "-qc", "feat/x"]);
    fael(
        &d,
        &[
            "add",
            "note",
            "uncommitted on feat/x",
            "--files",
            "plan:demo",
        ],
    );
    std::fs::write(d.join("a.txt"), "a\n").unwrap();
    git(&d, &["add", "a.txt"]);
    git(&d, &["commit", "-qm", "work"]);
    git(&d, &["switch", "-q", "main"]);
    git(&d, &["switch", "-qc", "feat/empty"]);
    fael(
        &d,
        &[
            "add",
            "note",
            "cut but never committed",
            "--files",
            "plan:demo",
        ],
    );
    git(&d, &["switch", "-q", "main"]);

    let out = fael(&d, &["kickoff", "plan:demo"]);
    assert!(line(&out, "uncommitted on").ends_with("@feat/x"), "{out}");
    assert!(
        line(&out, "never committed").ends_with("@feat/empty"),
        "{out}"
    );

    git(&d, &["merge", "-q", "--squash", "feat/x"]);
    git(&d, &["commit", "-qm", "squash feat/x"]);
    let out = fael(&d, &["kickoff", "plan:demo"]);
    assert!(
        line(&out, "uncommitted on").ends_with("@feat/x (merged)"),
        "{out}"
    );
    // still on main's first-parent line: cut from it, never merged
    assert!(
        line(&out, "never committed").ends_with("@feat/empty"),
        "{out}"
    );
    // the tag is kickoff's: find stays free of git spawns
    assert!(!fael(&d, &["find", "plan:demo"]).contains("(merged)"));
}

#[test]
fn find_reads_an_anchor_like_kickoff() {
    let d = repo();
    fael(
        &d,
        &["add", "note", "handoff for demo", "--files", "plan:demo"],
    );
    let out = fael(&d, &["find", "plan:demo"]);
    assert!(out.contains("handoff for demo"), "{out}");
    // a name no row is filed on stays a text search
    let o = Command::new(env!("CARGO_BIN_EXE_fael"))
        // never the developer's real usage log or session (fael:01M4F3G0)
        .env(
            "FAEL_STATE_DIR",
            std::env::temp_dir().join(format!("fael-test-state-{}", std::process::id())),
        )
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env_remove("FAEL_SESSION")
        .args(["find", "plan:other"])
        .current_dir(&d)
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&o.stderr).contains("no rows match \"plan:other\""));
}
