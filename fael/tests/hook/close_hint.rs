//! PLAN-fael-close-helpers chunk 1: the edit push names the open issues it
//! about the file in context, with the `fael close <short id>` call ready to
//! run; chunk 2's `close --key` lands on the turn's receipt.
//!
//! PLAN-fael-file-hash chunk 2 narrows this to rows with no file verdict:
//! rows stamped with `fh` whose files still match earn no hint at all (see
//! `changed_hint`), so these tests strip `fh` to exercise the legacy path
//! with rows that read as written before hashes existed.

use super::{fael, fael_env, json, repo, strip_fh};

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
    strip_fh(&d, "problem");
    let out = edit_a(&d);
    let hint = out
        .lines()
        .find(|l| l.contains("done with one?"))
        .expect(&out);
    // at most two ready closes, however many issues were shown
    assert_eq!(hint.matches("\"<why>\"").count(), 2, "{hint}");
    // ids are the short form the rows print, never a made-up one
    let shown: Vec<&str> = hint
        .split("fael close ")
        .skip(1)
        .map(|s| s.split(' ').next().unwrap())
        .filter(|id| *id != "<id>")
        .collect();
    assert!(
        shown.iter().all(|id| out.contains(&format!("[{id}"))),
        "{hint}\n{out}"
    );
}

#[test]
fn edit_hint_never_asks_about_a_decision() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    // a decision is shown, but earns no ask (issue hook:changed-line-noise)
    let (ok, _, err) = fael(
        &d,
        &["add", "decision", "pick x", "--files", "src/a.rs"],
        "",
    );
    assert!(ok, "{err}");
    strip_fh(&d, "pick x");
    let out = edit_a(&d);
    assert!(out.contains("pick x"), "{out}");
    assert!(!out.contains("done with one?"), "{out}");
    assert!(!out.contains("fael close <id>"), "{out}");
    assert!(!out.contains("\"now in <file>\""), "{out}");
}

/// The row is already in the agent's context (a `find` said it; a read says
/// nothing) — the edit still offers its ready close.
#[test]
fn edit_after_the_row_was_said_still_offers_the_ready_close() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    let (ok, out, err) = fael(
        &d,
        &["add", "issue", "seen on read", "--files", "src/a.rs"],
        "",
    );
    assert!(ok, "{err}");
    strip_fh(&d, "seen on read");
    let id = out.split_whitespace().next().unwrap().to_string();
    let input = format!(
        r#"{{"cwd":{},"session_id":"s1","tool_input":{{"file_path":{}}}}}"#,
        json(&d),
        json(&d.join("src/a.rs"))
    );
    let (ok, read, err) = fael(&d, &["hook", "read", "--client", "claude"], &input);
    assert!(ok && !read.contains("seen on read"), "{err}{read}");
    let env = [("CLAUDE_CODE_SESSION_ID", "s1")];
    let (ok, found, err) = fael_env(&d, &["find", "--files", "src/a.rs"], "", &env);
    assert!(ok && found.contains("seen on read"), "{err}{found}");
    let out = edit_a(&d);
    // the row itself is not repeated, only the close for it
    assert!(!out.contains("seen on read"), "{out}");
    let hint = out
        .lines()
        .find(|l| l.contains("done with one?"))
        .expect(&out);
    let short = hint.split("fael close ").nth(1).unwrap();
    let short = short.split(' ').next().unwrap();
    assert!(id.starts_with(short), "{hint}");
}

/// `close --key` on a key whose head superseded an older row closes the
/// whole chain, and the turn's receipt names the key.
#[test]
fn close_by_key_closes_the_chain_and_lands_on_the_receipt() {
    let d = repo();
    let env = [("CLAUDE_CODE_SESSION_ID", "s1")];
    let add = |args: &[&str]| {
        let (ok, out, err) = fael_env(&d, args, "", &env);
        assert!(ok, "{err}");
        out.split_whitespace().next().unwrap().to_string()
    };
    let old = add(&["add", "note", "v1", "--key", "k", "--files", "src/a.rs"]);
    let head = add(&[
        "add",
        "note",
        "v2",
        "--key",
        "k",
        "--files",
        "src/a.rs",
        "--supersedes",
        &old,
    ]);
    let (ok, _, err) = fael_env(&d, &["close", "--key", "k", "done"], "", &env);
    assert!(ok, "{err}");
    let (_, all, _) = fael(&d, &["find", "--key", "k", "--all"], "");
    assert!(all.contains("v1") && all.contains("v2"), "{all}");
    let (_, open, _) = fael(&d, &["find", "--key", "k"], "");
    assert!(!open.contains("v1") && !open.contains("v2"), "{open}");
    // chain-close: each version has its own close row
    let closes: String = std::fs::read_dir(d.join(".fael/log"))
        .unwrap()
        .flatten()
        .flat_map(|who| {
            std::fs::read_dir(who.path())
                .into_iter()
                .flatten()
                .flatten()
        })
        .filter(|e| e.file_name().to_string_lossy().ends_with(".close.jsonl"))
        .map(|e| std::fs::read_to_string(e.path()).unwrap())
        .collect();
    assert!(closes.contains(&old) && closes.contains(&head), "{closes}");
    let input = format!(r#"{{"cwd":{},"session_id":"s1"}}"#, json(&d));
    let (ok, out, err) = fael(&d, &["hook", "stop", "--client", "claude"], &input);
    assert!(ok, "{err}");
    assert!(out.contains("closed 1 (#k)"), "{out}");
}
