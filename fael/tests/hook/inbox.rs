//! The inbox line: notes left for the running chunk, said at session start.

use super::{fael, json, repo};

/// Several notes waiting at session start are one inbox line: one header and
/// one reply/show tail, a bullet per note — not a line (and tail) per note.
/// The notes come from a sibling worktree (the owner, a peer); one written in
/// the run's own worktree is its own checkpoint and stays silent (01M88ZGB).
#[test]
fn several_inbox_notes_share_one_tail() {
    let d = repo();
    let (ok, out, err) = fael(&d, &["chunk", "add", "T", "--brief", "b"], "");
    assert!(ok, "{err}");
    let uid = out.trim().to_string();
    let (ok, _, err) = fael(&d, &["chunk", "start", &uid, "--run", "R1"], "");
    assert!(ok, "{err}");
    let wt = super::sibling(&d);
    let (ok, _, err) = super::fael_env(
        &d,
        &["chunk", "note", &uid, "own checkpoint"],
        "",
        &[("FAEL_SESSION", "other")],
    );
    assert!(ok, "{err}");
    for text in ["first word", "second word", "third word"] {
        let (ok, _, err) = super::fael_env(
            &wt,
            &["chunk", "note", &uid, text],
            "",
            &[("FAEL_SESSION", "other")],
        );
        assert!(ok, "{err}");
    }
    let input = format!(
        r#"{{"cwd":{},"session_id":"s1","source":"startup"}}"#,
        json(&d)
    );
    let (ok, out, err) = fael(&d, &["hook", "session-start", "--client", "claude"], &input);
    assert!(ok, "{err} {out}");
    for w in ["first word", "second word", "third word"] {
        assert!(out.contains(w), "{w} listed: {out}");
    }
    assert!(
        !out.contains("own checkpoint"),
        "own worktree's note: {out}"
    );
    assert!(
        out.contains(&format!("3 new notes for chunk {uid}")),
        "{out}"
    );
    // the chunk line's pointer and the inbox tail: no third copy per note
    assert_eq!(out.matches("fael chunk show").count(), 2, "{out}");
}

/// Chunk start-budget: a held chunk with a 2000-char note — the session-start
/// context stays within `kickoff_tokens` (800) and the note shows as one line
/// ending in `fael chunk show <uid>`.
#[test]
fn session_start_fits_a_long_note_in_one_line() {
    let d = repo();
    let (ok, out, err) = fael(&d, &["chunk", "add", "T", "--brief", "b"], "");
    assert!(ok, "{err}");
    let uid = out.trim().to_string();
    let (ok, _, err) = fael(&d, &["chunk", "start", &uid, "--run", "R1"], "");
    assert!(ok, "{err}");
    // token-heavy: an id-like run costs 1 token per 2 chars, so 2000 chars
    // would blow the 800-token kickoff budget if inlined whole
    let long = "A1".repeat(1000);
    assert_eq!(long.len(), 2000);
    let (ok, _, err) = fael(&super::sibling(&d), &["chunk", "note", &uid, &long], "");
    assert!(ok, "{err}");
    let input = format!(
        r#"{{"cwd":{},"session_id":"s1","source":"startup"}}"#,
        json(&d)
    );
    let (ok, out, err) = fael(&d, &["hook", "session-start", "--client", "claude"], &input);
    assert!(ok, "{err} {out}");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let ctx = v
        .pointer("/hookSpecificOutput/additionalContext")
        .and_then(|c| c.as_str())
        .unwrap_or("");
    assert!(!ctx.is_empty(), "inbox said: {out}");
    let toks = fael_core::est_tokens(ctx);
    assert!(
        toks <= 800,
        "session-start context {toks} tokens over kickoff budget 800: {out}"
    );
    let note_lines: Vec<&str> = ctx
        .lines()
        .filter(|l| l.contains("new note for chunk"))
        .collect();
    assert_eq!(note_lines.len(), 1, "one inbox line: {out}");
    assert_eq!(note_lines[0].lines().count(), 1);
    assert!(
        note_lines[0]
            .trim_end()
            .ends_with(&format!("fael chunk show {uid}")),
        "ends in show: {}",
        note_lines[0]
    );
    assert!(!ctx.contains(&long[..200]), "full note not inlined");
}

