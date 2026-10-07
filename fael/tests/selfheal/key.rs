//! Chunk 3c: a caller-supplied key is the topic's identity — the single open
//! row with the same kind + key (any branch, same writer) supersedes itself;
//! another writer's row, or several matches, is kept and listed, never asked.

use super::{fael, fael_env, mcp_add_env, names, repo, usage};
use std::path::Path;
use std::process::Command;

fn add(d: &Path, kind: &str, text: &str, files: &str, key: &str) -> (bool, String, String) {
    fael(d, &["add", kind, text, "--files", files, "--key", key], "")
}

/// Self-heal holds a same-key match filed within `BURST_MS` (parallel adds
/// share a key, 01M47QEJ), so a test that wants the replace passes a zero
/// window to the child instead of sleeping out a real second.
const PAST_BURST: &[(&str, &str)] = &[("FAEL_BURST_MS", "0")];

fn add_past_burst(
    d: &Path,
    kind: &str,
    text: &str,
    files: &str,
    key: &str,
) -> (bool, String, String) {
    fael_env(
        d,
        &["add", kind, text, "--files", files, "--key", key],
        "",
        PAST_BURST,
    )
}

/// Full ids of the currently listed open rows.
fn open_rows(d: &Path) -> Vec<String> {
    let (ok, out, err) = fael(d, &["find", "--json"], "");
    assert!(ok, "{err}");
    out.lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter_map(|v| v["id"].as_str().map(String::from))
        .collect()
}

#[test]
fn single_key_match_supersedes_any_branch() {
    let d = repo();
    let (ok, out, err) = add(&d, "decision", "first", "src/a.rs", "auth:session");
    assert!(ok, "{err}");
    let first = out.split_whitespace().next().unwrap().to_string();
    // key is the identity: the branch a row sits on is not part of the match
    assert!(
        Command::new("git")
            .args(["checkout", "-qb", "feature"])
            .current_dir(&d)
            .status()
            .unwrap()
            .success()
    );
    let (ok, _, err) = add_past_burst(&d, "decision", "second", "src/a.rs", "auth:session");
    assert!(ok, "{err}");
    assert!(names(&err, "superseded ", &first), "{err}");
    // the line names what it replaced and the undo — a same key can be
    // another topic, and an id alone does not say so
    assert!(
        err.contains("— was \"first\"; wrong one? fael restore "),
        "{err}"
    );
    assert_eq!(open_rows(&d).len(), 1);
    assert!(usage(&d).is_empty(), "self-heal info is no ask");
}

#[test]
fn another_writers_key_row_is_listed_not_closed() {
    let d = repo();
    let (ok, _, err) = add(&d, "decision", "mine", "src/a.rs", "auth:session");
    assert!(ok, "{err}");
    assert!(
        Command::new("git")
            .args(["config", "user.name", "Someone Else"])
            .current_dir(&d)
            .status()
            .unwrap()
            .success()
    );
    let (ok, _, err) = add(&d, "decision", "theirs", "src/a.rs", "auth:session");
    assert!(ok, "{err}");
    assert!(!err.contains("superseded"), "{err}");
    assert!(
        err.contains("auth:session") && err.contains("another writer"),
        "{err}"
    );
    assert_eq!(open_rows(&d).len(), 2);
}

#[test]
fn several_key_matches_are_kept_and_listed() {
    let d = repo();
    // a decoy the second add supersedes, so its own heal is bypassed
    let (ok, out, err) = add(&d, "note", "decoy", "src/a.rs", "zz:decoy");
    assert!(ok, "{err}");
    let decoy = out.split_whitespace().next().unwrap().to_string();
    let (ok, _, err) = add(&d, "decision", "one", "src/a.rs", "auth:session");
    assert!(ok, "{err}");
    // a caller-given flag skips heal: two open rows now share kind + key
    let (ok, _, err) = fael(
        &d,
        &[
            "add",
            "decision",
            "two",
            "--files",
            "src/b.rs",
            "--key",
            "auth:session",
            "--supersedes",
            &decoy,
        ],
        "",
    );
    assert!(ok, "{err}");
    let (ok, _, err) = add(&d, "decision", "three", "src/b.rs", "auth:session");
    assert!(ok, "{err}");
    assert!(!err.contains("superseded"), "{err}");
    assert!(err.contains("already use key auth:session"), "{err}");
    assert_eq!(open_rows(&d).len(), 3, "nothing superseded");
    assert!(usage(&d).is_empty(), "the list is info, no ask");
}

#[test]
fn key_match_wins_and_files_note_is_kept() {
    let d = repo();
    let (ok, out, err) = fael(
        &d,
        &[
            "add",
            "note",
            "A",
            "--files",
            "src/a.rs",
            "--key",
            "auth:session",
        ],
        "",
    );
    assert!(ok, "{err}");
    let a = out.split_whitespace().next().unwrap().to_string();
    let (ok, out, err) = fael(&d, &["add", "note", "B", "--files", "src/b.rs"], "");
    assert!(ok, "{err}");
    let b = out.split_whitespace().next().unwrap().to_string();
    // the new note carries the key and spans both files: (c) picks A, (b) sees B
    let (ok, _, err) = fael_env(
        &d,
        &[
            "add",
            "note",
            "new",
            "--files",
            "src/a.rs,src/b.rs",
            "--key",
            "auth:session",
        ],
        "",
        PAST_BURST,
    );
    assert!(ok, "{err}");
    assert!(names(&err, "superseded ", &a), "{err}");
    assert!(
        names(&err, "note ", &b) && err.contains("also overlaps these files"),
        "{err}"
    );
    let open = open_rows(&d);
    assert_eq!(open.len(), 2);
    assert!(open.contains(&b) && !open.contains(&a), "{open:?}");
}

