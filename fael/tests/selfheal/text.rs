//! Chunk 3d: the text says what the row supersedes — with no flag, an id
//! after a "supersede*" word; with a flag that resolves to nothing, any open
//! id in the text rescues it. Both ask nothing; the rescue only fires for
//! exactly one open row, so the genuinely unknown / ambiguous flag keeps the
//! original reject.

use super::{fael, mcp_add, names, open_notes, repo, usage};

/// The id `fael add` printed for the row it just wrote.
fn added(d: &std::path::Path, args: &[&str]) -> String {
    let (ok, out, err) = fael(d, args, "");
    assert!(ok, "{err}");
    out.split_whitespace().next().unwrap().to_string()
}

fn seed(d: &std::path::Path, text: &str, files: &str) -> String {
    added(d, &["add", "note", text, "--files", files])
}

#[test]
fn text_without_the_flag_sets_it() {
    let d = repo();
    let first = seed(&d, "first pass", "src/a.rs");
    let (ok, _, err) = fael(
        &d,
        &[
            "add",
            "note",
            &format!("second pass. Supersedes {first}"),
            "--files",
            "src/b.rs",
        ],
        "",
    );
    assert!(ok, "{err}");
    assert!(names(&err, "superseded ", &first), "{err}");
    assert!(err.contains("id in the text"), "{err}");
    assert_eq!(open_notes(&d).len(), 1);
    assert!(usage(&d).is_empty(), "self-heal info is no ask");
}

#[test]
fn an_id_merely_mentioned_is_not_a_target() {
    let d = repo();
    let first = seed(&d, "first pass", "src/a.rs");
    let (ok, _, err) = fael(
        &d,
        &[
            "add",
            "note",
            &format!("context lives in {first}, untouched"),
            "--files",
            "src/b.rs",
        ],
        "",
    );
    assert!(ok, "{err}");
    assert!(!err.contains("superseded"), "{err}");
    assert_eq!(open_notes(&d).len(), 2, "the id needs the word supersede");
}

/// The 2026-10-01 incident: "supersedes" in one sentence armed every id that
/// followed, so an id cited later as evidence was superseded. Only ids right
/// after the word are targets.
#[test]
fn an_id_cited_after_other_words_is_not_a_target() {
    let d = repo();
    let first = seed(&d, "first pass", "src/a.rs");
    let (ok, _, err) = fael(
        &d,
        &[
            "add",
            "note",
            &format!("a parallel note supersedes the handoff. Seen in {first}"),
            "--files",
            "src/b.rs",
        ],
        "",
    );
    assert!(ok, "{err}");
    assert!(!err.contains("superseded"), "{err}");
    assert_eq!(open_notes(&d).len(), 2, "a cited id stays open");
}

#[test]
fn two_named_rows_file_and_list_without_asking() {
    let d = repo();
    let a = seed(&d, "about a", "src/a.rs");
    let b = seed(&d, "about b", "src/b.rs");
    let (ok, _, err) = fael(
        &d,
        &[
            "add",
            "note",
            &format!("merging both. Supersedes {a} and {b}"),
            "--files",
            "src/a.rs,src/b.rs",
        ],
        "",
    );
    assert!(ok, "{err}");
    assert!(err.contains("are named in the text"), "{err}");
    assert_eq!(
        open_notes(&d).len(),
        3,
        "supersedes takes one row, so none went"
    );
    assert!(usage(&d).is_empty(), "the list is info, no ask");
}

#[test]
fn an_id_of_a_closed_row_supersedes_nothing() {
    let d = repo();
    let first = seed(&d, "first pass", "src/a.rs");
    let (ok, _, err) = fael(&d, &["close", &first, "done"], "");
    assert!(ok, "{err}");
    let (ok, out, err) = fael(
        &d,
        &[
            "add",
            "note",
            &format!("later. Supersedes {first}"),
            "--files",
            "src/b.rs",
        ],
        "",
    );
    assert!(ok, "{err}");
    assert!(!err.contains("superseded"), "{err}");
    let (ok, rows, err) = fael(&d, &["find", "--json", "--all"], "");
    assert!(ok, "{err}");
    let newest = rows
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .find(|v| v["id"].as_str() == Some(out.split_whitespace().next().unwrap()))
        .unwrap();
    assert!(newest["supersedes"].is_null(), "{newest}");
}

