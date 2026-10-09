//! The edit ask over a `from: user` row tells the agent the row is the user's
//! call — ask before re-filing or closing it. An agent's own row asks as before.

use super::{fael, json, repo};
use std::path::Path;

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

#[test]
fn the_edit_ask_names_the_users_call() {
    let d = repo();
    for (f, from) in [("src/a.rs", Some("user")), ("src/b.rs", None)] {
        std::fs::write(d.join(f), "// v1\n").unwrap();
        let mut args = vec!["add", "issue", "rule", "--files", f];
        args.extend(from.map(|u| ["--from", u]).into_iter().flatten());
        let (ok, _, err) = fael(&d, &args, "");
        assert!(ok, "{err}");
        std::fs::write(d.join(f), "// v2\n").unwrap();
    }
    let a = edit(&d, "u1", "src/a.rs");
    assert!(a.contains("was written (from user)"), "{a}");
    assert!(a.contains("the user's call: ask them before"), "{a}");
    let b = edit(&d, "u2", "src/b.rs");
    assert!(b.contains("changed since") && !b.contains("user"), "{b}");
}
