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
