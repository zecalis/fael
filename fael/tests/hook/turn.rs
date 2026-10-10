//! The edit ask names only open issues (issue hook:changed-line-noise), and an
//! open issue is asked past the once-per-turn limit (01M4BP2G): a turn that
//! edits several files with open issues asks each, once per session.

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

/// An open issue on each of `files`, then every file changed after it.
fn stale_rows(d: &Path, files: &[&str]) {
    for f in files {
        std::fs::write(d.join(f), "// v1\n").unwrap();
        let (ok, _, err) = fael(
            d,
            &["add", "issue", &format!("rule for {f}"), "--files", f],
            "",
        );
        assert!(ok, "{err}");
        std::fs::write(d.join(f), "// v2\n").unwrap();
    }
}

/// The vela miss: one file's ask spent the turn, so the edit of the file
/// with an open issue said nothing. An open issue is asked at its file's
/// edit whatever else the turn asked — still once per session.
#[test]
fn an_open_issue_is_asked_past_the_turn_limit() {
    let d = repo();
    stale_rows(&d, &["src/a.rs", "src/b.rs"]);
    prompt(&d, "t3");
    assert!(edit(&d, "t3", "src/a.rs").contains("changed since"));
    let b = edit(&d, "t3", "src/b.rs");
    assert!(b.contains("changed since"), "{b}");
    assert!(!edit(&d, "t3", "src/b.rs").contains("changed since"));
}
