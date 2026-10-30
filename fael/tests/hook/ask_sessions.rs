//! The edit ask across sessions (src/hook/asked.rs): said once until the row
//! changes. Every real edit moves the file's bytes, so bytes never decide.

use super::changed_hint::{add, edit_or_silent};
use super::{fael, repo};

fn asked(out: &str) -> bool {
    out.contains("changed since")
}

/// Asked in A; silent in B though the bytes moved again; a bump makes C ask.
#[test]
fn ask_is_said_once_until_the_row_is_bumped() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// v1\n").unwrap();
    let id = add(&d, "issue", "retry uses backoff here", "src/a.rs");
    std::fs::write(d.join("src/a.rs"), "// v2\n").unwrap();
    let a = edit_or_silent(&d, "sA", "src/a.rs");
    assert!(asked(&a), "{a}");

    std::fs::write(d.join("src/a.rs"), "// v3 longer\n").unwrap();
    let b = edit_or_silent(&d, "sB", "src/a.rs");
    assert!(!asked(&b) && !b.contains("fael close"), "{b}");

    let (ok, _, err) = fael(&d, &["bump", &id, "--to", "owner"], "");
    assert!(ok, "{err}");
    std::fs::write(d.join("src/a.rs"), "// v4 longer still\n").unwrap();
    let c = edit_or_silent(&d, "sC", "src/a.rs");
    assert!(asked(&c), "{c}");

    // and once more silent: the bump was asked about
    std::fs::write(d.join("src/a.rs"), "// v5\n").unwrap();
    let e = edit_or_silent(&d, "sD", "src/a.rs");
    assert!(!asked(&e), "{e}");
}

/// A Codex shell row and its later edit use one identity despite the rollout
/// filename: the chat is never asked to revalidate the issue it just filed.
#[test]
fn codex_edit_does_not_ask_about_its_own_shell_row() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// before\n").unwrap();
    let base = serde_json::json!({"cwd":d,"session_id":"chat-a",
        "transcript_path":d.join("rollout-2026-10-31T04-54-13-chat-a.jsonl"),"source":"startup"});
    assert!(
        fael(
            &d,
            &["hook", "session-start", "--client", "codex"],
            &base.to_string()
        )
        .0
    );
    let (ok, _, err) = super::fael_env(
        &d,
        &["add", "issue", "my current issue", "--files", "src/a.rs"],
        "",
        &[("CODEX_THREAD_ID", "chat-a")],
    );
    assert!(ok, "{err}");
    std::fs::write(d.join("src/a.rs"), "// after fix\n").unwrap();
    let mut edit = base;
    edit["tool_input"] = serde_json::json!({"command":"*** Update File: src/a.rs"});
    let (ok, out, err) = fael(
        &d,
        &["hook", "edit", "--client", "codex"],
        &edit.to_string(),
    );
    assert!(ok && !out.contains("changed since"), "{out}{err}");
}
