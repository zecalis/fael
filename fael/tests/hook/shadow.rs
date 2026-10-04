//! PLAN-fael-file-hash chunk 3 (shadow): every read push records the
//! `changed` / `unchanged` verdict split on its usage line, while the
//! rendered context stays byte-identical (nothing new is displayed). An edit
//! push records none: the hook runs after the write, so the edited file would
//! always read as changed.

use super::{fael, json, repo, strip_fh};
use std::path::Path;

/// A push of `file` in a fresh session; returns the rendered context plus
/// the push's usage line (the `read`/`edit` event row, never `in-context`).
/// Reads and edits take the same `tool_input.file_path` shape (one file per
/// push — see `close_hint`); only the hint and the shadow differ by event.
fn push(d: &Path, event: &str, session: &str, file: &str) -> (String, serde_json::Value) {
    let input = format!(
        r#"{{"cwd":{},"session_id":"{session}","tool_input":{{"file_path":{}}}}}"#,
        json(d),
        json(&d.join(file))
    );
    let (ok, out, err) = fael(d, &["hook", event, "--client", "claude"], &input);
    assert!(ok, "{err}");
    let v: serde_json::Value = serde_json::from_str(&out).expect(&out);
    let context = v["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .expect(&out)
        .to_string();
    let usage = std::fs::read_to_string(super::state(d).join("usage.jsonl")).unwrap();
    let line = usage
        .lines()
        .map(|l| serde_json::from_str::<serde_json::Value>(l).expect(l))
        .find(|l| l["event"] == event && l["session"] == session)
        .expect(&usage);
    (context, line)
}

/// The full id `add` just filed (its stdout starts with it).
fn add(d: &Path, kind: &str, text: &str, files: &str) -> String {
    let (ok, out, err) = fael(d, &["add", kind, text, "--files", files], "");
    assert!(ok, "{err}");
    out.split_whitespace().next().unwrap().to_string()
}

/// Full ids of a usage array field.
fn ids(v: &serde_json::Value, key: &str) -> Vec<String> {
    v[key]
        .as_array()
        .unwrap_or_else(|| panic!("no {key} in {v}"))
        .iter()
        .map(|i| i.as_str().unwrap().to_string())
        .collect()
}

#[test]
fn read_push_marks_matching_files_unchanged_with_no_display() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// v1\n").unwrap();
    let id = add(&d, "decision", "retry uses backoff here", "src/a.rs");
    let (context, usage) = push(&d, "read", "s1", "src/a.rs");
    assert!(context.contains("retry uses backoff here"), "{context}");
    assert_eq!(ids(&usage, "ids"), vec![id.clone()], "{usage}");
    assert_eq!(ids(&usage, "changed"), Vec::<String>::new(), "{usage}");
    assert_eq!(ids(&usage, "unchanged"), vec![id], "{usage}");
    // shadow is usage-line only: no new word reaches the context
    for banned in ["changed", "unchanged"] {
        assert!(!context.contains(banned), "{banned} in:\n{context}");
    }
}

#[test]
fn read_push_marks_a_changed_file_changed_with_no_display() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// v1\n").unwrap();
    let id = add(&d, "decision", "retry uses backoff here", "src/a.rs");
    std::fs::write(d.join("src/a.rs"), "// v2 edited\n").unwrap();
    let (context, usage) = push(&d, "read", "s1", "src/a.rs");
    assert_eq!(ids(&usage, "changed"), vec![id], "{usage}");
    assert_eq!(ids(&usage, "unchanged"), Vec::<String>::new(), "{usage}");
    for banned in ["changed", "\"changed\"", "\"unchanged\"", "unchanged:"] {
        assert!(!context.contains(banned), "{banned} in:\n{context}");
    }
}

/// The hook runs after the write, so an edit's own bytes would make every
/// edited file read as changed: an edit push records the hint, not the split.
#[test]
fn edit_push_records_no_shadow() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// v1\n").unwrap();
    let id = add(&d, "decision", "retry uses backoff here", "src/a.rs");
    std::fs::write(d.join("src/a.rs"), "// v2 edited\n").unwrap();
    let (context, usage) = push(&d, "edit", "s1", "src/a.rs");
    assert_eq!(ids(&usage, "ids"), vec![id], "{usage}");
    assert!(usage.get("changed").is_none(), "{usage}");
    assert!(usage.get("unchanged").is_none(), "{usage}");
    assert!(context.contains("changed since"), "{context}");
}

