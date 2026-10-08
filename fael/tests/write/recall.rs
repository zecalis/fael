//! Closed-issue recall (PLAN-fael-context-loop chunk 5), CLI side: an issue
//! filed on a file that carries a closed one names it, with the `--supersedes`
//! command. At most two, only issues, silent when there is none.

use super::{fael, repo};
use std::path::Path;

const LINE: &str = "closed issue on these files";

fn issue(d: &Path, text: &str, extra: &[&str]) -> (String, String) {
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    let mut args = vec!["add", "issue", text, "--files", "src/a.rs"];
    args.extend(extra);
    let (ok, out, err) = fael(d, &args, "");
    assert!(ok, "{err}");
    (out.split_whitespace().next().unwrap().to_string(), err)
}

fn close(d: &Path, id: &str) {
    let (ok, _, err) = fael(d, &["close", id, "fixed"], "");
    assert!(ok, "{err}");
}

#[test]
fn a_closed_issue_on_the_file_is_named_with_the_command() {
    let d = repo();
    let (old, _) = issue(&d, "heic upload breaks in chrome", &[]);
    close(&d, &old);
    let (new, err) = issue(&d, "chrome rejects heic again", &[]);
    assert!(err.contains(LINE), "{err}");
    let after = |flag: &str| {
        let t = err.split(flag).nth(1).unwrap();
        t.split(|c: char| !c.is_alphanumeric())
            .next()
            .unwrap()
            .to_string()
    };
    // ids print at their shortest unique prefix
    assert!(old.starts_with(&after("--supersedes ")), "{err}");
    assert!(new.starts_with(&after("fael close ")), "{err}");
}

#[test]
fn no_closed_issue_is_silent() {
    let d = repo();
    let (_, err) = issue(&d, "first on this file", &[]);
    let (_, err2) = issue(&d, "second while the first is open", &[]);
    assert!(!err.contains(LINE) && !err2.contains(LINE), "{err}{err2}");
}

#[test]
fn a_linked_issue_is_silent() {
    let d = repo();
    let (old, _) = issue(&d, "heic upload breaks in chrome", &[]);
    close(&d, &old);
    let (_, err) = issue(&d, "chrome rejects heic again", &["--supersedes", &old]);
    assert!(!err.contains(LINE), "{err}");
}

#[test]
fn only_issues_count_and_only_two_are_named() {
    let d = repo();
    let mut closed = vec![];
    for t in ["bug one", "bug two", "bug three"] {
        let (id, _) = issue(&d, t, &[]);
        close(&d, &id);
        closed.push(id);
    }
    // a closed note on the same file is no issue to recall
    let (ok, out, err) = fael(&d, &["add", "note", "n", "--files", "src/a.rs"], "");
    assert!(ok, "{err}");
    close(&d, out.split_whitespace().next().unwrap());
    let (_, err) = issue(&d, "bug four", &[]);
    assert_eq!(err.matches(LINE).count(), 2, "{err}");
    // newest first: the oldest closed issue is the one left out
    let named: Vec<&str> = err
        .lines()
        .filter(|l| l.contains(LINE))
        .map(|l| l.split(LINE).nth(1).unwrap())
        .collect();
    let has = |id: &str| {
        let tok = |l: &&str| {
            l.trim_start_matches(": ")
                .split_whitespace()
                .next()
                .map(String::from)
        };
        named.iter().filter_map(tok).any(|t| id.starts_with(&t))
    };
    assert!(
        has(&closed[2]) && has(&closed[1]) && !has(&closed[0]),
        "{err}"
    );
}
