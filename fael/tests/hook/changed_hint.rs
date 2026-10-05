//! PLAN-fael-file-hash chunk 2: the edit hint names the tier-0 rows whose
//! edited file changed since the row was written, earns no hint when it still
//! matches, and names rows with no verdict with their ready retire.

use super::{fael, fael_env, json, repo, strip_fh};
use std::path::Path;

/// An edit of `file` in a fresh session; returns the hook's stdout.
fn edit(d: &Path, session: &str, file: &str) -> String {
    let input = format!(
        r#"{{"cwd":{},"session_id":"{session}","tool_input":{{"file_path":{}}}}}"#,
        json(d),
        json(&d.join(file))
    );
    let (ok, out, err) = fael(d, &["hook", "edit", "--client", "claude"], &input);
    assert!(ok, "{err}");
    let v: serde_json::Value = serde_json::from_str(&out).expect(&out);
    v["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .expect(&out)
        .to_string()
}

/// The full id `add` just filed (its stdout starts with it).
fn add(d: &Path, kind: &str, text: &str, files: &str) -> String {
    let (ok, out, err) = fael(d, &["add", kind, text, "--files", files], "");
    assert!(ok, "{err}");
    out.split_whitespace().next().unwrap().to_string()
}

/// Short ids the hint named (`changed since <short> was written`), several
/// to a line when two rows share one.
fn named(out: &str) -> Vec<&str> {
    out.lines()
        .flat_map(|l| l.split("changed since ").skip(1))
        .map(|l| l.split(" was written").next().unwrap())
        .collect()
}

/// An edit that may say nothing: the hook's context, empty when it was silent.
fn edit_or_silent(d: &Path, session: &str, file: &str) -> String {
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

#[test]
fn unchanged_files_earn_no_hint() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// v1\n").unwrap();
    add(&d, "decision", "retry uses backoff here", "src/a.rs");
    add(&d, "issue", "login loops here", "src/a.rs");
    let out = edit(&d, "s1", "src/a.rs");
    assert!(out.contains("retry uses backoff here"), "{out}");
    assert!(out.contains("login loops here"), "{out}");
    for banned in [
        "changed since",
        "fael close",
        "fael bump",
        "supersedes",
        "a row above",
    ] {
        assert!(!out.contains(banned), "{banned} in:\n{out}");
    }
}

#[test]
fn changed_file_names_the_row_with_retire_commands() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// v1\n").unwrap();
    let id = add(&d, "decision", "retry uses backoff here", "src/a.rs");
    std::fs::write(d.join("src/a.rs"), "// v2 changed\n").unwrap();
    let out = edit(&d, "s1", "src/a.rs");
    let hint = out
        .lines()
        .find(|l| l.contains("changed since"))
        .expect(&out);
    let short = hint.split("changed since ").nth(1).unwrap();
    let short = short.split(" was written").next().unwrap();
    assert!(id.starts_with(short), "{hint}\n{id}");
    for cmd in [
        format!("fael bump {short}"),
        format!("--supersedes {short}"),
        format!("fael close {short}"),
    ] {
        assert!(hint.contains(&cmd), "{cmd} missing in:\n{hint}");
    }
}

#[test]
fn changed_hint_names_at_most_two() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// v1\n").unwrap();
    let ids = [
        add(&d, "decision", "first module choice", "src/a.rs"),
        add(&d, "decision", "second module choice", "src/a.rs"),
        add(&d, "decision", "third module choice", "src/a.rs"),
    ];
    std::fs::write(d.join("src/a.rs"), "// v2 changed\n").unwrap();
    let out = edit(&d, "s1", "src/a.rs");
    let shorts = named(&out);
    assert_eq!(shorts.len(), 2, "{out}");
    // exactly two of the three rows are named (a short names the id it prefixes)
    let hit: Vec<bool> = ids
        .iter()
        .map(|id| shorts.iter().any(|s| id.starts_with(s)))
        .collect();
    assert_eq!(hit.iter().filter(|h| **h).count(), 2, "{shorts:?}\n{out}");
    // two rows share one line and one command tail
    let lines = out.lines().filter(|l| l.contains("changed since")).count();
    assert_eq!(lines, 1, "{out}");
    assert!(out.contains("`fael bump <id>`"), "{out}");
}

#[test]
fn issue_without_fh_keeps_the_named_close() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// v1\n").unwrap();
    add(&d, "issue", "old problem predates hashes", "src/a.rs");
    strip_fh(&d, "predates hashes");
    let out = edit(&d, "s1", "src/a.rs");
    assert!(!out.contains("changed since"), "{out}");
    assert!(out.contains("done with one?"), "{out}");
}

