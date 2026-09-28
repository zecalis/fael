//! Chunk 6e: seen-ids — an `add` inside a hook session marks its id seen,
//! and so does `find --files`. Split out of session.rs (file-size ratchet).

use super::{fael, fael_env, json, repo};

/// Chunk 6e: an `add` inside a hook session marks its id seen, and so does
/// `find --files` — the next read push of the same file stays silent, while a
/// fresh session still gets the row (seen is per session).
#[test]
fn add_and_find_mark_seen_for_the_session() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    std::fs::write(d.join("src/b.rs"), "// b\n").unwrap();
    // file one row as session s1: the id lands in s1's seen file
    let (ok, _, err) = fael_env(
        &d,
        &["add", "issue", "seen login loops", "--files", "src/a.rs"],
        "",
        &[("CLAUDE_CODE_SESSION_ID", "s1")],
    );
    assert!(ok, "{err}");
    // never pushed in s1, yet the read push stays silent — add marked it seen
    let fa = d.join("src/a.rs");
    let input = format!(
        r#"{{"cwd":{},"session_id":"s1","tool_input":{{"file_path":{}}}}}"#,
        json(&d),
        json(&fa)
    );
    let (ok, out, _) = fael(&d, &["hook", "read", "--client", "claude"], &input);
    assert!(ok && !out.contains("seen login loops"), "{out}");

    // the find half: a second row shown by `find --files` in s1 also skips push
    let (ok, _, err) = fael(
        &d,
        &["add", "issue", "found via find", "--files", "src/b.rs"],
        "",
    );
    assert!(ok, "{err}");
    let (ok, out, _) = fael_env(
        &d,
        &["find", "--files", "src/b.rs"],
        "",
        &[("CLAUDE_CODE_SESSION_ID", "s1")],
    );
    assert!(ok && out.contains("found via find"), "{out}");
    let fb = d.join("src/b.rs");
    let input = format!(
        r#"{{"cwd":{},"session_id":"s1","tool_input":{{"file_path":{}}}}}"#,
        json(&d),
        json(&fb)
    );
    let (ok, out, _) = fael(&d, &["hook", "read", "--client", "claude"], &input);
    assert!(ok && !out.contains("found via find"), "{out}");
    // a fresh session still gets it — seen is per session, not per row
    let input = format!(
        r#"{{"cwd":{},"session_id":"s2","tool_input":{{"file_path":{}}}}}"#,
        json(&d),
        json(&fb)
    );
    let (ok, out, _) = fael(&d, &["hook", "read", "--client", "claude"], &input);
    assert!(ok && out.contains("found via find"), "{out}");
}
