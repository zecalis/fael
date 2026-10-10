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

/// A read of `file`: its push puts rows in the session's context.
fn read(d: &Path, session: &str, file: &str) {
    let input = format!(
        r#"{{"cwd":{},"session_id":"{session}","tool_input":{{"file_path":{}}}}}"#,
        json(d),
        json(&d.join(file))
    );
    let (ok, _, err) = fael(d, &["hook", "read", "--client", "claude"], &input);
    assert!(ok, "{err}");
}

/// A file with both a fixed bug and six decisions in context: the turn's one
/// ask goes to the carry-back (the repo's experience), the consolidate ask
/// waits for the next turn.
#[test]
fn the_carry_back_goes_before_the_merge_ask() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    for i in 0..6 {
        let text = format!("rule {i} of a");
        let (ok, _, err) = fael(&d, &["add", "decision", &text, "--files", "src/a.rs"], "");
        assert!(ok, "{err}");
    }
    let add = [
        "add",
        "issue",
        "retry loops",
        "--files",
        "src/a.rs",
        "--key",
        "a:retry",
    ];
    let (ok, _, err) = fael(&d, &add, "");
    assert!(ok, "{err}");
    let (ok, _, err) = fael(
        &d,
        &[
            "close",
            "--key",
            "a:retry",
            "no cap → cap at 3; fixed in (#7)",
        ],
        "",
    );
    assert!(ok, "{err}");
    // the fix on main: carry speaks only once a commit there names it
    super::git(&d, &["commit", "-q", "--allow-empty", "-m", "cap (#7)"]);
    // two reads put all six decisions in context (five per push)
    read(&d, "t4", "src/a.rs");
    read(&d, "t4", "src/a.rs");
    prompt(&d, "t4");
    let first = edit(&d, "t4", "src/a.rs");
    assert!(first.contains("broke before"), "{first}");
    assert!(!first.contains("one matter?"), "{first}");
    prompt(&d, "t4");
    let next = edit(&d, "t4", "src/a.rs");
    assert!(next.contains("one matter?"), "{next}");
}
