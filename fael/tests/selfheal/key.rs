//! Chunk 3c: a caller-supplied key is the topic's identity — the single open
//! row with the same kind + key (any branch, same writer) supersedes itself;
//! another writer's row, or several matches, is kept and listed, never asked.

use super::{fael, mcp_add, names, repo, usage};
use std::path::Path;
use std::process::Command;

fn add(d: &Path, kind: &str, text: &str, files: &str, key: &str) -> (bool, String, String) {
    fael(d, &["add", kind, text, "--files", files, "--key", key], "")
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
    let (ok, _, err) = add(&d, "decision", "second", "src/a.rs", "auth:session");
    assert!(ok, "{err}");
    assert!(names(&err, "superseded ", &first), "{err}");
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
    let (ok, _, err) = fael(
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
    let (ok, _, err) = fael(
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
    let (is_err, text) = mcp_add(
        &d,
        serde_json::json!({"kind": "decision", "text": "second",
            "files": ["src/a.rs"], "key": "auth:session", "cwd": d}),
    );
    assert!(!is_err, "{text}");
    assert!(text.contains("superseded"), "{text}");
    assert_eq!(open_rows(&d).len(), 1);
    assert!(usage(&d).is_empty(), "self-heal info is no ask");
}
