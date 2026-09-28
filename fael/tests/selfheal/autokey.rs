//! Chunk 3e: no `--key` and exactly one key on the row's files → that key,
//! said in one info line. Zero or several candidates → the row is filed
//! keyless without asking, and a key fael guessed never reaches (c), so it
//! can't close a row the caller never named.

use super::{fael, mcp_add, repo, usage};
use std::path::Path;

/// The `key` field of the open row whose text is `text`.
fn key_of(d: &Path, text: &str) -> Option<String> {
    let (ok, out, err) = fael(d, &["find", "--json"], "");
    assert!(ok, "{err}");
    out.lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .find(|v| v["text"].as_str() == Some(text))
        .and_then(|v| v["key"].as_str().map(String::from))
}

/// Open rows, any kind.
fn open_rows(d: &Path) -> Vec<serde_json::Value> {
    let (ok, out, err) = fael(d, &["find", "--json"], "");
    assert!(ok, "{err}");
    out.lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .collect()
}

fn seed_key(d: &Path, key: &str) {
    let (ok, _, err) = fael(
        d,
        &[
            "add", "decision", "seed", "--files", "src/a.rs", "--key", key,
        ],
        "",
    );
    assert!(ok, "{err}");
}

#[test]
fn the_only_key_on_the_files_is_adopted() {
    let d = repo();
    seed_key(&d, "auth:session");
    let (ok, _, err) = fael(&d, &["add", "note", "follow-up", "--files", "src/a.rs"], "");
    assert!(ok, "{err}");
    assert!(err.contains("key auth:session — the only key"), "{err}");
    assert_eq!(key_of(&d, "follow-up").as_deref(), Some("auth:session"));
    assert!(usage(&d).is_empty(), "self-heal info is no ask");
}

#[test]
fn no_key_on_these_files_means_no_key() {
    let d = repo();
    let (ok, _, err) = fael(&d, &["add", "note", "solo", "--files", "src/a.rs"], "");
    assert!(ok, "{err}");
    assert!(!err.contains("the only key"), "{err}");
    assert_eq!(key_of(&d, "solo"), None);
    assert!(usage(&d).is_empty());
}

#[test]
fn several_keys_choose_nothing_and_ask_nothing() {
    let d = repo();
    seed_key(&d, "auth:session");
    let (ok, _, err) = fael(
        &d,
        &[
            "add",
            "decision",
            "other",
            "--files",
            "src/a.rs",
            "--key",
            "db:ledger",
        ],
        "",
    );
    assert!(ok, "{err}");
    let (ok, _, err) = fael(&d, &["add", "note", "follow-up", "--files", "src/a.rs"], "");
    assert!(ok, "{err}");
    assert!(!err.contains("the only key"), "{err}");
    assert_eq!(key_of(&d, "follow-up"), None, "two candidates are a choice");
    assert!(usage(&d).is_empty(), "neither candidate nor row is an ask");
}

#[test]
fn a_key_the_caller_wrote_is_never_overwritten() {
    let d = repo();
    seed_key(&d, "auth:session");
    let (ok, _, err) = fael(
        &d,
        &[
            "add",
            "decision",
            "mine",
            "--files",
            "src/a.rs",
            "--key",
            "db:ledger",
        ],
        "",
    );
    assert!(ok, "{err}");
    assert!(!err.contains("the only key"), "{err}");
    assert_eq!(key_of(&d, "mine").as_deref(), Some("db:ledger"));
    assert_eq!(open_rows(&d).len(), 2, "neither row supersedes the other");
}

/// The rule that keeps (e) from feeding (c): the key is written after `heal`,
/// so two open decisions can end up sharing kind + key without either
/// superseding the other. Closing rows stays the caller's business.
#[test]
fn a_guessed_key_never_closes_a_row() {
    let d = repo();
    seed_key(&d, "auth:session");
    let (ok, _, err) = fael(
        &d,
        &["add", "decision", "second", "--files", "src/a.rs"],
        "",
    );
    assert!(ok, "{err}");
    assert!(err.contains("key auth:session — the only key"), "{err}");
    let open = open_rows(&d);
    assert_eq!(
        open.len(),
        2,
        "the guessed key supersedes nothing: {open:?}"
    );
    assert!(usage(&d).is_empty());
}

#[test]
fn mcp_add_auto_keys_too() {
    let d = repo();
    seed_key(&d, "auth:session");
    let (is_err, text) = mcp_add(
        &d,
        serde_json::json!({"kind": "note", "text": "follow-up",
            "files": ["src/a.rs"], "cwd": d}),
    );
    assert!(!is_err, "{text}");
    assert!(text.contains("key auth:session — the only key"), "{text}");
    assert_eq!(key_of(&d, "follow-up").as_deref(), Some("auth:session"));
    assert!(usage(&d).is_empty(), "self-heal info is no ask");
}
