//! Key suggestions (PLAN-fael-call-fit chunk k1), CLI side: a keyless add on
//! files that carry keys names at most three, newest first, once each. Silent
//! with `--key` or with no keyed row on a real shared file.
//! One key on the files is adopted by auto-key, so the line needs two or more.

use super::{fael, repo};
use std::path::Path;

const LINE: &str = "existing keys on these files:";

/// File a note on `src/a.rs` (optionally keyed); returns stderr.
fn note(d: &Path, text: &str, key: Option<&str>) -> String {
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    let mut args = vec!["add", "note", text, "--files", "src/a.rs"];
    if let Some(k) = key {
        args.extend(["--key", k]);
    }
    let (ok, _, err) = fael(d, &args, "");
    assert!(ok, "{err}");
    std::thread::sleep(std::time::Duration::from_millis(5)); // ids are ms-ordered
    err
}

fn line(err: &str) -> Option<&str> {
    err.lines().find(|l| l.contains(LINE))
}

#[test]
fn names_at_most_three_newest_first_once_each() {
    let d = repo();
    for k in ["k-one", "k-two", "k-two", "k-three", "k-four"] {
        note(&d, &format!("row for {k}"), Some(k));
    }
    let err = note(&d, "keyless row", None);
    let l = line(&err).unwrap_or_else(|| panic!("{err}"));
    assert!(
        l.ends_with("existing keys on these files: k-four, k-three, k-two"),
        "{l}"
    );
}

#[test]
fn silent_with_a_key_or_without_a_keyed_row() {
    let d = repo();
    // no keyed row yet
    assert!(line(&note(&d, "first", None)).is_none());
    note(&d, "a", Some("k-one"));
    note(&d, "b", Some("k-two"));
    // the caller named a key
    assert!(line(&note(&d, "keyed add", Some("k-one"))).is_none());
    // a different file shares no key
    std::fs::write(d.join("src/b.rs"), "// b\n").unwrap();
    let (ok, _, err) = fael(&d, &["add", "note", "elsewhere", "--files", "src/b.rs"], "");
    assert!(ok && line(&err).is_none(), "{err}");
}

#[test]
fn anchor_files_never_match() {
    let d = repo();
    for (t, k) in [("a", "k-one"), ("b", "k-two")] {
        let (ok, _, err) = fael(
            &d,
            &["add", "note", t, "--files", "doc:shared", "--key", k],
            "",
        );
        assert!(ok, "{err}");
    }
    let (ok, _, err) = fael(&d, &["add", "note", "keyless", "--files", "doc:shared"], "");
    assert!(ok && line(&err).is_none(), "{err}");
}

#[test]
fn glob_files_never_match() {
    let d = repo();
    for (t, k) in [("a", "k-one"), ("b", "k-two")] {
        let (ok, _, err) = fael(
            &d,
            &["add", "note", t, "--files", "src/*.rs", "--key", k],
            "",
        );
        assert!(ok, "{err}");
    }
    let (ok, _, err) = fael(&d, &["add", "note", "keyless", "--files", "src/*.rs"], "");
    assert!(ok && line(&err).is_none(), "{err}");
}