/// The broken-flag half: `--supersedes` the log cannot resolve, so the text
/// is all that is left to say what to replace. Exactly one open row → used,
/// with the reason in one info line and no reject on the books.
#[test]
fn a_broken_flag_is_rescued_by_the_text() {
    let d = repo();
    let first = seed(&d, "first pass", "src/a.rs");
    let (ok, _, err) = fael(
        &d,
        &[
            "add",
            "note",
            &format!("retry. Supersedes {first}"),
            "--files",
            "src/b.rs",
            "--supersedes",
            "nope-no-row",
        ],
        "",
    );
    assert!(ok, "{err}");
    assert!(err.contains("matched nothing"), "{err}");
    assert!(names(&err, "used ", &first), "{err}");
    assert!(err.contains("from the text"), "{err}");
    assert_eq!(open_notes(&d).len(), 1);
    assert!(usage(&d).is_empty(), "the rescue replaced the reject");
}

/// 0 candidates: the flag named nothing and the text says nothing either —
/// the original reject stands, here and in the replay's R6.
#[test]
fn a_broken_flag_with_no_text_target_still_rejects() {
    let d = repo();
    let _ = seed(&d, "first pass", "src/a.rs");
    let (ok, _, err) = fael(
        &d,
        &[
            "add",
            "note",
            "retry with no reference",
            "--files",
            "src/b.rs",
            "--supersedes",
            "nope-no-row",
        ],
        "",
    );
    assert!(!ok && err.contains("rejected: no row with id"), "{err}");
    let u = usage(&d);
    assert_eq!(u.len(), 1, "{u:?}");
    assert_eq!(u[0]["ask"], "reject", "{u:?}");
}

/// The rescue honors §6d's merely-mentioned rule: a broken flag plus a text
/// that names no row after a "supersede" word stays a reject, so a passing
/// reference is never closed just because the flag was wrong.
#[test]
fn a_broken_flag_with_a_mention_but_no_word_still_rejects() {
    let d = repo();
    let first = seed(&d, "first pass", "src/a.rs");
    let (ok, _, err) = fael(
        &d,
        &[
            "add",
            "note",
            &format!("context lives in {first}, retry"),
            "--files",
            "src/b.rs",
            "--supersedes",
            "nope-no-row",
        ],
        "",
    );
    assert!(!ok && err.contains("rejected: no row with id"), "{err}");
    assert_eq!(open_notes(&d), vec![first], "the mention did not close it");
    let u = usage(&d);
    assert_eq!(u.len(), 1, "{u:?}");
    assert_eq!(u[0]["ask"], "reject", "{u:?}");
}

/// More than one candidate: which row did the agent mean? It has to say, so
/// the original reject stands rather than fael picking one.
#[test]
fn a_broken_flag_with_two_text_targets_still_rejects() {
    let d = repo();
    let a = seed(&d, "about a", "src/a.rs");
    let b = seed(&d, "about b", "src/b.rs");
    let (ok, _, err) = fael(
        &d,
        &[
            "add",
            "note",
            &format!("retry. Supersedes {a} and {b}"),
            "--files",
            "src/b.rs",
            "--supersedes",
            "nope-no-row",
        ],
        "",
    );
    assert!(!ok && err.contains("rejected: no row with id"), "{err}");
    assert_eq!(usage(&d).len(), 1);
}

#[test]
fn mcp_add_reads_the_text_too() {
    let d = repo();
    let first = seed(&d, "first pass", "src/a.rs");
    let (is_err, text) = mcp_add(
        &d,
        serde_json::json!({"kind": "note", "text": format!("second. Supersedes {first}"),
            "files": ["src/b.rs"], "cwd": d}),
    );
    assert!(!is_err, "{text}");
    assert!(text.contains("superseded"), "{text}");
    assert_eq!(open_notes(&d).len(), 1);
    assert!(usage(&d).is_empty(), "self-heal info is no ask");
}
