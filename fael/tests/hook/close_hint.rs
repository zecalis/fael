//! PLAN-fael-close-helpers chunk 1: the edit push names the open issues it
//! just showed, with the `fael close <short id>` call ready to run.

use super::{fael, json, repo};

/// An edit of `src/a.rs` in session `s1`; returns the hook's stdout.
fn edit_a(d: &std::path::Path) -> String {
    let input = format!(
        r#"{{"cwd":{},"session_id":"s1","tool_input":{{"file_path":{}}}}}"#,
        json(d),
        json(&d.join("src/a.rs"))
    );
    let (ok, out, err) = fael(d, &["hook", "edit", "--client", "claude"], &input);
    assert!(ok, "{err}");
    let v: serde_json::Value = serde_json::from_str(&out).expect(&out);
    v["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .expect(&out)
        .to_string()
}

#[test]
fn edit_hint_names_open_issues_with_a_ready_close() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    for t in ["first problem", "second problem", "third problem"] {
        let (ok, _, err) = fael(&d, &["add", "issue", t, "--files", "src/a.rs"], "");
        assert!(ok, "{err}");
    }
    let out = edit_a(&d);
    let hint = out
        .lines()
        .find(|l| l.contains("done with one?"))
        .expect(&out);
    // at most two ready closes, however many issues were shown
    assert_eq!(hint.matches("fael close ").count(), 2, "{hint}");
    assert!(
        hint.contains("\"<why>\"") && hint.contains("--supersedes"),
        "{hint}"
    );
    // ids are the short form the rows print, never a made-up one
    let shown: Vec<&str> = hint
        .split("fael close ")
        .skip(1)
        .map(|s| s.split(' ').next().unwrap())
        .collect();
    assert!(
        shown.iter().all(|id| out.contains(&format!("[{id}"))),
        "{hint}\n{out}"
    );
}

#[test]
fn edit_hint_stays_generic_without_an_open_issue() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    // a decision is shown, but never gets a ready close
    let (ok, _, err) = fael(
        &d,
        &["add", "decision", "pick x", "--files", "src/a.rs"],
        "",
    );
    assert!(ok, "{err}");
    let out = edit_a(&d);
    assert!(out.contains("pick x"), "{out}");
    assert!(!out.contains("done with one?"), "{out}");
    assert!(out.contains("fael close <id> \"now in <file>\""), "{out}");
}
