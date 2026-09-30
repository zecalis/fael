//! Stop-hook adoption: a log in the clone's journal counts, not only one in the
//! worktree's own `.fael/log` — a `store = "local"` repo keeps no rows in the tree,
//! and a session or sub-agent in a fresh worktree starts without a `.fael/`.

use super::{fael, git, json, repo};
use std::path::Path;

fn stop(d: &Path, extra: &str) -> String {
    let input = format!(r#"{{"cwd":{},"session":"s1",{extra}}}"#, json(d));
    let (ok, out, err) = fael(d, &["hook", "stop"], &input);
    assert!(ok, "{err}");
    out
}

const REPLY: &str = r#""reply":"done\n\nfael issue: Retry loop has no backoff [files: src/a.rs]""#;

fn seed(d: &Path) {
    std::fs::write(d.join("src/a.rs"), "//\n").unwrap();
    let (ok, _, err) = fael(d, &["add", "note", "seed row", "--files", "src/a.rs"], "");
    assert!(ok, "{err}");
}

/// `store = "local"`: rows live in the journal only, yet a reply line is filed.
#[test]
fn a_journal_only_repo_files_reply_lines() {
    let d = repo();
    std::fs::create_dir_all(d.join(".fael")).unwrap();
    std::fs::write(d.join(".fael/config.toml"), "store = \"local\"\n").unwrap();
    seed(&d);
    assert!(!d.join(".fael/log").exists(), "the tree holds no log");
    stop(&d, REPLY);
    assert!(fael(&d, &["find", "backoff"], "").1.contains("Retry loop"));
}

/// A sub-agent in a fresh worktree (no `.fael/`, as `isolation: worktree` makes
/// one) stops with the worktree as `cwd`: the clone's journal still says adopted.
#[test]
fn a_fresh_worktree_files_a_subagent_reply_line() {
    let d = repo();
    seed(&d);
    let wt = d.with_file_name(format!("{}-wt", d.file_name().unwrap().to_string_lossy()));
    git(
        &d,
        &["worktree", "add", "-q", wt.to_str().unwrap(), "-b", "wt"],
    );
    assert!(!wt.join(".fael").exists());
    stop(&wt, &format!(r#""agent":"a1",{REPLY}"#));
    assert!(fael(&d, &["find", "backoff"], "").1.contains("Retry loop"));
}

/// A repo fael never touched stays untouched: no log anywhere, nothing filed.
#[test]
fn a_repo_without_any_log_files_nothing() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "//\n").unwrap();
    stop(&d, REPLY);
    assert!(!d.join(".fael/log").exists() && !d.join(".git/fael").exists());
    assert!(fael(&d, &["find", "backoff"], "").1.is_empty());
}