/// Step 1 (own-note): the session that wrote a note resumes — its own note
/// stays silent, another session's note still shows.
#[test]
fn own_note_stays_silent_while_another_session_note_shows() {
    let d = repo();
    let (ok, out, err) = fael(&d, &["chunk", "add", "T", "--brief", "b"], "");
    assert!(ok, "{err}");
    let uid = out.trim().to_string();
    let (ok, _, err) = fael(&d, &["chunk", "start", &uid, "--run", "R1"], "");
    assert!(ok, "{err}");
    let wt = super::sibling(&d);
    for (text, sess) in [("from other", "other"), ("from s1", "s1")] {
        let (ok, _, err) = super::fael_env(
            &wt,
            &["chunk", "note", &uid, text],
            "",
            &[("FAEL_SESSION", sess)],
        );
        assert!(ok, "{err}");
    }
    let input = format!(
        r#"{{"cwd":{},"session_id":"s1","source":"startup"}}"#,
        json(&d)
    );
    let (ok, out, err) = fael(&d, &["hook", "session-start", "--client", "claude"], &input);
    assert!(ok, "{err} {out}");
    assert!(
        out.contains("from other"),
        "other session's note shows: {out}"
    );
    assert!(!out.contains("from s1"), "own note silent: {out}");
}

/// A note written while the agent runs appears at its next edit, once, and
/// the agent's own note never does. ShellEdit stays silent (card admits Edit
/// only).
#[test]
fn inbox_note_appears_at_next_edit_once() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    let (ok, out, err) = fael(&d, &["chunk", "add", "T", "--brief", "b"], "");
    assert!(ok, "{err}");
    let uid = out.trim().to_string();
    let (ok, _, err) = fael(&d, &["chunk", "start", &uid, "--run", "R1"], "");
    assert!(ok, "{err}");
    // notes come from a sibling worktree: one from the run's own is silent
    // (01M88ZGB), so the session filter below is tested on its own
    let wt = super::sibling(&d);
    let note_as = |text: &str, sess: &str| {
        let (ok, _, err) = super::fael_env(
            &wt,
            &["chunk", "note", &uid, text],
            "",
            &[("FAEL_SESSION", sess)],
        );
        assert!(ok, "{err}");
    };
    let start = |session: &str| {
        let input = format!(
            r#"{{"cwd":{},"session_id":"{session}","source":"startup"}}"#,
            json(&d)
        );
        let (ok, out, err) = fael(&d, &["hook", "session-start", "--client", "claude"], &input);
        assert!(ok, "{err} {out}");
        out
    };
    let edit = |session: &str| {
        let f = d.join("src/a.rs");
        let input = format!(
            r#"{{"cwd":{},"session_id":"{session}","tool_input":{{"file_path":{}}}}}"#,
            json(&d),
            json(&f)
        );
        let (ok, out, err) = fael(&d, &["hook", "edit", "--client", "claude"], &input);
        assert!(ok, "{err} {out}");
        out
    };
    note_as("first from other", "other");
    let out = start("s1");
    assert!(out.contains("first from other"), "{out}");
    // a note written while s1 runs: the next edit says it, once
    note_as("second while running", "other");
    let out = edit("s1");
    assert!(
        out.contains("second while running") && out.contains(&format!("fael chunk show {uid}")),
        "edit says the live note once: {out}"
    );
    assert!(
        !out.contains("first from other"),
        "already seen stays silent: {out}"
    );
    let out = edit("s1");
    assert!(
        !out.contains("second while running") && !out.contains("new note for chunk"),
        "second edit silent: {out}"
    );
    // the agent's own note never reaches it, at start or at edit
    note_as("own note", "s1");
    let out = edit("s1");
    assert!(!out.contains("own note"), "own note silent at edit: {out}");
}
