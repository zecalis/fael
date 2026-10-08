//! PLAN-fael-context-loop chunk 3: an edit whose file has six or more open
//! decision/note rows in the agent's context asks once whether they are one
//! matter, naming ids with the ready `--supersedes`; fewer is silent.

use super::{fael, fael_env, json, repo};
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

/// `n` rows of `kind` on `file`, each with `extra` args.
fn rows_of(d: &Path, kind: &str, n: usize, file: &str, extra: &[&str]) {
    for i in 0..n {
        let text = format!("{kind} {i} of {file}");
        let mut args = vec!["add", kind, text.as_str(), "--files", file];
        args.extend(extra);
        let (ok, _, err) = fael(d, &args, "");
        assert!(ok, "{err}");
    }
}

/// Two edits of `src/a.rs` (the second says what the push cap held back):
/// did either ask?
fn asked(d: &Path, session: &str) -> bool {
    let first = edit(d, session, "src/a.rs");
    first.contains("one matter?") || edit(d, session, "src/a.rs").contains("one matter?")
}

#[test]
fn the_users_rows_do_not_count() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// v1\n").unwrap();
    rows_of(&d, "decision", 3, "src/a.rs", &[]);
    rows_of(&d, "decision", 3, "src/a.rs", &["--from", "user"]);
    assert!(!asked(&d, "s1"));
}

#[test]
fn issues_do_not_count() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// v1\n").unwrap();
    rows_of(&d, "decision", 3, "src/a.rs", &[]);
    rows_of(&d, "issue", 3, "src/a.rs", &[]);
    assert!(!asked(&d, "s1"));
}

/// The agent that filed the rows is not asked to merge them; another session is.
#[test]
fn rows_this_session_filed_do_not_count() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// v1\n").unwrap();
    // the session stamp needs a recorded session: an edit ran under it first
    // (as in changed_hint's own-row test, with a row on another file)
    std::fs::write(d.join("src/b.rs"), "// b\n").unwrap();
    rows_of(&d, "decision", 1, "src/b.rs", &[]);
    edit(&d, "s1", "src/a.rs");
    for i in 0..6 {
        let (ok, _, err) = fael_env(
            &d,
            &[
                "add",
                "decision",
                &format!("mine {i}"),
                "--files",
                "src/a.rs",
            ],
            "",
            &[("CLAUDE_CODE_SESSION_ID", "s1")],
        );
        assert!(ok, "{err}");
    }
    assert!(!asked(&d, "s1"));
    assert!(asked(&d, "s2"));
}

#[test]
fn six_notes_are_filed_as_a_note() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// v1\n").unwrap();
    // distinct words: same-shape notes on one file are self-healed into one
    for w in ["retry", "cache", "locale", "auth", "queue", "export"] {
        let text = format!("{w} lives behind its own switch here");
        let key = format!("a:{w}");
        let (ok, _, err) = fael(
            &d,
            &["add", "note", &text, "--files", "src/a.rs", "--key", &key],
            "",
        );
        assert!(ok, "{err}");
    }
    edit(&d, "s1", "src/a.rs");
    let second = edit(&d, "s1", "src/a.rs");
    assert!(second.contains("fael add note"), "{second}");
}

/// A shell edit naming two files: rows are counted per file, never pooled,
/// and the ask names the file that holds them.
#[test]
fn a_shell_edit_of_two_files_counts_each_file_alone() {
    let shell = |d: &Path| {
        let input = format!(
            r#"{{"cwd":{},"session_id":"s1","tool_name":"Bash","tool_input":{{"command":"sed -i s/x/y/ src/a.rs src/b.rs"}},"tool_response":{{"stdout":""}}}}"#,
            json(d)
        );
        let (ok, out, err) = fael(d, &["hook", "search", "--client", "claude"], &input);
        assert!(ok, "{err}");
        out
    };
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    std::fs::write(d.join("src/b.rs"), "// b\n").unwrap();
    rows_of(&d, "decision", 3, "src/a.rs", &[]);
    rows_of(&d, "decision", 3, "src/b.rs", &[]);
    shell(&d);
    assert!(!shell(&d).contains("one matter?"), "3 + 3 are not 6");
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    std::fs::write(d.join("src/b.rs"), "// b\n").unwrap();
    rows_of(&d, "decision", 1, "src/a.rs", &[]);
    rows_of(&d, "decision", 6, "src/b.rs", &[]);
    shell(&d);
    let out = shell(&d);
    assert!(
        out.contains("src/b.rs has 6 open decision/note rows"),
        "{out}"
    );
}
