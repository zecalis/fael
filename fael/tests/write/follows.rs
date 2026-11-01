//! `add --follows <id>` and `add … --then "<text>"`: a row that continues an
//! earlier one without replacing it. The closed-earlier recall stays silent for
//! that id, `find` shows "followed by", nothing is closed or hidden.

use super::{fael, repo};
use std::path::Path;

const LINE: &str = "closed earlier on these files";

fn add(d: &Path, kind: &str, text: &str, extra: &[&str]) -> (String, String) {
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    let mut args = vec!["add", kind, text, "--files", "src/a.rs"];
    args.extend(extra);
    let (ok, out, err) = fael(d, &args, "");
    assert!(ok, "{err}");
    (out.split_whitespace().next().unwrap().to_string(), err)
}

fn closed_issue(d: &Path) -> String {
    let (old, _) = add(d, "issue", "bench harness has no real provider", &[]);
    let (ok, _, err) = fael(
        d,
        &[
            "close",
            &old,
            "no provider in bench → measured 8 queries; guard `src/a.rs`",
        ],
        "",
    );
    assert!(ok, "{err}");
    old
}

#[test]
fn follows_silences_the_closed_earlier_line_for_that_id_only() {
    let d = repo();
    let old = closed_issue(&d);
    let (_, plain) = add(&d, "issue", "provider latency still unmeasured", &[]);
    assert!(plain.contains(LINE), "{plain}");
    let (_, err) = add(
        &d,
        "issue",
        "provider latency not measured yet",
        &["--follows", &old],
    );
    assert!(!err.contains(LINE), "{err}");
}

#[test]
fn find_shows_followed_by_and_the_old_row_stays_closed() {
    let d = repo();
    let old = closed_issue(&d);
    let (new, _) = add(
        &d,
        "issue",
        "provider latency still unmeasured",
        &["--follows", &old],
    );
    let (ok, out, err) = fael(&d, &["find", "--all", "--files", "src/a.rs"], "");
    assert!(ok, "{err}");
    let line = out.lines().find(|l| l.contains("bench harness")).unwrap();
    assert!(
        line.contains("(closed)") && line.contains("followed by"),
        "{out}"
    );
    assert!(line.contains(&new[..8]), "{out}");
}

#[test]
fn follows_an_unknown_id_is_rejected_and_nothing_is_written() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    let args = [
        "add",
        "note",
        "x",
        "--files",
        "src/a.rs",
        "--follows",
        "ZZZZZZ",
    ];
    let (ok, _, err) = fael(&d, &args, "");
    assert!(!ok && err.contains("no row with id"), "{err}");
}

#[test]
fn then_files_a_result_note_and_a_child_issue_that_follows_it() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    let args = [
        "add",
        "note",
        "8 queries, 3.7-5.5 s at 11k docs",
        "--files",
        "src/a.rs",
        "--then",
        "measure with the real provider",
    ];
    let (ok, out, err) = fael(&d, &args, "");
    assert!(ok, "{err}");
    let ids: Vec<&str> = out
        .lines()
        .map(|l| l.split_whitespace().next().unwrap())
        .collect();
    assert_eq!(ids.len(), 2, "{out}");
    let (_, open, _) = fael(&d, &["find", "--files", "src/a.rs"], "");
    let note = open.lines().find(|l| l.contains("8 queries")).unwrap();
    assert!(note.contains("note (followed by"), "{open}");
    let kid = open.lines().find(|l| l.contains("measure with")).unwrap();
    assert!(kid.contains(" issue "), "{open}");
}

#[test]
fn then_is_refused_with_dry_run() {
    let d = repo();
    let args = [
        "add",
        "note",
        "x",
        "--files",
        "src/a.rs",
        "--then",
        "y",
        "--dry-run",
    ];
    let (ok, _, err) = fael(&d, &args, "");
    assert!(!ok && err.contains("--then"), "{err}");
}
