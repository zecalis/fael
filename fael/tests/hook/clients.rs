//! Client shapes: codex apply_patch edits + last message, claude
//! NotebookEdit paths.

use super::{fael, flagged, json, repo, state, transcript};

/// Every path the edit hook recorded, across sessions.
fn recorded_edits(d: &std::path::Path) -> String {
    std::fs::read_dir(state(d).join("sessions"))
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
    let edit = serde_json::json!({"cwd": d, "transcript_path": t, "tool_name": "apply_patch",
        "tool_input": {"command": patch}})
    .to_string();
    assert!(fael(&d, &["hook", "edit", "--client", "codex"], &edit).0);
    assert!(recorded_edits(&d).contains("src/b.rs"));
    // a bug claim in the last message is flagged on the next push
    let stop = serde_json::json!({"cwd": d, "transcript_path": t,
        "last_assistant_message": "Found a bug: the parser is broken on empty input."})
    .to_string();
    let (ok, out, _) = fael(&d, &["hook", "stop", "--client", "codex"], &stop);
    assert!(ok && !out.contains("block"), "{out}");
    assert!(flagged(&d, &json(&t)));
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
