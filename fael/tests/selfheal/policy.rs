//! Chunk 2 (PLAN-fael-selfheal-verdict): the policy table's precedence as the
//! CLI shows it — a resolving `--supersedes` flag is Explicit and beats the
//! text, the first eligible class decides alone, and a hold files the row
//! without touching anything. The A/B shape below is the incident from the
//! plan: counting candidates across classes must not turn an explicit act
//! into a hold.

use super::{fael, names, open_notes, repo, usage};

/// The id `fael add` printed for the row it just wrote.
fn added(d: &std::path::Path, args: &[&str]) -> String {
    let (ok, out, err) = fael(d, args, "");
    assert!(ok, "{err}");
    out.split_whitespace().next().unwrap().to_string()
}

fn seed(d: &std::path::Path, text: &str, files: &str) -> String {
    added(d, &["add", "note", text, "--files", files])
}

/// A resolving flag is Explicit: the caller typed it, so the text naming a
/// different row cannot override it — the flag passes through silently.
#[test]
fn a_resolving_flag_beats_the_text() {
    let d = repo();
    let a = seed(&d, "about a", "src/a.rs");
    let b = seed(&d, "about b", "src/b.rs");
    let c = added(
        &d,
        &[
            "add",
            "note",
            &format!("merging. Supersedes {b}"),
            "--files",
            "src/a.rs",
            "--supersedes",
            &a,
        ],
    );
    let open = open_notes(&d);
    assert!(!open.contains(&a), "the flag's target went: {open:?}");
    assert!(open.contains(&b) && open.contains(&c), "{open:?}");
    assert!(usage(&d).is_empty(), "a passthrough asks nothing");
}

/// Explicit holds when it has two: the row is filed, nothing is superseded,
/// and the line says which `--supersedes <id>` to pass.
#[test]
fn two_named_rows_hold_and_touch_nothing() {
    let d = repo();
    let a = seed(&d, "about a", "src/a.rs");
    let b = seed(&d, "about b", "src/b.rs");
    let c = added(
        &d,
        &[
            "add",
            "note",
            &format!("merging both. Supersedes {a} and {b}"),
            "--files",
            "src/a.rs,src/b.rs",
        ],
    );
    assert_eq!(open_notes(&d).len(), 3, "filed, nothing superseded");
    let (ok, rows, err) = fael(&d, &["find", "--json", "--all"], "");
    assert!(ok, "{err}");
    let newest = rows
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .find(|v| v["id"].as_str() == Some(c.as_str()))
        .unwrap();
    assert!(newest["supersedes"].is_null(), "{newest}");
    assert!(usage(&d).is_empty(), "the hold is info, no ask");
}

/// The incident shape: A (decision, other key) named by the text acts, while
/// B (same key, overlapping note) is reported "also kept" — the lower class
/// never drags the explicit pick into a hold.
#[test]
fn explicit_act_reports_the_lower_class_as_also_kept() {
    let d = repo();
    let (ok, out, err) = fael(
        &d,
        &[
            "add",
            "decision",
            "first decision",
            "--files",
            "src/a.rs",
            "--key",
            "auth:x",
        ],
        "",
    );
    assert!(ok, "{err}");
    let a = out.split_whitespace().next().unwrap().to_string();
    let (ok, out, err) = fael(
        &d,
        &[
            "add",
            "note",
            "shared topic",
            "--files",
            "src/a.rs",
            "--key",
            "billing:invoice",
        ],
        "",
    );
    assert!(ok, "{err}");
    let b = out.split_whitespace().next().unwrap().to_string();
    let (ok, _, err) = fael(
        &d,
        &[
            "add",
            "note",
            &format!("shared topic. Supersedes {a}"),
            "--files",
            "src/a.rs",
            "--key",
            "billing:invoice",
        ],
        "",
    );
    assert!(ok, "{err}");
    assert!(names(&err, "superseded ", &a), "{err}");
    assert!(
        names(&err, "note ", &b) && err.contains("also overlaps these files"),
        "{err}"
    );
    assert!(usage(&d).is_empty(), "act plus also-kept is info, no ask");
}
