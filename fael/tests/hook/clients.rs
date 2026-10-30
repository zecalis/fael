//! Client shapes: codex apply_patch edits + last message, claude
//! NotebookEdit paths.

use super::{fael, flagged, repo, state, transcript};

/// Redacted real CLI 0.166.0-alpha.5.1 hook payloads; substitute only local
/// paths and identities, leaving the vendor's shape intact.
fn codex_fixture(event: &str, d: &std::path::Path, id: &str) -> serde_json::Value {
    let raw = match event {
        "start" => include_str!("fixtures/codex-session-start.json"),
        "edit" => include_str!("fixtures/codex-edit.json"),
        "stop" => include_str!("fixtures/codex-stop.json"),
        _ => unreachable!(),
    };
    let mut v: serde_json::Value = serde_json::from_str(raw).unwrap();
    v["cwd"] = serde_json::json!(d);
    v["session_id"] = id.into();
    let name = std::path::Path::new(v["transcript_path"].as_str().unwrap())
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .replace("<thread-id>", id);
    v["transcript_path"] = serde_json::json!(d.join(name));
    v
}

/// Every path the edit hook recorded, across sessions.
fn recorded_edits(d: &std::path::Path) -> String {
    std::fs::read_dir(state(d).join("sessions").join("edits"))
        .unwrap()
        .map(|e| std::fs::read_to_string(e.unwrap().path()).unwrap())
        .collect()
}

