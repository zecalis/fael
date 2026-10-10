//! PLAN-fael-file-hash chunk 3 (shadow): the `changed` / `unchanged` verdict
//! split was recorded on a read push's usage line. Reads push nothing now, and
//! an edit push records none: the hook runs after the write, so the edited
//! file would always read as changed.

use super::{fael, json, repo};
use std::path::Path;

/// A push of `file` in a fresh session; returns the rendered context plus
/// the push's usage line (the `read`/`edit` event row, never `in-context`).
/// An edit takes the `tool_input.file_path` shape (one file per push — see
/// `close_hint`).
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

/// The hook runs after the write, so an edit's own bytes would make every
/// edited file read as changed: an edit push records the hint, not the split.
#[test]
fn edit_push_records_no_shadow() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// v1\n").unwrap();
    let id = add(&d, "issue", "retry uses backoff here", "src/a.rs");
    std::fs::write(d.join("src/a.rs"), "// v2 edited\n").unwrap();
    let (context, usage) = push(&d, "edit", "s1", "src/a.rs");
    assert_eq!(ids(&usage, "ids"), vec![id], "{usage}");
    assert!(usage.get("changed").is_none(), "{usage}");
    assert!(usage.get("unchanged").is_none(), "{usage}");
    assert!(context.contains("changed since"), "{context}");
}
