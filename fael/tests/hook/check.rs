//! PLAN-fael-experience-loop chunk 3: an edit of a file whose closed issue
//! was closed pointing at a backticked path that is gone says so once per
//! session; a path that is still there (or no path at all) is silent.

use super::{fael, json, repo};
use std::path::Path;

fn edit(d: &Path, session: &str) -> String {
    let input = format!(
        r#"{{"cwd":{},"session_id":"{session}","tool_input":{{"file_path":{}}}}}"#,
        json(d),
        json(&d.join("src/a.rs"))
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

/// An issue on `src/a.rs`, closed with `why`.
fn closed_with(d: &Path, why: &str) {
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    let (ok, _, err) = fael(
        d,
        &[
            "add",
            "issue",
            "retry loops",
            "--files",
            "src/a.rs",
            "--key",
            "a:retry",
        ],
        "",
    );
    assert!(ok, "{err}");
    let (ok, _, err) = fael(d, &["close", "--key", "a:retry", why], "");
    assert!(ok, "{err}");
}

#[test]
fn a_gone_check_is_said_once_per_session() {
    let d = repo();
    closed_with(&d, "guarded by `scripts/check-retry.sh`");
    let first = edit(&d, "s1");
    assert!(
        first.contains("closed pointing at `scripts/check-retry.sh`, now gone"),
        "{first}"
    );
    assert!(
        first.contains("fael add issue") && first.contains("--supersedes "),
        "{first}"
    );
    assert!(!edit(&d, "s1").contains("now gone"));
    // another session is told again
    assert!(edit(&d, "s2").contains("now gone"));
    // the yield is recorded under its own kind, not earned yet
    let (_, out, _) = fael(&d, &["stats", "--json"], "");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["said"]["check"]["said"], 2, "{v}");
    assert_eq!(v["said"]["check"]["earned"], 0, "{v}");
}

#[test]
fn a_check_that_is_there_or_no_check_is_silent() {
    let d = repo();
    std::fs::create_dir_all(d.join("scripts")).unwrap();
    std::fs::write(d.join("scripts/check-retry.sh"), "#!/bin/sh\n").unwrap();
    closed_with(&d, "guarded by `scripts/check-retry.sh`");
    assert!(!edit(&d, "s1").contains("now gone"));
    let d = repo();
    closed_with(&d, "fixed in abc1234");
    assert!(!edit(&d, "s1").contains("now gone"));
}

/// The yield: `earned` only once the agent files the successor the line
/// offered (`--supersedes <id>`), never on the line alone.
#[test]
fn the_check_is_earned_when_the_issue_is_superseded() {
    let d = repo();
    closed_with(&d, "guarded by `scripts/check-retry.sh`");
    let said = edit(&d, "s1");
    let id = said.split("--supersedes ").nth(1).expect(&said);
    let id = id
        .split(|c: char| !c.is_ascii_alphanumeric())
        .next()
        .unwrap();
    let earned = || {
        let (_, out, _) = fael(&d, &["stats", "--json"], "");
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        (
            v["said"]["check"]["said"].as_u64(),
            v["said"]["check"]["earned"].as_u64(),
        )
    };
    assert_eq!(earned(), (Some(1), Some(0)));
    let (ok, _, err) = fael(
        &d,
        &[
            "add",
            "issue",
            "retry loops are back",
            "--files",
            "src/a.rs",
            "--supersedes",
            id,
        ],
        "",
    );
    assert!(ok, "{err}");
    assert_eq!(earned(), (Some(1), Some(1)));
}

/// A read of the file is not an edit: no line, and the key stays unspent for
/// the edit that follows. A shell edit counts as an edit.
#[test]
fn a_read_is_silent_and_a_shell_edit_is_said() {
    let d = repo();
    closed_with(&d, "guarded by `scripts/check-retry.sh`");
    let read = format!(
        r#"{{"cwd":{},"session_id":"s1","tool_input":{{"file_path":{}}}}}"#,
        json(&d),
        json(&d.join("src/a.rs"))
    );
    let (ok, out, err) = fael(&d, &["hook", "read", "--client", "claude"], &read);
    assert!(ok, "{err}");
    assert!(!out.contains("now gone"), "{out}");
    let shell = format!(
        r#"{{"cwd":{},"session_id":"s1","tool_name":"Bash","tool_input":{{"command":"sed -i s/a/b/ src/a.rs"}},"tool_response":{{"stdout":""}}}}"#,
        json(&d)
    );
    let (ok, out, err) = fael(&d, &["hook", "search", "--client", "claude"], &shell);
    assert!(ok, "{err}");
    assert!(out.contains("now gone"), "{out}");
}

/// The close shape the skill teaches (`<cause> → <fix>; tried …; guard
/// `<test path>``): a bare file name in the prose is talk, not a check, so
/// it stays silent while the file lives under a dir; the guard path is said
/// once it is gone.
#[test]
fn the_taught_close_shape_says_only_its_guard() {
    let d = repo();
    std::fs::create_dir_all(d.join("tests")).unwrap();
    std::fs::write(d.join("tests/retry.rs"), "\n").unwrap();
    closed_with(
        &d,
        "`a.rs` retried forever → cap at 3; tried a sleep; guard `tests/retry.rs`",
    );
    assert!(!edit(&d, "s1").contains("now gone"));
    std::fs::remove_file(d.join("tests/retry.rs")).unwrap();
    let out = edit(&d, "s2");
    assert!(
        out.contains("pointing at `tests/retry.rs`, now gone") && !out.contains("`a.rs`"),
        "{out}"
    );
}
