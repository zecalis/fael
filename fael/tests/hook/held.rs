//! PLAN-fael-row-retire w1 (from PLAN-fael-decision-held chunk 2): an open
//! decision or note — any writer — on a file this session edited, changed
//! since the row was written, is named once, at the push after stop (here an
//! edit of a file no row is on), with bump / close / supersede ready to run.
//! Never at the edit before stop, never an issue, an unedited or unchanged
//! file, or a row already closed or superseded.

use super::{fael, fael_env, json};
use std::path::Path;

const S: &str = "2020-01-01T00:00:00Z";
const HELD: &str = "changed since it was written";

fn hook(d: &Path, event: &str, file: &str) -> String {
    let input = format!(
        r#"{{"cwd":{},"session":"{S}","files":["{file}"]}}"#,
        json(d)
    );
    let (ok, out, err) = fael(d, &["hook", event], &input);
    assert!(ok, "{err}");
    out
}

/// Change `file` on disk, then the session's edit hook for it.
fn edit(d: &Path, file: &str) -> String {
    std::fs::write(d.join(file), format!("fn {}() {{}}\n", fael_core::ulid())).unwrap();
    hook(d, "edit", file)
}

fn stop(d: &Path) {
    let input = format!(r#"{{"cwd":{},"session":"{S}","text":"done"}}"#, json(d));
    let (ok, out, err) = fael(d, &["hook", "stop"], &input);
    assert!(ok && out.contains(r#""block":false"#), "{out}{err}");
}

/// File a `kind` row; `session` stamps it as that session's, `more` adds flags.
fn add(
    d: &Path,
    kind: &str,
    text: &str,
    file: &str,
    session: Option<&str>,
    more: &[&str],
) -> String {
    let envs: Vec<(&str, &str)> = session.map(|s| ("FAEL_SESSION", s)).into_iter().collect();
    let args = [&["add", kind, text, "--files", file][..], more].concat();
    let (ok, out, err) = fael_env(d, &args, "", &envs);
    assert!(ok, "{err}");
    let id = out
        .split_whitespace()
        .find(|w| w.starts_with("01"))
        .unwrap();
    id.trim_matches(|c: char| !c.is_ascii_alphanumeric())
        .to_string()
}

/// The short id `out` names `id` by — fael abbreviates, and no other row's
/// short id is a prefix of this one's. `None` when `out` never names it.
fn said_id(out: &str, id: &str) -> Option<String> {
    out.split(|c: char| !c.is_ascii_alphanumeric())
        .find(|w| w.len() >= 8 && id.starts_with(w))
        .map(String::from)
}

fn decide(d: &Path, text: &str, file: &str, session: Option<&str>) -> String {
    add(d, "decision", text, file, session, &[])
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
    let out = edit(&d, "src/a.rs");
    assert!(!out.contains(HELD), "never at the edit: {out}");
    stop(&d);
    let out = hook(&d, "edit", "src/c.rs");
    let id = said_id(&out, &id).unwrap_or(id);
    for want in [
        format!("fael bump {id}"),
        format!(r#"fael close {id} \"now in `src/a.rs`\""#),
        format!("--supersedes {id}"),
    ] {
        assert!(out.contains(&want), "{want}: {out}");
    }
    assert_eq!(out.matches(HELD).count(), 1, "{out}");
    assert!(!out.contains("&&"), "one row, no chain: {out}");
    assert!(!hook(&d, "edit", "src/c.rs").contains(HELD), "shown once");
    // a later turn's decision: the session was already told
    decide(&d, "rows sort by date", "src/a.rs", Some(S));
    edit(&d, "src/a.rs");
    stop(&d);
    assert!(
        !hook(&d, "edit", "src/c.rs").contains(HELD),
        "once per session"
    );
}

#[test]
fn any_writers_decision_or_note_on_an_edited_changed_file_is_named() {
    let d = repo();
    let other = decide(&d, "written by another session", "src/a.rs", Some("other"));
    let none = add(
        &d,
        "note",
        "written outside any session",
        "src/a.rs",
        None,
        &[],
    );
    let user = decide(&d, "the user said so", "src/a.rs", Some("other"));
    let own = decide(&d, "this session's own", "src/a.rs", Some(S));
    edit(&d, "src/a.rs");
    stop(&d);
    let out = hook(&d, "edit", "src/c.rs");
    // own row first, then log order; at most three
    let at = |id: &str| {
        let s = said_id(&out, id).unwrap_or_default();
        out.find(&format!("fael bump {s} ")).unwrap_or(usize::MAX)
    };
    assert!(at(&own) < at(&other) && at(&other) < at(&none), "{out}");
    assert!(said_id(&out, &user).is_none(), "cut at three: {out}");
    assert!(out.contains("fael: note "), "{out}");
    assert!(
        out.contains("chain several in one shell call with &&"),
        "{out}"
    );
}

#[test]
fn a_from_user_row_says_ask_the_user() {
    let d = repo();
    decide(&d, "x", "src/a.rs", None);
    add(
        &d,
        "decision",
        "the user said so",
        "src/a.rs",
        None,
        &["--from", "user"],
    );
    edit(&d, "src/a.rs");
    stop(&d);
    let out = hook(&d, "edit", "src/c.rs");
    assert_eq!(out.matches("ask them before").count(), 1, "{out}");
}

#[test]
fn an_issue_an_unedited_or_unchanged_file_is_silent() {
    let d = repo();
    add(
        &d,
        "issue",
        "issues have the edit ask",
        "src/a.rs",
        Some(S),
        &[],
    );
    decide(&d, "on a file not edited", "src/b.rs", Some(S));
    edit(&d, "src/a.rs");
    // written after the edit: the file matches its stamp
    decide(&d, "written after the edit", "src/a.rs", Some(S));
    stop(&d);
    assert!(!hook(&d, "edit", "src/c.rs").contains(HELD));
}

#[test]
fn a_decision_closed_or_superseded_in_the_session_is_not_named() {
    let d = repo();
    let closed = decide(&d, "closed before the push", "src/a.rs", Some(S));
    let old = decide(&d, "superseded before the push", "src/a.rs", Some(S));
    edit(&d, "src/a.rs");
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
    let out = hook(&d, "edit", "src/c.rs");
    assert!(!out.contains(HELD), "{out}");
}
