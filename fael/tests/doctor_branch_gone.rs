//! doctor [Gone]/[PartGone] through the real binary: a row filed on a branch
//! that has not merged names files the current checkout lacks — alive on that
//! branch, so never flagged. A file gone from every branch still is.

use std::path::{Path, PathBuf};
use std::process::Command;

fn run(bin: &str, dir: &Path, args: &[&str]) -> (bool, String) {
    let o = Command::new(bin)
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    let s = format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    (o.status.success(), s)
}

fn fael(dir: &Path, args: &[&str]) -> (bool, String) {
    run(env!("CARGO_BIN_EXE_fael"), dir, args)
}

fn git(dir: &Path, args: &[&str]) {
    let (ok, out) = run("git", dir, args);
    assert!(ok, "git {args:?}: {out}");
}

fn repo() -> PathBuf {
    let d = std::env::temp_dir().join(format!("fael-branch-gone-{}", fael_core::ulid()));
    std::fs::create_dir_all(d.join("src")).unwrap();
    git(&d, &["init", "-q"]);
    git(&d, &["config", "user.name", "Test User"]);
    git(&d, &["config", "user.email", "t@example.com"]);
    std::fs::write(d.join("src/a.rs"), "").unwrap();
    git(&d, &["add", "."]);
    git(&d, &["commit", "-q", "-m", "base"]);
    d
}

#[test]
fn rows_of_an_unmerged_branch_are_not_gone() {
    let d = repo();
    // work on a branch: a new file and a row about it (plus src/a.rs, so the
    // row would read as PartGone from the base checkout)
    git(&d, &["switch", "-q", "-c", "feat/x"]);
    std::fs::write(d.join("src/new.rs"), "").unwrap();
    git(&d, &["add", "."]);
    git(&d, &["commit", "-q", "-m", "new"]);
    let (ok, out) = fael(
        &d,
        &[
            "add",
            "decision",
            "branch work",
            "--key",
            "t:x",
            "--files",
            "src/new.rs,src/a.rs",
        ],
    );
    assert!(ok, "{out}");
    // a row on the base whose file is deleted there: really gone
    git(&d, &["switch", "-q", "-"]);
    std::fs::write(d.join("src/old.rs"), "").unwrap();
    let (ok, out) = fael(
        &d,
        &[
            "add",
            "decision",
            "old work",
            "--key",
            "t:o",
            "--files",
            "src/old.rs",
        ],
    );
    assert!(ok, "{out}");
    std::fs::remove_file(d.join("src/old.rs")).unwrap();
    let (_, out) = fael(&d, &["doctor"]);
    assert!(out.contains("note [Gone]: 1 open row(s)"), "{out}");
    assert!(
        !out.contains("[PartGone]") && !out.contains("src/new.rs"),
        "{out}"
    );
    // the branch deleted unmerged: its files exist nowhere, so the row is gone
    git(&d, &["branch", "-q", "-D", "feat/x"]);
    let (_, out) = fael(&d, &["doctor"]);
    assert!(
        out.contains("[PartGone]") && out.contains("src/new.rs"),
        "{out}"
    );
}
