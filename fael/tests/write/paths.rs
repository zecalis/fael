//! Path evidence: typos are rejected with the closest name, globs and
//! anchors pass without evidence, and the rest warn without blocking.

use super::{edit, fael, repo, row_files};
use std::process::Command;

#[test]
fn typo_is_rejected_with_the_closest_name() {
    let d = repo();
    std::fs::write(d.join("src/auth.rs"), "//\n").unwrap();
    let (ok, _, err) = fael(&d, &["add", "note", "x", "--files", "src/autn.rs"], "");
    assert!(!ok, "a near-miss of an existing file must not be filed");
    assert!(
        err.contains("src/auth.rs") && err.contains("did you mean"),
        "{err}"
    );
}

#[test]
fn unmatched_path_without_a_close_sibling_is_warned_not_blocked() {
    let d = repo();
    std::fs::write(d.join("src/live.rs"), "//\n").unwrap();
    // rows about deleted or planned files stay fileable — kickoff/doctor,
    // not the write path, judge those
    let (ok, _, err) = fael(
        &d,
        &["add", "note", "about gone", "--files", "src/gone.rs"],
        "",
    );
    assert!(ok, "{err}");
    assert!(err.contains("matches nothing on disk"), "{err}");
    assert_eq!(row_files(&d, "about gone"), ["src/gone.rs"]);
}

#[test]
fn edited_then_deleted_file_passes_silently() {
    let d = repo();
    std::fs::write(d.join("src/tmp.rs"), "//\n").unwrap();
    edit(&d, "s1", &[d.join("src/tmp.rs")]);
    std::fs::remove_file(d.join("src/tmp.rs")).unwrap();
    // in this session's edits: evidence, even though it is gone from disk
    let (ok, _, err) = fael(
        &d,
        &["add", "note", "about tmp", "--files", "src/tmp.rs"],
        "",
    );
    assert!(ok, "{err}");
    assert!(!err.contains("matches nothing"), "{err}");
}

#[test]
fn glob_and_anchor_pass_without_evidence() {
    let d = repo();
    let (ok, _, err) = fael(
        &d,
        &["add", "note", "pattern row", "--files", "src/*.rs"],
        "",
    );
    assert!(ok, "{err}");
    let (ok, _, err) = fael(
        &d,
        &["add", "note", "anchor row", "--files", "doc:pricing"],
        "",
    );
    assert!(ok, "{err}");
}

#[test]
fn untracked_shell_made_file_passes() {
    let d = repo();
    // the edit hook never saw it (no hook event), but git status knows it
    std::fs::write(d.join("src/shell.rs"), "//\n").unwrap();
    let (ok, _, err) = fael(
        &d,
        &["add", "note", "shell file", "--files", "src/shell.rs"],
        "",
    );
    assert!(ok, "{err}");
}

#[test]
fn force_files_a_planned_sibling_of_an_existing_file() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "//\n").unwrap();
    let (ok, _, err) = fael(&d, &["add", "note", "plan b", "--files", "src/b.rs"], "");
    assert!(!ok && err.contains("--force"), "{err}");
    let (ok, _, err) = fael(
        &d,
        &["add", "note", "plan b", "--files", "src/b.rs", "--force"],
        "",
    );
    assert!(ok && err.contains("matches nothing on disk"), "{err}");
    assert_eq!(row_files(&d, "plan b"), ["src/b.rs"]);
}

#[test]
fn staged_rename_source_is_not_evidence() {
    let d = repo();
    let git = |args: &[&str]| {
        assert!(
            Command::new("git")
                .args(args)
                .current_dir(&d)
                .status()
                .unwrap()
                .success()
        );
    };
    std::fs::create_dir_all(d.join("xyzc")).unwrap();
    std::fs::write(d.join("xyzc/foo.rs"), "//\n").unwrap();
    git(&["add", "xyzc/foo.rs"]);
    git(&["commit", "-qm", "foo"]);
    std::fs::create_dir_all(d.join("lib")).unwrap();
    git(&["mv", "xyzc/foo.rs", "lib/foo.rs"]);
    // -z prints `R  lib/foo.rs\0src/foo.rs`; the source must not be read as
    // an entry of its own (`c/foo.rs` after chopping the status columns)
    let (ok, _, err) = fael(&d, &["add", "note", "chopped", "--files", "c/foo.rs"], "");
    assert!(ok && err.contains("matches nothing on disk"), "{err}");
}

#[test]
fn root_relative_files_from_a_subdir_do_not_double_the_prefix() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "//\n").unwrap();
    let sub = d.join("src");
    // from `src/`, `src/a.rs` reads as `src/src/a.rs`; `a.rs` must keep meaning src/a.rs
    let (ok, _, err) = fael(
        &sub,
        &["add", "note", "from sub", "--files", "src/a.rs"],
        "",
    );
    assert!(ok, "{err}");
    assert!(err.contains("resolved from repo root"), "{err}");
    assert_eq!(row_files(&d, "from sub"), ["src/a.rs"]);
    let (ok, _, err) = fael(&sub, &["add", "note", "cwd rel", "--files", "a.rs"], "");
    assert!(ok, "{err}");
    assert!(!err.contains("resolved from repo root"), "{err}");
    assert_eq!(row_files(&d, "cwd rel"), ["src/a.rs"]);
}
