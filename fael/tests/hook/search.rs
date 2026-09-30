//! Grep/Bash push: an agent that reads through the shell still gets the rows
//! about the files it touched — from the command's arguments and from a
//! grep's hit list, never from a path that is not a file on disk.

use super::{fael, json, repo};
use std::path::Path;

fn seed(d: &Path) {
    for (f, text) in [("src/a.rs", "login loops"), ("src/b.rs", "retry storms")] {
        std::fs::write(d.join(f), "// x\n").unwrap();
        let (ok, _, err) = fael(d, &["add", "issue", text, "--files", f], "");
        assert!(ok, "{err}");
    }
}

/// One `PostToolUse` call in Claude's shape, through `fael hook search`.
fn search(d: &Path, session: &str, tool: &str, input: &str, response: &str) -> String {
    let payload = format!(
        r#"{{"cwd":{},"session_id":"{session}","tool_name":"{tool}","tool_input":{input},"tool_response":{response}}}"#,
        json(d)
    );
    let (ok, out, err) = fael(d, &["hook", "search", "--client", "claude"], &payload);
    assert!(ok, "{err}");
    out
}

fn bash(d: &Path, session: &str, cmd: &str, stdout: &str) -> String {
    search(
        d,
        session,
        "Bash",
        &format!(r#"{{"command":{}}}"#, serde_json::to_string(cmd).unwrap()),
        &format!(r#"{{"stdout":{}}}"#, serde_json::to_string(stdout).unwrap()),
    )
}

#[test]
fn a_shell_read_pushes_the_rows_of_its_file() {
    let d = repo();
    seed(&d);
    for (i, cmd) in [
        "sed -n 1,20p src/a.rs",
        "cat src/a.rs | head -5",
        "cd /tmp && cat 'src/a.rs'",
        "git show HEAD:src/a.rs",
    ]
    .iter()
    .enumerate()
    {
        let out = bash(&d, &format!("s{i}"), cmd, "");
        assert!(out.contains("login loops"), "{cmd}: {out}");
        assert!(!out.contains("retry storms"), "{cmd}: {out}");
    }
}

#[test]
fn a_grep_hit_list_pushes_the_files_it_names() {
    let d = repo();
    seed(&d);
    let out = bash(
        &d,
        "s1",
        "grep -rn x src",
        "src/a.rs:1:// x\nsrc/b.rs:1:// x\n",
    );
    assert!(
        out.contains("login loops") && out.contains("retry storms"),
        "{out}"
    );
    // the Grep tool: files_with_matches lists bare paths
    let out = search(
        &d,
        "s2",
        "Grep",
        r#"{"pattern":"x","path":"src"}"#,
        r#"{"mode":"files_with_matches","filenames":["src/b.rs"],"numFiles":1}"#,
    );
    assert!(
        out.contains("retry storms") && !out.contains("login loops"),
        "{out}"
    );
}

#[test]
fn only_readers_and_real_files_push() {
    let d = repo();
    seed(&d);
    // not a reader: the argument is not a touch
    assert!(bash(&d, "s1", "cargo test src/a.rs", "src/a.rs:1: x").is_empty());
    assert!(bash(&d, "s1", "git commit -m src/a.rs", "").is_empty());
    // a reader on a path with no file behind it, and on a directory
    assert!(bash(&d, "s1", "cat src/nope.rs", "").is_empty());
    assert!(bash(&d, "s1", "grep -rn x src", "").is_empty());
    // cat's output is file content, never a hit list
    assert!(bash(&d, "s1", "cat notes.txt", "src/a.rs:1: x").is_empty());
}

#[test]
fn a_hit_list_is_capped() {
    let d = repo();
    let mut hits = String::new();
    for i in 0..20 {
        let f = format!("src/f{i}.rs");
        std::fs::write(d.join(&f), "// x\n").unwrap();
        let (ok, _, err) = fael(
            &d,
            &["add", "note", &format!("about f{i}"), "--files", &f],
            "",
        );
        assert!(ok, "{err}");
        hits.push_str(&format!("{f}:1:// x\n"));
    }
    let out = bash(&d, "s1", "grep -rn x src", &hits);
    // only the first 8 hits are looked up
    assert!(
        out.contains("src/f7.rs") && !out.contains("src/f8.rs"),
        "{out}"
    );
}