#[test]
fn legacy_rows_land_in_neither_list() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// v1\n").unwrap();
    let id = add(&d, "decision", "old choice predates hashes", "src/a.rs");
    strip_fh(&d, "predates hashes");
    let (context, usage) = push(&d, "read", "s1", "src/a.rs");
    assert_eq!(ids(&usage, "ids"), vec![id], "{usage}");
    assert_eq!(ids(&usage, "changed"), Vec::<String>::new(), "{usage}");
    assert_eq!(ids(&usage, "unchanged"), Vec::<String>::new(), "{usage}");
    assert!(context.contains("old choice predates hashes"), "{context}");
}

#[test]
fn changed_and_unchanged_files_split_across_pushes() {
    // one file per push, so the split shows across two usage lines: the
    // edited file's row lands in `changed`, the untouched file's in
    // `unchanged` — each line carrying the other list empty
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// v1\n").unwrap();
    std::fs::write(d.join("src/b.rs"), "// v1\n").unwrap();
    let changed_id = add(&d, "decision", "first module choice", "src/a.rs");
    let same_id = add(&d, "decision", "second module choice", "src/b.rs");
    std::fs::write(d.join("src/a.rs"), "// v2 edited\n").unwrap();
    let (_, usage) = push(&d, "read", "s1", "src/a.rs");
    assert_eq!(ids(&usage, "changed"), vec![changed_id], "{usage}");
    assert_eq!(ids(&usage, "unchanged"), Vec::<String>::new(), "{usage}");
    let (_, usage) = push(&d, "read", "s2", "src/b.rs");
    assert_eq!(ids(&usage, "changed"), Vec::<String>::new(), "{usage}");
    assert_eq!(ids(&usage, "unchanged"), vec![same_id], "{usage}");
}

#[test]
fn changed_row_and_legacy_row_share_one_push() {
    // a stamped row and a pre-hash row on the same edited file: the stamped
    // row lands in `changed`, the legacy row rides `ids` but neither list
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// v1\n").unwrap();
    let changed_id = add(&d, "decision", "new module choice", "src/a.rs");
    let legacy_id = add(&d, "decision", "old choice predates hashes", "src/a.rs");
    strip_fh(&d, "predates hashes");
    std::fs::write(d.join("src/a.rs"), "// v2 edited\n").unwrap();
    let (context, usage) = push(&d, "read", "s1", "src/a.rs");
    let mut shown = ids(&usage, "ids");
    shown.sort();
    let mut want = vec![changed_id.clone(), legacy_id];
    want.sort();
    assert_eq!(shown, want, "{usage}");
    assert_eq!(ids(&usage, "changed"), vec![changed_id], "{usage}");
    assert_eq!(ids(&usage, "unchanged"), Vec::<String>::new(), "{usage}");
    for banned in ["\"changed\"", "\"unchanged\"", "unchanged:"] {
        assert!(!context.contains(banned), "{banned} in:\n{context}");
    }
}

#[test]
fn over_cap_files_have_no_verdict() {
    // the stamp side covers up to 16 MiB, but the push path never hashes
    // over 1 MiB (01M42CGE): the stamped row reads as unknown — neither
    // shadow list, legacy hint — never a false "changed"
    let d = repo();
    std::fs::write(d.join("src/big.bin"), vec![b'x'; 2 * 1024 * 1024]).unwrap();
    let id = add(&d, "decision", "big asset choice", "src/big.bin");
    let (context, usage) = push(&d, "read", "s1", "src/big.bin");
    assert!(context.contains("big asset choice"), "{context}");
    assert_eq!(ids(&usage, "ids"), vec![id], "{usage}");
    assert_eq!(ids(&usage, "changed"), Vec::<String>::new(), "{usage}");
    assert_eq!(ids(&usage, "unchanged"), Vec::<String>::new(), "{usage}");
    assert!(!context.contains("changed since"), "{context}");
}

#[test]
fn stats_still_counts_shadowed_pushes() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// v1\n").unwrap();
    let id = add(&d, "decision", "retry uses backoff here", "src/a.rs");
    let (_, usage) = push(&d, "read", "s1", "src/a.rs");
    assert!(usage.get("changed").is_some(), "{usage}"); // the line really carries the keys
    // the state dir is scratch (under temp), so temp repos are kept (01M3CRR6A)
    let (ok, out, err) = fael(&d, &["stats", "--json"], "");
    assert!(ok, "{err}");
    let v: serde_json::Value = serde_json::from_str(&out).expect(&out);
    // the `add` line and the shadowed read line, one event each
    assert_eq!(v["events"], 2, "{out}");
    assert_eq!(v["by_event"]["read"]["events"], 1, "{out}");
    assert_eq!(v["by_event"]["add"]["events"], 1, "{out}");
    assert_eq!(v["top_rows"][0]["id"], id.as_str(), "{out}");
    assert_eq!(v["top_rows"][0]["pushes"], 1, "{out}");
}
