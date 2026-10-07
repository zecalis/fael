//! `kickoff` marks a handoff `(files changed since)` once a code file it names
//! moved after the row was written (PLAN-fael-file-hash chunk 5a) — the same
//! tag channel as `(merged)`, and never for an unchanged row, the plan file
//! alone, a row that is no handoff, or a handoff with no stamp.

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
        .args(args)
        .current_dir(d)
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn repo() -> PathBuf {
    let d = std::env::temp_dir().join(format!("fael-kickoff-changed-{}", fael_core::ulid()));
    std::fs::create_dir_all(&d).unwrap();
    git(&d, &["init", "-q", "-b", "main"]);
    git(&d, &["config", "user.name", "Changed Test"]);
    git(&d, &["config", "user.email", "changed@example.com"]);
    git(&d, &["commit", "-q", "--allow-empty", "-m", "init"]);
    d
}

/// The kickoff line of the row whose text holds `needle`.
fn line<'a>(out: &'a str, needle: &str) -> &'a str {
    out.lines()
        .find(|l| l.contains(needle))
        .unwrap_or_else(|| panic!("no {needle:?} in:\n{out}"))
}

fn handoff(d: &Path, text: &str, files: &str) {
    fael(
        d,
        &[
            "add",
            "note",
            text,
            "--files",
            files,
            "--key",
            "plan:demo:handoff",
        ],
    );
}

#[test]
fn kickoff_marks_handoff_when_code_moved() {
    let d = repo();
    std::fs::write(d.join("a.rs"), "v1\n").unwrap();
    handoff(&d, "handoff before the fix", "plan:demo,a.rs");
    std::fs::write(d.join("a.rs"), "v2\n").unwrap();

    let out = fael(&d, &["kickoff", "plan:demo"]);
    let l = line(&out, "handoff before the fix");
    assert!(l.ends_with("(files changed since)"), "{out}");
    assert!(!l.contains("@("), "bare suffix, never `@(...)`: {out}");
}

#[test]
fn kickoff_leaves_unchanged_handoff_unmarked() {
    let d = repo();
    std::fs::write(d.join("a.rs"), "v1\n").unwrap();
    handoff(&d, "handoff nothing touched", "plan:demo,a.rs");

    let out = fael(&d, &["kickoff", "plan:demo"]);
    line(&out, "handoff nothing touched");
    assert!(!out.contains("(files changed since)"), "{out}");
}

#[test]
fn kickoff_ignores_the_plan_file_itself() {
    let d = repo();
    std::fs::write(d.join("a.rs"), "v1\n").unwrap();
    std::fs::write(d.join("PLAN-demo.md"), "chunk 1\n").unwrap();
    handoff(
        &d,
        "handoff next chunk planned",
        "plan:demo,PLAN-demo.md,a.rs",
    );
    // the plan file moves every chunk — only it moving labels nothing
    std::fs::write(d.join("PLAN-demo.md"), "chunk 1\nchunk 2\n").unwrap();

    let out = fael(&d, &["kickoff", "plan:demo"]);
    line(&out, "handoff next chunk planned");
    assert!(!out.contains("(files changed since)"), "{out}");
}

#[test]
fn kickoff_leaves_non_handoff_rows_unmarked() {
    let d = repo();
    std::fs::write(d.join("a.rs"), "v1\n").unwrap();
    fael(
        &d,
        &[
            "add",
            "decision",
            "retry uses backoff",
            "--files",
            "plan:demo,a.rs",
        ],
    );
    std::fs::write(d.join("a.rs"), "v2\n").unwrap();

    let out = fael(&d, &["kickoff", "plan:demo"]);
    line(&out, "retry uses backoff");
    assert!(!out.contains("(files changed since)"), "{out}");
}

#[test]
fn kickoff_leaves_unstamped_handoff_unmarked() {
    let d = repo();
    std::fs::write(d.join("a.rs"), "v1\n").unwrap();
    // anchors only: no `fh`, so no verdict — unknown, never changed
    handoff(&d, "handoff filed on the anchor", "plan:demo");
    std::fs::write(d.join("a.rs"), "v2\n").unwrap();

    let out = fael(&d, &["kickoff", "plan:demo"]);
    line(&out, "handoff filed on the anchor");
    assert!(!out.contains("(files changed since)"), "{out}");
}

#[test]
fn kickoff_combines_branch_and_changed_labels() {
    let d = repo();
    git(&d, &["switch", "-qc", "feat/x"]);
    std::fs::write(d.join("a.rs"), "v1\n").unwrap();
    handoff(&d, "handoff from feat/x", "plan:demo,a.rs");
    git(&d, &["switch", "-q", "main"]);
    std::fs::write(d.join("a.rs"), "v2\n").unwrap();

    let out = fael(&d, &["kickoff", "plan:demo"]);
    assert!(
        line(&out, "handoff from feat/x").ends_with("@feat/x (files changed since)"),
        "{out}"
    );
}
