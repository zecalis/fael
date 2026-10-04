//! 01M411WF step 2: edit usage lines (and the `in-context` line the same edit
//! writes) name the edited file, and `fael stats` counts two sessions that
//! edited one file within the window under `value.cross_agent.same_file`.
//! Reads carry no file: only an edit says a session is at the file.

use super::{fael, json, repo};
use std::path::Path;

fn hook(d: &Path, event: &str, session: &str, file: &str) {
    let input = format!(
        r#"{{"cwd":{},"session_id":"{session}","tool_input":{{"file_path":{}}}}}"#,
        json(d),
        json(&d.join(file))
    );
    let (ok, _, err) = fael(d, &["hook", event, "--client", "claude"], &input);
    assert!(ok, "{err}");
}

fn usage(d: &Path) -> Vec<serde_json::Value> {
    std::fs::read_to_string(d.join("state/usage.jsonl"))
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

#[test]
fn edit_lines_name_the_file_and_two_sessions_on_it_count() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    let (ok, _, err) = fael(
        &d,
        &[
            "add",
            "decision",
            "keep the parser pure",
            "--files",
            "src/a.rs",
        ],
        "",
    );
    assert!(ok, "{err}");
    // s1 reads, then edits: the row is already in its context, so the edit
    // says nothing and only the in-context line (which names the file too)
    // shows it was there; s2 edits cold and the push says the row
    hook(&d, "read", "s1", "src/a.rs");
    hook(&d, "edit", "s1", "src/a.rs");
    hook(&d, "edit", "s2", "src/a.rs");
    let lines = usage(&d);
    let files = |v: &serde_json::Value| v["files"].clone();
    for l in lines.iter().filter(|l| l["event"] == "read") {
        assert!(l.get("files").is_none(), "a read names no file: {l}");
    }
    let edits: Vec<_> = lines.iter().filter(|l| l["event"] == "edit").collect();
    assert_eq!(edits.len(), 1, "{lines:?}");
    assert_eq!(edits[0]["session"], "s2", "{lines:?}");
    for l in edits {
        assert_eq!(files(l), serde_json::json!(["src/a.rs"]), "{l}");
    }
    let ctx = lines.iter().find(|l| l["event"] == "in-context").unwrap();
    assert_eq!(files(ctx), serde_json::json!(["src/a.rs"]), "{ctx}");

    let (ok, out, _) = fael(&d, &["stats", "--json"], "");
    assert!(ok, "{out}");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(
        v["value"]["cross_agent"]["same_file"],
        serde_json::json!({"sessions_seen": 2, "files": 1, "session_pairs": 1}),
        "{out}"
    );
}