/// The ask names the row it means: an ask with no id is one the agent
/// cannot act on.
#[test]
fn decision_without_fh_is_named_with_its_retire() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// v1\n").unwrap();
    let id = add(&d, "decision", "old choice predates hashes", "src/a.rs");
    strip_fh(&d, "predates hashes");
    let out = edit(&d, "s1", "src/a.rs");
    assert!(!out.contains("changed since"), "{out}");
    assert_retire_names(&out, &id);
}

/// The no-verdict retire line names `id` by its short form, commands included.
fn assert_retire_names(out: &str, id: &str) {
    let hint = out
        .lines()
        .find(|l| l.contains("does the code now say or contradict"))
        .expect(out);
    let short = hint.split("contradict ").nth(1).unwrap();
    let short = short.split('?').next().unwrap();
    assert!(id.starts_with(short), "{hint}\n{id}");
    assert!(hint.contains(&format!("fael close {short} ")), "{hint}");
    assert!(hint.contains(&format!("--supersedes {short}")), "{hint}");
}

/// A row filed under the old path reads the bytes at the new one: renamed
/// but untouched earns no hint, edited after the rename names the row.
#[test]
fn rename_resolves_to_the_new_bytes() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// v1\n").unwrap();
    let id = add(&d, "decision", "moved module choice", "src/a.rs");
    std::fs::rename(d.join("src/a.rs"), d.join("src/b.rs")).unwrap();
    let (ok, _, err) = fael(&d, &["mv", "src/a.rs", "src/b.rs"], "");
    assert!(ok, "{err}");
    let out = edit(&d, "s1", "src/b.rs");
    assert!(out.contains("moved module choice"), "{out}");
    assert!(!out.contains("changed since"), "{out}");
    std::fs::write(d.join("src/b.rs"), "// v2 changed\n").unwrap();
    let out = edit(&d, "s2", "src/b.rs");
    let shorts = named(&out);
    assert_eq!(shorts.len(), 1, "{out}");
    assert!(id.starts_with(shorts[0]), "{out}\n{id}");
}

/// The hint asks only about the file this edit touched: another file of the
/// row (a shared PLAN another worktree edited) is not this session's to judge.
#[test]
fn hint_asks_only_about_the_edited_file() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// a\n").unwrap();
    std::fs::write(d.join("src/c.rs"), "// c v1\n").unwrap();
    add(&d, "decision", "pair choice", "src/a.rs,src/c.rs");
    std::fs::write(d.join("src/c.rs"), "// c v2\n").unwrap();
    let out = edit(&d, "s1", "src/a.rs");
    assert!(out.contains("pair choice"), "{out}");
    assert!(!out.contains("changed since"), "{out}");
    std::fs::write(d.join("src/a.rs"), "// a v2\n").unwrap();
    let out = edit(&d, "s2", "src/a.rs");
    let hint = out
        .lines()
        .find(|l| l.contains("changed since"))
        .expect(&out);
    assert!(hint.starts_with("fael: src/a.rs changed since"), "{hint}");
}

/// A changed row does not hide the ready close of a no-verdict issue.
#[test]
fn changed_row_keeps_the_legacy_issue_close() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// v1\n").unwrap();
    add(&d, "issue", "old problem predates hashes", "src/a.rs");
    strip_fh(&d, "predates hashes");
    add(&d, "decision", "stamped module choice", "src/a.rs");
    std::fs::write(d.join("src/a.rs"), "// v2 changed\n").unwrap();
    let out = edit(&d, "s1", "src/a.rs");
    assert_eq!(named(&out).len(), 1, "{out}");
    assert!(out.contains("done with one?"), "{out}");
}

/// A hub file past the row cap says its peek and the count line: the hint
/// names only rows the push said (a row never shown cannot be judged).
#[test]
fn hint_skips_rows_the_cap_cut() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// v1\n").unwrap();
    for i in 0..14 {
        add(
            &d,
            "decision",
            &format!("hub choice number {i}"),
            "src/a.rs",
        );
    }
    std::fs::write(d.join("src/a.rs"), "// v2 changed\n").unwrap();
    let out = edit(&d, "s1", "src/a.rs");
    assert!(out.contains("3 of 14"), "{out}");
    let ids = named(&out);
    assert!(!ids.is_empty(), "{out}");
    for id in ids {
        assert!(
            out.contains(&format!("- [{id}]")),
            "{id} never shown: {out}"
        );
    }
}

/// A path renamed away and then recreated is read as itself, not through the
/// rename to the file it became.
#[test]
fn recreated_path_is_read_as_itself() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// v1\n").unwrap();
    let id = add(&d, "decision", "split module choice", "src/a.rs");
    std::fs::rename(d.join("src/a.rs"), d.join("src/b.rs")).unwrap();
    let (ok, _, err) = fael(&d, &["mv", "src/a.rs", "src/b.rs"], "");
    assert!(ok, "{err}");
    std::fs::write(d.join("src/a.rs"), "// brand new a\n").unwrap();
    let out = edit(&d, "s1", "src/a.rs");
    let shorts = named(&out);
    assert_eq!(shorts.len(), 1, "{out}");
    assert!(id.starts_with(shorts[0]), "{out}\n{id}");
    assert!(out.contains("fael: src/a.rs changed since"), "{out}");
}

