//! Session start names an open issue a commit on the branch names
//! (`hook/named.rs`): one close line, once per session, gone once closed.

use super::{commit, fael, json, repo};
use std::path::Path;

fn start(d: &Path, session: &str) -> String {
    let input = format!(r#"{{"cwd":{},"session":"{session}"}}"#, json(d));
    let (ok, out, err) = fael(d, &["hook", "session-start"], &input);
    assert!(ok, "{err}");
    out
}

#[test]
fn an_open_issue_a_commit_names_gets_one_close_line() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "//\n").unwrap();
    let (ok, out, err) = fael(
        &d,
        &[
            "add",
            "issue",
            "retry has no backoff",
            "--title",
            "retry no backoff",
            "--files",
            "src/a.rs",
        ],
        "",
    );
    assert!(ok, "{err}");
    let id = out.split_whitespace().next().unwrap().to_string();
    assert!(!start(&d, "s0").contains("names it"), "no commit yet");

    commit(&d, &format!("fix(a): retry backs off (issue {})", &id[..8]));
    let out = start(&d, "s1");
    assert!(
        out.contains(&format!("{} is still open, and commit", &id[..8])),
        "{out}"
    );
    assert!(out.contains("fael close"), "{out}");

    // the commit push later in the same session does not say it again
    let input = serde_json::json!({"cwd": d, "session": "s1", "tool": "Bash",
        "tool_input": {"command": format!("git commit -m 'more (issue {})'", &id[..8])}});
    let (ok, out, _) = fael(&d, &["hook", "search"], &input.to_string());
    assert!(ok && !out.contains("cited in a commit"), "{out}");
    // …while a session that never started here still gets it
    let mut fresh = input.clone();
    fresh["session"] = "s9".into();
    let (_, out, _) = fael(&d, &["hook", "search"], &fresh.to_string());
    assert!(out.contains("cited in a commit"), "{out}");

    let (ok, _, err) = fael(&d, &["close", &id, "backoff added"], "");
    assert!(ok, "{err}");
    assert!(!start(&d, "s2").contains("names it"), "closed = silent");
}
