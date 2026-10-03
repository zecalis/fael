//! The receipt `fael add` prints: the id, and what a supersede replaced.

use super::{fael, repo};

#[test]
fn add_says_what_it_superseded() {
    let d = repo();
    let (ok, out, err) = fael(
        &d,
        &["add", "decision", "pick sqlite", "--files", "doc:a"],
        "",
    );
    assert!(ok, "{err}");
    let old = out.split_whitespace().next().unwrap().to_string();
    assert!(!out.contains("supersedes"), "{out}");

    let args = [
        "add",
        "decision",
        "pick postgres",
        "--files",
        "doc:a",
        "--supersedes",
        &old,
    ];
    let (ok, out, err) = fael(&d, &args, "");
    assert!(ok, "{err}");
    assert!(
        out.trim_end().ends_with(&format!("· supersedes {old}")),
        "{out}"
    );
}
