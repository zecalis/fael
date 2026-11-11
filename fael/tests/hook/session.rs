//! Session start (hand-offs and assigned rows) and the edit push (each row once per session).

use super::{fael, git, json, repo};

#[test]
fn session_start_and_edit_push() {
    let d = repo();
    // empty log = no brief, not an error
    let (ok, out, _) = fael(
        &d,
        &["hook", "session-start", "--client", "claude"],
        r#"{}"#,
    );
    assert!(ok && !out.contains("open issue"), "{out}");

    // kickoff drops rows whose files are all gone, so the file must exist
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    let (ok, _, err) = fael(
        &d,
        &["add", "issue", "login loops", "--files", "src/a.rs"],
        "",
    );
    assert!(ok, "{err}");
    let input = format!(r#"{{"cwd":{}}}"#, json(&d));
    let (ok, out, _) = fael(&d, &["hook", "session-start", "--client", "claude"], &input);
    // nothing handed to the reader: silence, the issue pushes on file touch
    assert!(ok && out.is_empty(), "{out}");
    // the explicit-arg path keeps today's kickoff: the row is still there
    let (ok, out, _) = fael(&d, &["kickoff", "src/a.rs"], "");
    assert!(ok && out.contains("login loops"), "{out}");

    // edit: claude shape in, PostToolUse context out
    let f = d.join("src/a.rs");
    std::fs::write(&f, "// a\n").unwrap();
    let input = format!(
        r#"{{"cwd":{},"tool_input":{{"file_path":{}}}}}"#,
        json(&d),
        json(&f)
    );
    let (ok, out, _) = fael(&d, &["hook", "edit", "--client", "claude"], &input);
    assert!(
        ok && out.contains("PostToolUse") && out.contains("login loops"),
        "{out}"
    );

    // with a session, a row pushes once — the second edit of the same file does not repeat it
    let input = format!(
        r#"{{"cwd":{},"session_id":"s1","tool_input":{{"file_path":{}}}}}"#,
        json(&d),
        json(&f)
    );
    let (_, out, _) = fael(&d, &["hook", "edit", "--client", "claude"], &input);
    assert!(out.contains("login loops"), "{out}");
    let (ok, out, _) = fael(&d, &["hook", "edit", "--client", "claude"], &input);
    assert!(ok && !out.contains("login loops"), "{out}");

    // neutral shape: Event in, Reply out · a file outside the repo pushes nothing
    let input = format!(r#"{{"cwd":{},"files":["src/a.rs"]}}"#, json(&d));
    let (ok, out, _) = fael(&d, &["hook", "edit"], &input);
    assert!(ok && out.contains("login loops"), "{out}");
    let (ok, out, _) = fael(&d, &["hook", "edit"], r#"{"cwd":"/","files":["x.rs"]}"#);
    assert!(
        ok && out.contains(r#""block":false"#) && !out.contains("context"),
        "{out}"
    );

    // every injection is accounted, per machine
    let (ok, out, _) = fael(&d, &["stats"], "");
    assert!(ok && out.contains("3 injections"), "{out}");
    let (ok, out, _) = fael(&d, &["stats", "--json"], "");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert!(
        ok && v["events"] == 3 && v["by_event"]["edit"]["events"] == 3,
        "{out}"
    );
}

/// Decision 01M5528X: session start says hand-offs of this branch's work and
/// rows `to` the reader — no count line, no urgent unassigned, no decisions —
/// once per context: a resume says nothing, a compact says it again.
#[test]
fn session_start_says_handoffs_and_assigned_only() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    let add = |args: &[&str]| {
        let (ok, _, err) = fael(&d, args, "");
        assert!(ok, "{err}");
    };
    let issue = |text: &str, extra: &[&str]| {
        let mut args = vec!["add", "issue", text, "--files", "src/a.rs"];
        args.extend(extra);
        add(&args);
    };
    // the test repo's writer is Hook Test (hook-test-…): mixed case routes to it
    issue("answer me", &["--to", "Hook-Test"]);
    issue("answer them", &["--to", "someone"]);
    issue("hot unowned", &["--urgent"]);
    add(&["add", "decision", "keep it lean", "--files", "src/a.rs"]);
    let handoff = |text: &str, plan: &str| {
        let key = format!("plan:{plan}:handoff");
        add(&[
            "add",
            "note",
            text,
            "--files",
            &format!("plan:{plan}"),
            "--key",
            &key,
        ]);
    };
    // filed on the default branch: no session's work, never said
    handoff("trunk handoff", "old");
    git(&d, &["switch", "-q", "-c", "feat/x"]);
    handoff("chunk handoff", "demo");
    let start = |source: &str| {
        let input = format!(
            r#"{{"cwd":{},"session_id":"s1","source":"{source}"}}"#,
            json(&d)
        );
        let (ok, out, _) = fael(&d, &["hook", "session-start", "--client", "claude"], &input);
        assert!(ok, "{out}");
        out
    };
    let out = start("startup");
    assert!(out.contains("answer me (to: hook-test)"), "{out}");
    assert!(out.contains("chunk handoff"), "{out}");
    for gone in [
        "answer them",
        "hot unowned",
        "keep it lean",
        "trunk handoff",
        "open issue",
    ] {
        assert!(!out.contains(gone), "{gone} in:\n{out}");
    }
    // hand-offs first, then the reader's own rows
    assert!(out.find("chunk handoff") < out.find("answer me"), "{out}");
    assert!(start("resume").is_empty());
    assert!(start("compact").contains("answer me"));
}

/// PLAN-fael-id-refs chunk-4 (ids:integrity): an id is verified only after
/// find printed it, never typed from memory. PLAN-fael-say-gate chunk 2: the
/// rule rides the skill's description, the help and the reject of an id that
/// does not exist — never the session-start text, and neither does the
/// report rule.
#[test]
fn id_rule_rides_the_reject_not_the_session_start() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    let (ok, _, err) = fael(
        &d,
        &["add", "issue", "login loops", "--files", "src/a.rs"],
        "",
    );
    assert!(ok, "{err}");
    let input = format!(r#"{{"cwd":{}}}"#, json(&d));
    let (ok, out, _) = fael(&d, &["hook", "session-start", "--client", "claude"], &input);
    assert!(ok, "{out}");
    for rule in ["never type an id from memory", "saw something broken"] {
        assert!(!out.contains(rule), "{rule} in:\n{out}");
    }
    // the skill's description is always in context, its body only once invoked
    let skill = include_str!("../../skill/SKILL.md");
    let description = skill
        .lines()
        .find(|l| l.starts_with("description: "))
        .unwrap();
    for rule in ["likely to break", "never from memory"] {
        assert!(
            description.contains(rule),
            "{rule} missing in:\n{description}"
        );
    }
    let (ok, _, err) = fael(&d, &["close", "01ZZZZZZZZ", "done"], "");
    assert!(!ok && err.contains("never from memory"), "{err}");
    let (ok, out, _) = fael(&d, &["find", "--help"], "");
    assert!(ok && out.contains("never type one from memory"), "{out}");
    let (ok, out, _) = fael(&d, &["doctor", "--help"], "");
    assert!(ok && out.contains("[Phantom]"), "{out}");
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
    let (ok, _, err) = fael(&d, &["chunk", "note", &uid, &long], "");
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
    for (text, sess) in [("from other", "other"), ("from s1", "s1")] {
        let (ok, _, err) = super::fael_env(
            &d,
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

/// Start-stopped: a worktree holding a run whose last note is "stopped at
/// step 3" — a new session of another client gets one line
/// `last note (<client>, <age>): stopped at step 3` under the chunk line,
/// within the kickoff budget. Same card (ChunkBrief), same key: said once.
#[test]
fn session_start_shows_held_chunks_last_note_under_the_chunk_line() {
    let d = repo();
    let (ok, out, err) = fael(&d, &["chunk", "add", "T", "--brief", "b"], "");
    assert!(ok, "{err}");
    let uid = out.trim().to_string();
    let (ok, _, err) = fael(
        &d,
        &["chunk", "start", &uid, "--run", "R1", "--client", "claude"],
        "",
    );
    assert!(ok, "{err}");
    let (ok, _, err) = super::fael_env(
        &d,
        &["chunk", "note", &uid, "stopped at step 3"],
        "",
        &[("FAEL_SESSION", "writer")],
    );
    assert!(ok, "{err}");
    let input = format!(
        r#"{{"cwd":{},"session_id":"s2","source":"startup","client":"opencode"}}"#,
        json(&d)
    );
    let (ok, out, err) = fael(&d, &["hook", "session-start"], &input);
    assert!(ok, "{err} {out}");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let ctx = v
        .pointer("/hookSpecificOutput/additionalContext")
        .and_then(|c| c.as_str())
        .or_else(|| v.get("context").and_then(|c| c.as_str()))
        .unwrap_or("");
    let chunk_pos = ctx
        .find(&format!("this worktree runs chunk {uid}"))
        .unwrap_or_else(|| panic!("chunk line: {out}"));
    let note_pos = ctx
        .find("last note (claude,")
        .unwrap_or_else(|| panic!("last note with writer client: {out}"));
    assert!(
        note_pos > chunk_pos,
        "last note under the chunk line: {out}"
    );
    assert!(ctx.contains("stopped at step 3"), "note text: {out}");
    let note_line = ctx
        .lines()
        .find(|l| l.contains("last note (claude,"))
        .unwrap();
    assert_eq!(note_line.lines().count(), 1, "one line: {note_line}");
    let toks = fael_core::est_tokens(ctx);
    assert!(
        toks <= 800,
        "session-start context {toks} tokens over kickoff budget 800: {out}"
    );
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
    let note_as = |text: &str, sess: &str| {
        let (ok, _, err) = super::fael_env(
            &d,
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