/// An edit hint names a row once per session: the next edit of the same file
/// would only repeat the ask. A new session asks again.
#[test]
fn a_named_row_is_not_named_twice_in_a_session() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// v1\n").unwrap();
    add(&d, "decision", "retry uses backoff here", "src/a.rs");
    std::fs::write(d.join("src/a.rs"), "// v2\n").unwrap();
    assert!(edit_or_silent(&d, "s1", "src/a.rs").contains("changed since"));
    std::fs::write(d.join("src/a.rs"), "// v3\n").unwrap();
    let again = edit_or_silent(&d, "s1", "src/a.rs");
    assert!(!again.contains("changed since"), "{again}");
    assert!(edit_or_silent(&d, "s2", "src/a.rs").contains("changed since"));
}

/// A row with no verdict gets its retire, and an open issue its ready close,
/// once per session too.
#[test]
fn legacy_hints_are_said_once_per_session() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// v1\n").unwrap();
    add(&d, "decision", "old choice predates hashes", "src/a.rs");
    add(&d, "issue", "old problem predates hashes", "src/a.rs");
    strip_fh(&d, "predates hashes");
    let first = edit_or_silent(&d, "s1", "src/a.rs");
    assert!(first.contains("done with one?"), "{first}");
    assert!(first.contains("does the code now say"), "{first}");
    let again = edit_or_silent(&d, "s1", "src/a.rs");
    assert!(!again.contains("done with one?"), "{again}");
    assert!(!again.contains("does the code now say"), "{again}");
}

#[test]
fn a_no_verdict_row_is_named_once_per_session() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// v1\n").unwrap();
    add(&d, "decision", "old choice predates hashes", "src/a.rs");
    strip_fh(&d, "predates hashes");
    let ask = "does the code now say";
    assert!(edit_or_silent(&d, "s1", "src/a.rs").contains(ask));
    assert!(!edit_or_silent(&d, "s1", "src/a.rs").contains(ask));
}

/// The agent that just filed a row is not asked whether it is still true after
/// its own edit; another session is.
#[test]
fn a_row_this_session_filed_is_not_asked_about() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// v1\n").unwrap();
    let (ok, _, err) = fael_env(
        &d,
        &[
            "add",
            "decision",
            "mine this session",
            "--files",
            "src/a.rs",
        ],
        "",
        &[("CLAUDE_CODE_SESSION_ID", "s1")],
    );
    assert!(ok, "{err}");
    std::fs::write(d.join("src/a.rs"), "// v2 changed\n").unwrap();
    let mine = edit_or_silent(&d, "s1", "src/a.rs");
    assert!(!mine.contains("changed since"), "{mine}");
    let other = edit_or_silent(&d, "s2", "src/a.rs");
    assert!(other.contains("changed since"), "{other}");
}

/// Claude keys the hook by its transcript path while the row carries the bare
/// session id (the path's stem): the same session still is not asked.
#[test]
fn a_row_this_session_filed_is_not_asked_about_by_transcript_path() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "// v1\n").unwrap();
    let (ok, _, err) = fael_env(
        &d,
        &["add", "decision", "mine by path", "--files", "src/a.rs"],
        "",
        &[("CLAUDE_CODE_SESSION_ID", "s3")],
    );
    assert!(ok, "{err}");
    std::fs::write(d.join("src/a.rs"), "// v2 changed\n").unwrap();
    let edit = |id: &str| {
        let input = format!(
            r#"{{"cwd":{},"session_id":"{id}","transcript_path":"/h/.claude/projects/p/{id}.jsonl","tool_input":{{"file_path":{}}}}}"#,
            json(&d),
            json(&d.join("src/a.rs"))
        );
        let (ok, out, err) = fael(&d, &["hook", "edit", "--client", "claude"], &input);
        assert!(ok, "{err}");
        out
    };
    let mine = edit("s3");
    assert!(!mine.contains("changed since"), "{mine}");
    let other = edit("s4");
    assert!(other.contains("changed since"), "{other}");
}

/// Stamped at ~2 MiB (over the push path's 1 MiB read cap, under the 16 MiB
/// stamp cap): the edit push does not hash it, so the row has no verdict and
/// the row is named with its retire — never "changed", never silence.
#[test]
fn file_over_the_push_cap_is_unknown_not_changed() {
    let d = repo();
    std::fs::write(
        d.join("src/big.rs"),
        "fn f() { let x = 1; }\n".repeat(100_000),
    )
    .unwrap();
    let id = add(&d, "decision", "big generated table choice", "src/big.rs");
    let out = edit(&d, "s1", "src/big.rs");
    assert!(!out.contains("changed since"), "{out}");
    assert_retire_names(&out, &id);
}
