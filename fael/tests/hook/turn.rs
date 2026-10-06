//! The edit ask speaks once per user turn (prompt to prompt): a task that
//! edits many files gets one "still true?" line, not one per edit. A row not
//! asked keeps its key, so the next turn's edit asks it.

use super::{fael, json, repo};
use std::path::Path;

fn prompt(d: &Path, session: &str) {
    let input = format!(
        r#"{{"cwd":{},"session_id":"{session}","prompt":"go on"}}"#,
        json(d)
    );
    let (ok, _, err) = fael(d, &["hook", "prompt", "--client", "claude"], &input);
    assert!(ok, "{err}");
}

/// The hook's context for an edit of `file`, empty when it was silent.
fn edit(d: &Path, session: &str, file: &str) -> String {
    let input = format!(
        r#"{{"cwd":{},"session_id":"{session}","tool_input":{{"file_path":{}}}}}"#,
        json(d),
        json(&d.join(file))
    );
    let (ok, out, err) = fael(d, &["hook", "edit", "--client", "claude"], &input);
    assert!(ok, "{err}");
    serde_json::from_str::<serde_json::Value>(&out)
        .ok()
        .and_then(|v| {
            v["hookSpecificOutput"]["additionalContext"]
                .as_str()
                .map(String::from)
        })
        .unwrap_or_default()
}

/// A decision on each of `files`, then every file changed after it.
fn stale_rows(d: &Path, files: &[&str]) {
    for f in files {
        std::fs::write(d.join(f), "// v1\n").unwrap();
        let (ok, _, err) = fael(
            d,
            &["add", "decision", &format!("rule for {f}"), "--files", f],
            "",
        );
        assert!(ok, "{err}");
        std::fs::write(d.join(f), "// v2\n").unwrap();
    }
}

#[test]
fn the_edit_ask_speaks_once_per_turn() {
    let d = repo();
    stale_rows(&d, &["src/a.rs", "src/b.rs", "src/c.rs"]);
    prompt(&d, "t1");
    assert!(edit(&d, "t1", "src/a.rs").contains("changed since"));
    // same turn: b's row is pushed, but its ask waits
    let b = edit(&d, "t1", "src/b.rs");
    assert!(
        b.contains("rule for src/b.rs") && !b.contains("changed since"),
        "{b}"
    );
    // the next turn asks the row the last one held back, once
    prompt(&d, "t1");
    assert!(edit(&d, "t1", "src/b.rs").contains("changed since"));
    assert!(!edit(&d, "t1", "src/c.rs").contains("changed since"));
}

/// A client with no prompt hook never marks a turn: every edit may ask, as before.
#[test]
fn no_prompt_hook_no_turn_limit() {
    let d = repo();
    stale_rows(&d, &["src/a.rs", "src/b.rs"]);
    assert!(edit(&d, "t2", "src/a.rs").contains("changed since"));
    assert!(edit(&d, "t2", "src/b.rs").contains("changed since"));
}