#[test]
fn key_and_files_same_row_supersedes_once() {
    let d = repo();
    let (ok, out, err) = fael(
        &d,
        &[
            "add",
            "note",
            "A",
            "--files",
            "src/a.rs",
            "--key",
            "auth:session",
        ],
        "",
    );
    assert!(ok, "{err}");
    let a = out.split_whitespace().next().unwrap().to_string();
    let (ok, _, err) = fael_env(
        &d,
        &[
            "add",
            "note",
            "new",
            "--files",
            "src/a.rs",
            "--key",
            "auth:session",
        ],
        "",
        PAST_BURST,
    );
    assert!(ok, "{err}");
    assert!(names(&err, "superseded ", &a), "{err}");
    assert!(!err.contains("also overlaps"), "{err}");
    assert_eq!(open_rows(&d).len(), 1);
}

#[test]
fn mcp_add_single_key_supersedes() {
    let d = repo();
    let (ok, _, err) = add(&d, "decision", "first", "src/a.rs", "auth:session");
    assert!(ok, "{err}");
    let (is_err, text) = mcp_add_env(
        &d,
        serde_json::json!({"kind": "decision", "text": "second",
            "files": ["src/a.rs"], "key": "auth:session", "cwd": d}),
        PAST_BURST,
    );
    assert!(!is_err, "{text}");
    assert!(text.contains("superseded"), "{text}");
    assert_eq!(open_rows(&d).len(), 1);
    assert!(usage(&d).is_empty(), "self-heal info is no ask");
}

/// An issue is a finding, not a topic: a chunk key can hold several, so a
/// different issue must not be swallowed by the key alone (issue 01M3KCJ5Y).
#[test]
fn distinct_issues_on_one_key_both_stay_open() {
    let d = repo();
    let (ok, out, err) = add(&d, "issue", "broken counter", "src/a.rs", "plan:x:chunk-1");
    assert!(ok, "{err}");
    let first = out.split_whitespace().next().unwrap().to_string();
    let (ok, _, err) = add(&d, "issue", "stale cache", "src/b.rs", "plan:x:chunk-1");
    assert!(ok, "{err}");
    assert!(!err.contains("superseded"), "{err}");
    assert!(
        err.contains("kept open") && err.contains("plan:x:chunk-1"),
        "{err}"
    );
    let open = open_rows(&d);
    assert_eq!(open.len(), 2, "{open:?}");
    assert!(open.contains(&first), "{open:?}");
    assert!(usage(&d).is_empty(), "the kept-open line is info, no ask");
}

/// Same key and same file but different words are still two findings.
#[test]
fn two_issues_same_key_same_file_different_words_both_stay() {
    let d = repo();
    let (ok, _, err) = add(&d, "issue", "broken counter", "src/a.rs", "plan:x:chunk-1");
    assert!(ok, "{err}");
    let (ok, _, err) = add(&d, "issue", "stale cache", "src/a.rs", "plan:x:chunk-1");
    assert!(ok, "{err}");
    assert!(!err.contains("superseded"), "{err}");
    assert_eq!(open_rows(&d).len(), 2);
}

/// The same finding re-filed (same words, a shared file) still supersedes.
#[test]
fn refiled_issue_with_the_same_words_and_a_shared_file_supersedes() {
    let d = repo();
    let (ok, out, err) = add(&d, "issue", "broken counter", "src/a.rs", "plan:x:chunk-1");
    assert!(ok, "{err}");
    let first = out.split_whitespace().next().unwrap().to_string();
    let (ok, _, err) = add_past_burst(
        &d,
        "issue",
        "broken counter",
        "src/a.rs,src/b.rs",
        "plan:x:chunk-1",
    );
    assert!(ok, "{err}");
    assert!(names(&err, "superseded ", &first), "{err}");
    assert_eq!(open_rows(&d).len(), 1);
}

/// Parallel adds share a key without replacing each other (issue 01M47QEJ):
/// two same-key rows filed in one burst both stay open and name the first.
#[test]
fn same_burst_key_match_holds_and_names_the_first() {
    let d = repo();
    let (ok, out, err) = add(&d, "decision", "first", "src/a.rs", "auth:session");
    assert!(ok, "{err}");
    let first = out.split_whitespace().next().unwrap().to_string();
    // no pause: one burst — the second files beside the first, never over it
    let (ok, _, err) = add(&d, "decision", "second", "src/a.rs", "auth:session");
    assert!(ok, "{err}");
    assert!(!err.contains("superseded"), "{err}");
    assert!(names(&err, "open decision ", &first), "{err}");
    assert!(err.contains("kept both"), "{err}");
    assert_eq!(open_rows(&d).len(), 2);
    assert!(usage(&d).is_empty(), "the kept-both line is info, no ask");
}
