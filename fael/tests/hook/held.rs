//! PLAN-fael-decision-held chunk 2 (M1): a decision this session wrote on a
//! file it edited is named once, at the push after stop, with a ready
//! `fael close <id> "now in <path>"`. Never at the edit, never another
//! session's decision, never one already closed or superseded.

use super::{fael, fael_env, json};
use std::path::Path;

const S: &str = "2020-01-01T00:00:00Z";
const HELD: &str = "was written this session";

fn hook(d: &Path, event: &str, file: &str) -> String {
    let input = format!(
        r#"{{"cwd":{},"session":"{S}","files":["{file}"]}}"#,
        json(d)
    );
    let (ok, out, err) = fael(d, &["hook", event], &input);
    assert!(ok, "{err}");
    out
}

fn stop(d: &Path) {
    let input = format!(r#"{{"cwd":{},"session":"{S}","text":"done"}}"#, json(d));
    let (ok, out, err) = fael(d, &["hook", "stop"], &input);
    assert!(ok && out.contains(r#""block":false"#), "{out}{err}");
}

/// File a decision; `session` stamps it as that session's.
fn decide(d: &Path, text: &str, file: &str, session: Option<&str>) -> String {
    let envs: Vec<(&str, &str)> = session.map(|s| ("FAEL_SESSION", s)).into_iter().collect();
    let (ok, out, err) = fael_env(d, &["add", "decision", text, "--files", file], "", &envs);
    assert!(ok, "{err}");
    let id = out
        .split_whitespace()
        .find(|w| w.starts_with("01"))
        .unwrap();
    id.trim_matches(|c: char| !c.is_ascii_alphanumeric())
        .to_string()
}

fn repo() -> std::path::PathBuf {
    let d = super::repo();
    for f in ["src/a.rs", "src/b.rs"] {
        std::fs::write(d.join(f), "fn x() {}\n").unwrap();
    }
    d
}

#[test]
fn the_sessions_decision_on_an_edited_file_is_named_once_after_stop() {
    let d = repo();
    let id = decide(&d, "column widths hold still", "src/a.rs", Some(S));
    let edit = hook(&d, "edit", "src/a.rs");
    assert!(!edit.contains(HELD), "never at the edit: {edit}");
    stop(&d);
    let out = hook(&d, "read", "src/b.rs");
    let close = format!(r#"fael close {id} \"now in `src/a.rs`\""#);
    assert!(
        out.matches(HELD).count() == 1 && out.contains(&close),
        "{out}"
    );
    assert!(!hook(&d, "read", "src/b.rs").contains(HELD), "shown once");
    // a later turn's decision: the session was already told
    decide(&d, "rows sort by date", "src/a.rs", Some(S));
    stop(&d);
    assert!(
        !hook(&d, "read", "src/b.rs").contains(HELD),
        "once per session"
    );
}

#[test]
fn another_sessions_decision_or_an_unedited_file_is_silent() {
    let d = repo();
    decide(&d, "written by another session", "src/a.rs", Some("other"));
    decide(&d, "written outside any session", "src/a.rs", None);
    decide(&d, "on a file not edited", "src/b.rs", Some(S));
    hook(&d, "edit", "src/a.rs");
    stop(&d);
    assert!(!hook(&d, "read", "src/b.rs").contains(HELD));
}

#[test]
fn a_decision_closed_or_superseded_in_the_session_is_not_named() {
    let d = repo();
    let closed = decide(&d, "closed before the push", "src/a.rs", Some(S));
    let old = decide(&d, "superseded before the push", "src/a.rs", Some(S));
    hook(&d, "edit", "src/a.rs");
    stop(&d);
    // after stop stashed them: the push re-reads the log
    assert!(fael(&d, &["close", &closed, "now in src/a.rs"], "").0);
    let (ok, _, err) = fael(
        &d,
        &[
            "add",
            "decision",
            "the new rule",
            "--files",
            "src/a.rs",
            "--supersedes",
            &old,
        ],
        "",
    );
    assert!(ok, "{err}");
    let out = hook(&d, "read", "src/b.rs");
    assert!(!out.contains(HELD), "{out}");
}
