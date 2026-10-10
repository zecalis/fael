//! Grep/Bash/Read hooks: a shell call that wrote a file pushes that file's
//! rows as an edit; a read or a search — `Read`, `Grep`, `Glob`, `cat`, a hit
//! list — says nothing, on every client. Rows reach the agent at the edit.

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

    // a read of an old file says nothing: rows come at the edit
    let out = bash(&d, "w2", "cat src/b.rs", "");
    assert!(out.is_empty(), "{out}");
}

#[test]
fn a_shell_write_and_read_push_only_the_write() {
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
    assert!(edit < hint, "the edit block, then its hint: {out}");
    assert!(
        !out.contains("cache misses"),
        "the read says nothing: {out}"
    );
}

#[test]
fn codex_bash_payload_pushes_a_shell_write_like_claude() {
    let d = repo();
    seed(&d);
    // Codex shell calls match as `Bash`, whether the response arrives as
    // `stdout` or `output`; a `cat` beside the write says nothing.
    for (i, response) in [r#"{"stdout":""}"#, r#"{"output":""}"#]
        .into_iter()
        .enumerate()
    {
        std::fs::write(d.join("src/a.rs"), format!("// y{i}\n")).unwrap();
        let payload = format!(
            r#"{{"cwd":{},"session_id":"s{i}","tool_name":"Bash","tool_input":{{"command":"sed -i '' s/x/y/ src/a.rs; cat src/b.rs"}},"tool_response":{response}}}"#,
            json(&d)
        );
        let (ok, out, err) = fael(&d, &["hook", "search", "--client", "codex"], &payload);
        assert!(ok, "{err}");
        assert!(out.contains("login loops"), "{response}: {out}");
        assert!(!out.contains("retry storms"), "{response}: {out}");
    }
}

#[test]
fn neutral_search_resolves_opencode_shell_writes() {
    let d = repo();
    seed(&d);
    // `bash` writing through the shell, lowercase like OpenCode sends it
    std::fs::write(d.join("src/b.rs"), "// y\n").unwrap();
    let out = search_neutral(
        &d,
        "s1",
        "bash",
        r#"{"command":"sed -i '' s/x/y/ src/b.rs"}"#,
        r#""""#,
    );
    assert!(out.contains("retry storms"), "{out}");
    // a `grep`, `glob` or `bash` read pushes nothing (neutral still answers `{"block":false}`)
    for (i, (tool, input, response)) in [
        (
            "grep",
            r#"{"pattern":"x","path":"src"}"#,
            r#"{"output":"src/a.rs:1:// x\n"}"#,
        ),
        ("glob", r#"{"pattern":"src/*.rs"}"#, r#"["src/a.rs"]"#),
        ("bash", r#"{"command":"cat src/a.rs"}"#, r#""// x\n""#),
    ]
    .into_iter()
    .enumerate()
    {
        let out = search_neutral(&d, &format!("r{i}"), tool, input, response);
        assert!(
            !out.contains("context") && !out.contains("login loops"),
            "{tool}: {out}"
        );
    }
}

/// Rows reach the agent at the edit, at kickoff and in plan briefs — a read
/// or a search of a file with an open row says nothing, on every client.
#[test]
fn a_read_or_a_search_says_nothing() {
    let d = repo();
    seed(&d);
    // Claude `Read`
    let read = serde_json::json!({"cwd": d, "session_id": "c1", "tool_name": "Read",
        "tool_input": {"file_path": d.join("src/a.rs")}, "tool_response": {}})
    .to_string();
    let (ok, out, err) = fael(&d, &["hook", "read", "--client", "claude"], &read);
    assert!(ok, "{err}");
    assert!(out.is_empty(), "claude read: {out}");
    // Claude `Grep` naming the one file, and `Glob`
    let out = search(
        &d,
        "c2",
        "Grep",
        r#"{"pattern":"x","path":"src/a.rs"}"#,
        r#"{"mode":"files_with_matches","filenames":["src/a.rs"],"numFiles":1}"#,
    );
    assert!(out.is_empty(), "claude grep: {out}");
    let out = search(
        &d,
        "c3",
        "Glob",
        r#"{"pattern":"src/*.rs"}"#,
        r#"["src/a.rs"]"#,
    );
    assert!(out.is_empty(), "claude glob: {out}");
    // shell reads: `cat`, a lone-file grep, `git show`
    for (i, (cmd, stdout)) in [
        ("cat src/a.rs", "// x\n"),
        ("grep -rn x src", "src/a.rs:1:// x\n"),
        ("git show HEAD:src/a.rs", ""),
    ]
    .into_iter()
    .enumerate()
    {
        let out = bash(&d, &format!("b{i}"), cmd, stdout);
        assert!(out.is_empty(), "{cmd}: {out}");
    }
    // the neutral client's `read` and a `search` naming its files
    for (i, event) in ["read", "search"].into_iter().enumerate() {
        let input = format!(
            r#"{{"cwd":{},"session":"n{i}","files":["src/a.rs"]}}"#,
            json(&d)
        );
        let (ok, out, err) = fael(&d, &["hook", event], &input);
        assert!(ok, "{err}");
        assert!(!out.contains("login loops"), "neutral {event}: {out}");
    }
    // nothing said, nothing recorded as said
    let usage = std::fs::read_to_string(state(&d).join("usage.jsonl")).unwrap_or_default();
    assert!(
        !usage.contains(r#""event":"read""#) && !usage.contains(r#""event":"search""#),
        "{usage}"
    );
}
