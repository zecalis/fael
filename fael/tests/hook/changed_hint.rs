//! PLAN-fael-file-hash chunk 2: the edit hint names the tier-0 rows whose
//! files changed since the row was written, earns no hint when every such
//! file still matches, and keeps the legacy hint for rows with no verdict.

use super::{fael, json, repo, strip_fh};
use std::path::Path;

/// An edit of `file` in a fresh session; returns the hook's stdout.
fn edit(d: &Path, session: &str, file: &str) -> String {
    let input = format!(
        r#"{{"cwd":{},"session_id":"{session}","tool_input":{{"file_path":{}}}}}"#,
        json(d),
        json(&d.join(file))
    );
    let (ok, out, err) = fael(d, &["hook", "edit", "--client", "claude"], &input);
    assert!(ok, "{err}");
    let v: serde_json::Value = serde_json::from_str(&out).expect(&out);
    v["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .expect(&out)
        .to_string()
}

/// The full id `add` just filed (its stdout starts with it).
fn add(d: &Path, kind: &str, text: &str, files: &str) -> String {
    let (ok, out, err) = fael(d, &["add", kind, text, "--files", files], "");
    assert!(ok, "{err}");
    out.split_whitespace().next().unwrap().to_string()
}

/// Short ids the hint named (`changed since <short> was written`).
fn named(out: &str) -> Vec<&str> {
    out.lines()
        .filter(|l| l.contains("changed since"))
        .map(|l| {
            let l = l.split("changed since ").nth(1).unwrap();
            l.split(" was written").next().unwrap()
        })
        .collect()
}

#[test]
fn unchanged_files_earn_no_hint() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// v1\n").unwrap();
    add(&d, "decision", "retry uses backoff here", "src/a.rs");
    add(&d, "issue", "login loops here", "src/a.rs");
    let out = edit(&d, "s1", "src/a.rs");
    assert!(out.contains("retry uses backoff here"), "{out}");
    assert!(out.contains("login loops here"), "{out}");
    for banned in [
        "changed since",
        "fael close",
        "fael bump",
        "supersedes",
        "a row above",
    ] {
        assert!(!out.contains(banned), "{banned} in:\n{out}");
    }
}

#[test]
fn changed_file_names_the_row_with_retire_commands() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// v1\n").unwrap();
    let id = add(&d, "decision", "retry uses backoff here", "src/a.rs");
    std::fs::write(d.join("src/a.rs"), "// v2 changed\n").unwrap();
    let out = edit(&d, "s1", "src/a.rs");
    let hint = out
        .lines()
        .find(|l| l.contains("changed since"))
        .expect(&out);
    let short = hint.split("changed since ").nth(1).unwrap();
    let short = short.split(" was written").next().unwrap();
    assert!(id.starts_with(short), "{hint}\n{id}");
    for cmd in [
        format!("fael bump {short}"),
        format!("--supersedes {short}"),
        format!("fael close {short}"),
    ] {
        assert!(hint.contains(&cmd), "{cmd} missing in:\n{hint}");
    }
}

#[test]
fn changed_hint_names_at_most_two() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// v1\n").unwrap();
    let ids = [
        add(&d, "decision", "first module choice", "src/a.rs"),
        add(&d, "decision", "second module choice", "src/a.rs"),
        add(&d, "decision", "third module choice", "src/a.rs"),
    ];
    std::fs::write(d.join("src/a.rs"), "// v2 changed\n").unwrap();
    let out = edit(&d, "s1", "src/a.rs");
    let shorts = named(&out);
    assert_eq!(shorts.len(), 2, "{out}");
    // exactly two of the three rows are named (a short names the id it prefixes)
    let hit: Vec<bool> = ids
        .iter()
        .map(|id| shorts.iter().any(|s| id.starts_with(s)))
        .collect();
    assert_eq!(hit.iter().filter(|h| **h).count(), 2, "{shorts:?}\n{out}");
}

#[test]
fn issue_without_fh_keeps_the_named_close() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// v1\n").unwrap();
    add(&d, "issue", "old problem predates hashes", "src/a.rs");
    strip_fh(&d, "predates hashes");
    let out = edit(&d, "s1", "src/a.rs");
    assert!(!out.contains("changed since"), "{out}");
    assert!(out.contains("done with one?"), "{out}");
}

#[test]
fn decision_without_fh_keeps_the_generic_hint() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// v1\n").unwrap();
    add(&d, "decision", "old choice predates hashes", "src/a.rs");
    strip_fh(&d, "predates hashes");
    let out = edit(&d, "s1", "src/a.rs");
    assert!(!out.contains("changed since"), "{out}");
    assert!(
        out.contains("a row above the code now says or contradicts?"),
        "{out}"
    );
}

/// A row filed under the old path reads the bytes at the new one: renamed
/// but untouched earns no hint, edited after the rename names the row.
#[test]
fn rename_resolves_to_the_new_bytes() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// v1\n").unwrap();
    let id = add(&d, "decision", "moved module choice", "src/a.rs");
    std::fs::rename(d.join("src/a.rs"), d.join("src/b.rs")).unwrap();
    let (ok, _, err) = fael(&d, &["mv", "src/a.rs", "src/b.rs"], "");
    assert!(ok, "{err}");
    let out = edit(&d, "s1", "src/b.rs");
    assert!(out.contains("moved module choice"), "{out}");
    assert!(!out.contains("changed since"), "{out}");
    std::fs::write(d.join("src/b.rs"), "// v2 changed\n").unwrap();
    let out = edit(&d, "s2", "src/b.rs");
    let shorts = named(&out);
    assert_eq!(shorts.len(), 1, "{out}");
    assert!(id.starts_with(shorts[0]), "{out}\n{id}");
}