#[test]
fn codex_apply_patch_edits_and_last_message() {
    let d = repo();
    let (ok, _, err) = fael(
        &d,
        &["add", "decision", "old choice", "--files", "src/a.rs"],
        "",
    );
    assert!(ok, "{err}");
    let t = transcript(&d, "rollout.jsonl");
    std::fs::write(d.join("src/b.rs"), "//\n").unwrap();
    let patch = "*** Begin Patch\n*** Add File: src/b.rs\n+//\n*** End Patch\n";
    let edit = serde_json::json!({"cwd": d, "session_id": "codex-thread", "transcript_path": t, "tool_name": "apply_patch",
        "tool_input": {"command": patch}})
    .to_string();
    assert!(fael(&d, &["hook", "edit", "--client", "codex"], &edit).0);
    assert!(recorded_edits(&d).contains("src/b.rs"));
    // a bug claim in the last message is flagged on the next push
    let mut stop = codex_fixture("stop", &d, "codex-thread");
    stop["transcript_path"] = serde_json::json!(t);
    stop["last_assistant_message"] = "Found a bug: the parser is broken on empty input.".into();
    let stop = stop.to_string();
    let (ok, out, _) = fael(&d, &["hook", "stop", "--client", "codex"], &stop);
    assert!(ok && !out.contains("block"), "{out}");
    assert!(flagged(&d, r#""codex-thread""#));
}

#[test]
fn claude_notebook_edit_is_recorded() {
    let d = repo();
    let (ok, _, err) = fael(
        &d,
        &["add", "decision", "old choice", "--files", "src/a.rs"],
        "",
    );
    assert!(ok, "{err}");
    let t = transcript(&d, "t.jsonl");
    std::fs::write(d.join("src/n.ipynb"), "{}").unwrap();
    let edit = serde_json::json!({"cwd": d, "transcript_path": t,
        "tool_input": {"notebook_path": d.join("src/n.ipynb")}})
    .to_string();
    assert!(fael(&d, &["hook", "edit", "--client", "claude"], &edit).0);
    assert!(recorded_edits(&d).contains("src/n.ipynb"));
}

/// Real Codex rollouts include a timestamp before the thread UUID. The shell
/// knows only the UUID, so two chats must join on session_id, never the stem.
#[test]
fn codex_rollouts_join_shell_threads_without_crossing_chats() {
    let d = repo();
    assert!(fael(&d, &["add", "note", "seed", "--files", "doc:seed"], "").0);
    let threads = [
        "019dba93-8214-7d50-a089-9690b4ce6b9e",
        "019dba93-8214-7d50-a089-9690b4ce6b9f",
    ];
    for (id, file) in threads.iter().zip(["src/a.rs", "src/b.rs"]) {
        std::fs::write(d.join(file), "// file\n").unwrap();
        let start = codex_fixture("start", &d, id).to_string();
        assert!(fael(&d, &["hook", "session-start", "--client", "codex"], &start).0);
        let (ok, _, err) = super::fael_env(
            &d,
            &["add", "note", id, "--files", "doc:before-edit"],
            "",
            &[("CODEX_THREAD_ID", id)],
        );
        assert!(ok, "{err}");
    }
    // Record both edits after every row so the global row cutoff cannot hide one.
    std::thread::sleep(std::time::Duration::from_millis(5));
    for (id, file) in threads.iter().zip(["src/a.rs", "src/b.rs"]) {
        let mut edit = codex_fixture("edit", &d, id);
        edit["tool_input"]["command"] = format!("*** Update File: {file}").into();
        let edit = edit.to_string();
        assert!(fael(&d, &["hook", "edit", "--client", "codex"], &edit).0);
    }
    let (ok, out, err) = super::fael_env(
        &d,
        &["add", "note", "thread b files"],
        "",
        &[("CODEX_THREAD_ID", threads[1])],
    );
    assert!(ok, "{out}{err}");
    let log = super::writer::log_text(&d);
    let rows: Vec<serde_json::Value> = log
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    for id in threads {
        let row = rows.iter().find(|r| r["text"] == id).unwrap();
        assert_eq!(row["session"], id);
    }
    let row = rows.iter().find(|r| r["text"] == "thread b files").unwrap();
    assert_eq!(row["files"], serde_json::json!(["src/b.rs"]));
    assert_eq!(row["session"], threads[1]);
}

/// The vendor lifecycle contract names the parent session and the child agent.
/// A child tool event instead names its own session. Both must reach one context.
#[test]
fn codex_subagent_start_and_stop_use_the_child_context() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    let (ok, id, err) = fael(
        &d,
        &[
            "add",
            "issue",
            "child assigned handoff",
            "--files",
            "src/a.rs",
            "--to",
            "codex",
        ],
        "",
    );
    assert!(ok, "{err}");
    let id = id.split_whitespace().next().unwrap();
    let start = |session: &str, agent: Option<&str>| {
        let p = serde_json::json!({"cwd": d, "session_id":session,
            "transcript_path":d.join(format!("rollout-2026-10-31T04-54-13-{session}.jsonl")),
            "hook_event_name":if agent.is_some() { "SubagentStart" } else { "SessionStart" },
            "agent_id":agent,"source":"startup"})
        .to_string();
        fael(&d, &["hook", "session-start", "--client", "codex"], &p).1
    };
    assert!(start("parent", None).contains("child assigned handoff"));
    let out = start("parent", Some("child"));
    assert!(
        out.contains("child assigned handoff") && out.contains("SubagentStart"),
        "{out}"
    );
    assert!(!start("parent", Some("child")).contains("child assigned handoff"));
    assert!(start("parent", Some("sibling")).contains("child assigned handoff"));
    assert!(!start("parent", None).contains("child assigned handoff"));
    // Child shell joins its lifecycle identity before any edit.
    let (ok, _, err) = super::fael_env(
        &d,
        &["add", "note", "child shell row", "--files", "doc:child"],
        "",
        &[("CODEX_THREAD_ID", "child")],
    );
    assert!(ok, "{err}");
    assert!(
        super::writer::log_text(&d)
            .lines()
            .any(|l| l.contains("child shell row") && l.contains(r#""session":"child""#))
    );
    let mut edit: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/codex-child-edit.json")).unwrap();
    edit["cwd"] = serde_json::json!(d);
    edit["session_id"] = "parent".into();
    edit["agent_id"] = "child".into();
    let edit = edit.to_string();
    assert!(fael(&d, &["hook", "edit", "--client", "codex"], &edit).0);
    let edits = recorded_edits(&d);
    assert!(
        edits.lines().any(|l| {
            let v: serde_json::Value = serde_json::from_str(l).unwrap();
            v["session"] == "child"
        }),
        "{edits}"
    );
    let (ok, out, err) = super::fael_env(
        &d,
        &["add", "note", "child derived files"],
        "",
        &[("CODEX_THREAD_ID", "child")],
    );
    assert!(ok, "{out}{err}");
    let stop = serde_json::json!({"cwd":d,"session_id":"parent", "hook_event_name":"SubagentStop",
        "agent_id":"child", "transcript_path":d.join("parent.jsonl"),
        "agent_transcript_path":d.join("child.jsonl"),
        "last_assistant_message":format!("Found a bug: cited {id}")})
    .to_string();
    let (ok, out, err) = fael(&d, &["hook", "stop", "--client", "codex"], &stop);
    assert!(ok && out.is_empty(), "{out}{err}");
    let usage = std::fs::read_to_string(state(&d).join("usage.jsonl")).unwrap();
    let outcomes: Vec<serde_json::Value> = usage
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .filter(|v: &serde_json::Value| v["event"] == "outcome")
        .collect();
    assert!(
        outcomes
            .iter()
            .any(|v| v["session"] == "child"
                && v["cited"].as_array().is_some_and(|ids| !ids.is_empty())),
        "{usage}"
    );
    assert!(!flagged(&d, r#""parent""#));
}
