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

/// A claude read of `src/a.rs` in session s1, `extra` spliced into the event.
fn read_a(d: &std::path::Path, extra: &str) -> String {
    let input = format!(
        r#"{{"cwd":{},"session_id":"s1",{extra}"tool_input":{{"file_path":{}}}}}"#,
        json(d),
        json(&d.join("src/a.rs"))
    );
    let (ok, out, err) = fael(d, &["hook", "read", "--client", "claude"], &input);
    assert!(ok, "{err}");
    out
}

/// A row filed before the session's first edit stays seen: `add` only knows
/// `$CLAUDE_CODE_SESSION_ID`, the hook keys by the transcript path whose stem
/// is that id — no edit file exists yet to bridge the two.
#[test]
fn own_row_is_seen_before_the_sessions_first_edit() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    let (ok, _, err) = fael_env(
        &d,
        &["add", "issue", "filed while reading", "--files", "src/a.rs"],
        "",
        &[("CLAUDE_CODE_SESSION_ID", "s1")],
    );
    assert!(ok, "{err}");
    let out = read_a(&d, r#""transcript_path":"/tmp/t/s1.jsonl","#);
    assert!(!out.contains("filed while reading"), "{out}");
}

/// Reads fired in one batch race on the seen list: every row is pushed once.
#[test]
fn parallel_reads_push_a_row_once() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    let (ok, _, err) = fael(
        &d,
        &["add", "issue", "raced row", "--files", "src/a.rs"],
        "",
    );
    assert!(ok, "{err}");
    let reads: Vec<_> = (0..16)
        .map(|_| {
            let d = d.clone();
            std::thread::spawn(move || read_a(&d, ""))
        })
        .collect();
    let pushed = reads
        .into_iter()
        .map(|t| t.join().unwrap())
        .filter(|out| out.contains("raced row"))
        .count();
    assert_eq!(pushed, 1);
}

/// Seen is per context window, not per session: a sub-agent starts empty, so
/// it gets the row its parent was already told — once each.
#[test]
fn a_subagent_keeps_its_own_seen_list() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    let (ok, _, err) = fael(
        &d,
        &["add", "issue", "ctx login loops", "--files", "src/a.rs"],
        "",
    );
    assert!(ok, "{err}");
    assert!(read_a(&d, "").contains("ctx login loops"));
    assert!(!read_a(&d, "").contains("ctx login loops"));
    let a1 = r#""agent_id":"a1","agent_type":"Explore","#;
    assert!(read_a(&d, a1).contains("ctx login loops"));
    assert!(!read_a(&d, a1).contains("ctx login loops"));
    assert!(read_a(&d, r#""agent_id":"a2","#).contains("ctx login loops"));
    // the sub-agents never spent the parent's own list
    assert!(!read_a(&d, "").contains("ctx login loops"));
}

/// A compacted context lost the pushed rows: session-start with
/// `source: compact` starts the seen list over; a resume keeps it.
#[test]
fn compaction_starts_the_seen_list_over() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    // a decision: session-start lists open issues itself, which is not the push
    let (ok, _, err) = fael(
        &d,
        &["add", "decision", "ctx tenant keys", "--files", "src/a.rs"],
        "",
    );
    assert!(ok, "{err}");
    assert!(read_a(&d, "").contains("ctx tenant keys"));
    let start = |source: &str| {
        let input = format!(
            r#"{{"cwd":{},"session_id":"s1","source":"{source}"}}"#,
            json(&d)
        );
        let (ok, _, err) = fael(&d, &["hook", "session-start", "--client", "claude"], &input);
        assert!(ok, "{err}");
    };
    start("resume");
    assert!(!read_a(&d, "").contains("ctx tenant keys"));
    start("compact");
    assert!(read_a(&d, "").contains("ctx tenant keys"));
}
