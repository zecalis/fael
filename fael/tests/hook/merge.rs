//! PLAN-fael-context-loop chunk 3: an edit whose file has six or more open
//! decision/note rows in the agent's context asks once whether they are one
//! matter, naming ids with the ready `--supersedes`; fewer is silent.

use super::{fael, json, repo};
use std::path::Path;

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

fn rows(d: &Path, n: usize) {
    std::fs::write(d.join("src/a.rs"), "// v1\n").unwrap();
    for i in 0..n {
        let (ok, _, err) = fael(
            d,
            &[
                "add",
                "decision",
                &format!("rule {i} of a"),
                "--files",
                "src/a.rs",
            ],
            "",
        );
        assert!(ok, "{err}");
    }
}

#[test]
fn six_rows_in_context_ask_once_with_the_ready_supersedes() {
    let d = repo();
    rows(&d, 6);
    // the first edit says five rows (the push cap): not yet six in context
    let first = edit(&d, "s1", "src/a.rs");
    assert!(!first.contains("one matter?"), "{first}");
    let second = edit(&d, "s1", "src/a.rs");
    assert!(
        second.contains("src/a.rs has 6 open decision/note rows"),
        "{second}"
    );
    assert!(second.contains("fael add decision"), "{second}");
    assert!(second.contains("--supersedes "), "{second}");
    assert!(!edit(&d, "s1", "src/a.rs").contains("one matter?"));
    // the yield: three ids named, earned once the agent closes one
    let yield_of = || {
        let (_, out, _) = fael(&d, &["stats", "--json"], "");
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        let y = &v["said"]["merge"];
        (y["said"].as_u64().unwrap(), y["earned"].as_u64().unwrap())
    };
    assert_eq!(yield_of(), (3, 0));
    let id = second.split("among them ").nth(1).unwrap();
    let id = id.split_whitespace().next().unwrap();
    let (ok, _, err) = fael(&d, &["close", id, "merged"], "");
    assert!(ok, "{err}");
    assert_eq!(yield_of(), (3, 1));
}

#[test]
fn five_rows_stay_silent() {
    let d = repo();
    rows(&d, 5);
    for _ in 0..2 {
        assert!(!edit(&d, "s1", "src/a.rs").contains("one matter?"));
    }
}
