//! Grep/Bash push: an agent that reads through the shell still gets the rows
//! about the files it touched — from the command's arguments and from a
//! grep's hit list when it names one file, never from a path that is not a
//! file on disk.

use super::{fael, json, repo, state};
use std::path::Path;
use std::time::{Duration, SystemTime};

/// Two files with one issue each, written a minute ago — a shell call that
/// names a file modified just now counts as its edit.
fn seed(d: &Path) {
    for (f, text) in [("src/a.rs", "login loops"), ("src/b.rs", "retry storms")] {
        std::fs::write(d.join(f), "// x\n").unwrap();
        backdate(&d.join(f));
        let (ok, _, err) = fael(d, &["add", "issue", text, "--files", f], "");
        assert!(ok, "{err}");
    }
}

/// A minute old: past the window that makes a named file a shell edit.
fn backdate(f: &Path) {
    let old = SystemTime::now() - Duration::from_secs(60);
    let file = std::fs::File::options().write(true).open(f).unwrap();
    file.set_modified(old).unwrap();
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

/// Neutral `fael hook search`: the raw tool call, files resolved server-side.
fn search_neutral(d: &Path, session: &str, tool: &str, input: &str, response: &str) -> String {
    let payload = format!(
        r#"{{"cwd":{},"session":"{session}","client":"opencode","tool":"{tool}","tool_input":{input},"tool_response":{response}}}"#,
        json(d)
    );
    let (ok, out, err) = fael(d, &["hook", "search"], &payload);
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
        // a path after `cd` is under that directory, as the shell reads it
        "cd src && cat 'a.rs'",
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
fn a_grep_hit_list_pushes_only_a_lone_file() {
    let d = repo();
    seed(&d);
    // two matched files: a match is not intent, nothing pushes
    let out = bash(
        &d,
        "s1",
        "grep -rn x src",
        "src/a.rs:1:// x\nsrc/b.rs:1:// x\n",
    );
    assert!(out.is_empty(), "{out}");
    // every hit in one file: that file pushes
    let out = bash(
        &d,
        "s2",
        "grep -rn x src",
        "src/a.rs:1:// x\nsrc/a.rs:2:// x\n",
    );
    assert!(
        out.contains("login loops") && !out.contains("retry storms"),
        "{out}"
    );
    // the Grep tool: files_with_matches lists bare paths
    let out = search(
        &d,
        "s3",
        "Grep",
        r#"{"pattern":"x","path":"src"}"#,
        r#"{"mode":"files_with_matches","filenames":["src/b.rs"],"numFiles":1}"#,
    );
    assert!(
        out.contains("retry storms") && !out.contains("login loops"),
        "{out}"
    );
    // a file the grep names keeps pushing, whatever the hit list says
    let out = bash(
        &d,
        "s4",
        "grep -n x src/a.rs src/b.rs",
        "src/a.rs:1:// x\nsrc/b.rs:1:// x\n",
    );
    assert!(
        out.contains("login loops") && out.contains("retry storms"),
        "{out}"
    );
}

#[test]
fn a_hit_list_skips_data_files_but_a_named_one_counts() {
    let d = repo();
    seed(&d);
    std::fs::write(d.join("src/index.json"), "{}\n").unwrap();
    backdate(&d.join("src/index.json"));
    let (ok, _, err) = fael(
        &d,
        &[
            "add",
            "issue",
            "font index stale",
            "--files",
            "src/index.json",
        ],
        "",
    );
    assert!(ok, "{err}");
    let out = bash(
        &d,
        "s1",
        "grep -rn x src",
        "src/a.rs:1:// x\nsrc/index.json:1:x\n",
    );
    assert!(
        out.contains("login loops") && !out.contains("font index stale"),
        "{out}"
    );
    // a file the call names is intent, whatever its extension
    let out = bash(&d, "s2", "cat src/index.json", "{}");
    assert!(out.contains("font index stale"), "{out}");
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
fn a_wide_hit_list_pushes_nothing() {
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
    assert!(out.is_empty(), "{out}");
}

#[test]
fn codex_bash_payload_pushes_like_claude() {
    let d = repo();
    seed(&d);
    // Codex shell calls match as `Bash`: a `cat` read pushes the file's rows,
    // whether the response arrives as `stdout` or `output`.
    for (i, response) in [
        r#"{"stdout":""}"#.to_string(),
        r#"{"output":""}"#.to_string(),
    ]
    .into_iter()
    .enumerate()
    {
        let payload = format!(
            r#"{{"cwd":{},"session_id":"s{i}","tool_name":"Bash","tool_input":{{"command":"cat src/a.rs"}},"tool_response":{response}}}"#,
            json(&d)
        );
        let (ok, out, err) = fael(&d, &["hook", "search", "--client", "codex"], &payload);
        assert!(ok, "{err}");
        assert!(out.contains("login loops"), "{response}: {out}");
    }
}

#[test]
fn glob_pushes_the_files_it_names() {
    let d = repo();
    seed(&d);
    // Claude's `Glob`: the path counts when it is a file, the hit list the rest
    let out = search(
        &d,
        "s1",
        "Glob",
        r#"{"pattern":"src/*.rs","path":"src"}"#,
        r#"["src/a.rs"]"#,
    );
    assert!(out.contains("login loops"), "{out}");
    // OpenCode's lowercase `glob`, forwarded as a neutral raw call
    let out = search_neutral(
        &d,
        "s2",
        "glob",
        r#"{"pattern":"src/*.rs"}"#,
        r#"["src/b.rs"]"#,
    );
    assert!(out.contains("retry storms"), "{out}");
}

#[test]
fn neutral_search_resolves_opencode_tool_calls() {
    let d = repo();
    seed(&d);
    // `grep` with a hit list, lowercase like OpenCode sends it
    let out = search_neutral(
        &d,
        "s1",
        "grep",
        r#"{"pattern":"x","path":"src"}"#,
        r#"{"output":"src/a.rs:1:// x\n"}"#,
    );
    assert!(out.contains("login loops"), "{out}");
    // `bash` reading through the shell
    let out = search_neutral(
        &d,
        "s2",
        "bash",
        r#"{"command":"cat src/b.rs"}"#,
        r#""src/b.rs:1:// x\n""#,
    );
    assert!(out.contains("retry storms"), "{out}");
    // not a reader: nothing pushes (neutral still answers `{"block":false}`)
    let out = search_neutral(
        &d,
        "s3",
        "bash",
        r#"{"command":"cargo test src/a.rs"}"#,
        r#""src/a.rs:1: x""#,
    );
    assert!(!out.contains("context"), "{out}");
}

#[test]
fn a_shell_write_pushes_as_an_edit() {
    let d = repo();
    seed(&d);
    // the call ran: python rewrote a.rs, and only named b.rs in a string it never wrote
    std::fs::write(d.join("src/a.rs"), "// y\n").unwrap();
    let cmd =
        "python3 - <<'EOF'\np='src/a.rs'\nopen(p,'w').write(open(p).read())\nq=\"src/b.rs\"\nEOF";
    let out = bash(&d, "w1", cmd, "");
    // b.rs was named, not written — its row may still come through the
    // edit's same-directory tier, never as an edited file
    assert!(out.contains(r"fael mem for src/a.rs (1 of 2):\n"), "{out}");
    assert!(out.contains("login loops"), "{out}");
    assert!(
        out.contains("fael close"),
        "an edit carries the stale hint: {out}"
    );
    let usage = std::fs::read_to_string(state(&d).join("usage.jsonl")).unwrap();
    assert!(usage.contains(r#""event":"shell-edit""#), "{usage}");

    // a read of an old file is still a read: no stale hint
    let out = bash(&d, "w2", "cat src/b.rs", "");
    assert!(out.contains("retry storms"), "{out}");
    assert!(!out.contains("fael close"), "{out}");
}

#[test]
fn a_shell_write_and_read_push_both() {
    let d = repo();
    seed(&d);
    std::fs::write(d.join("src/a.rs"), "// y\n").unwrap();
    // outside src/: the edit's same-directory tier cannot reach it
    std::fs::write(d.join("c.rs"), "").unwrap();
    backdate(&d.join("c.rs"));
    let (ok, _, err) = fael(&d, &["add", "issue", "cache misses", "--files", "c.rs"], "");
    assert!(ok, "{err}");
    let out = bash(&d, "wr", "sed -i '' s/x/y/ src/a.rs && cat c.rs", "");
    let edit = out.find(r"fael mem for src/a.rs (1 of 2):\n").expect(&out);
    let hint = out.find("fael close").expect(&out);
    let read = out.find(r"fael mem for c.rs:\n").expect(&out);
    assert!(
        edit < hint && hint < read,
        "the edit block, its hint, then the read: {out}"
    );
    // one hint line (generic or ready close), the edit's: a read has none
    assert_eq!(
        out.matches("now in <file>").count(),
        1,
        "a read has no hint: {out}"
    );
}
