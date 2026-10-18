//! r1: retire rows stamp where the writing session was shown the row —
//! `extra.shown` is a captured fact (`held`, `done`), never inferred.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const HELD: &str = "changed since it was written";

fn fael(d: &Path, s: &Path, session: Option<&str>, args: &[&str]) -> String {
    let mut c = Command::new(env!("CARGO_BIN_EXE_fael"));
    c.args(args).current_dir(d).env("FAEL_STATE_DIR", s);
    c.env_remove("FAEL_DIR")
        .env_remove("FAEL_SESSION")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env_remove("CODEX_THREAD_ID");
    if let Some(v) = session {
        c.env("FAEL_SESSION", v);
    }
    let o = c.output().unwrap();
    assert!(
        o.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

fn git(d: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@t"])
        .args(args)
        .current_dir(d)
        .status()
        .unwrap();
    assert!(ok.success(), "git {args:?}");
}

fn repo() -> (PathBuf, PathBuf) {
    let t = std::env::temp_dir().join(format!("fael-shown-{}", fael_core::ulid()));
    let d = t.join("r");
    std::fs::create_dir_all(d.join("src")).unwrap();
    git(&d, &["init", "-q", "-b", "main"]);
    git(&d, &["commit", "-q", "--allow-empty", "-m", "init"]);
    std::fs::write(d.join("src/a.rs"), "fn x() {}\n").unwrap();
    std::fs::write(d.join("src/b.rs"), "fn y() {}\n").unwrap();
    std::fs::write(d.join("src/c.rs"), "fn z() {}\n").unwrap();
    (d.canonicalize().unwrap(), t.join("state"))
}

fn hook(d: &Path, s: &Path, event: &str, session: &str, files: &[&str]) -> String {
    let mut c = Command::new(env!("CARGO_BIN_EXE_fael"));
    c.args(["hook", event])
        .current_dir(d)
        .env("FAEL_STATE_DIR", s)
        .env_remove("FAEL_DIR")
        .env_remove("FAEL_SESSION")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env_remove("CODEX_THREAD_ID")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped());
    let mut c = c.spawn().unwrap();
    let files_json = serde_json::json!(files);
    let e = match event {
        "stop" => serde_json::json!({"cwd": d, "session": session, "text": "done"}),
        _ => serde_json::json!({"cwd": d, "session": session, "files": files_json}),
    };
    c.stdin
        .take()
        .unwrap()
        .write_all(e.to_string().as_bytes())
        .unwrap();
    let o = c.wait_with_output().unwrap();
    assert!(o.status.success());
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn close_row(d: &Path, target: &str) -> serde_json::Value {
    let log = d.join(".git/fael/log");
    let mut stack = vec![log];
    while let Some(p) = stack.pop() {
        for e in std::fs::read_dir(&p).into_iter().flatten().flatten() {
            let q = e.path();
            if q.is_dir() {
                stack.push(q);
            } else if let Ok(t) = std::fs::read_to_string(&q) {
                for line in t.lines() {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
                        let is_close = v["ref"].as_str() == Some(target)
                            || v["bumps"].as_str() == Some(target)
                            || v["supersedes"].as_str() == Some(target);
                        if is_close {
                            return v;
                        }
                    }
                }
            }
        }
    }
    panic!("no retire row for {target}");
}

fn hold(d: &Path, s: &Path, session: &str) -> String {
    let out = fael(
        d,
        s,
        Some(session),
        &["add", "decision", "held rule", "--files", "src/a.rs"],
    );
    let id = out.split_whitespace().next().unwrap().to_string();
    std::fs::write(d.join("src/a.rs"), "fn x() { 1 }\n").unwrap();
    hook(d, s, "edit", session, &["src/a.rs"]);
    hook(d, s, "stop", session, &[]);
    let out = hook(d, s, "edit", session, &["src/c.rs"]);
    assert!(out.contains(HELD), "{out}");
    id
}

#[test]
fn a_held_close_stamps_held_in_the_same_session() {
    let (d, s) = repo();
    let full = hold(&d, &s, "S1");
    fael(&d, &s, Some("S1"), &["close", &full, "now in src/a.rs"]);
    let row = close_row(&d, &full);
    assert_eq!(row["shown"], serde_json::json!(["held"]), "{row}");
}

#[test]
fn the_same_close_from_another_session_stamps_no_shown() {
    let (d, s) = repo();
    let full = hold(&d, &s, "S1");
    fael(&d, &s, Some("S2"), &["close", &full, "other session"]);
    let row = close_row(&d, &full);
    assert!(row.get("shown").is_none(), "{row}");
}

#[test]
fn a_done_bump_stamps_done() {
    let (d, s) = repo();
    git(&d, &["add", "src/a.rs", "src/b.rs"]);
    git(&d, &["commit", "-qm", "add ab"]);
    git(&d, &["update-ref", "refs/remotes/origin/main", "main"]);
    git(
        &d,
        &[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/main",
        ],
    );
    git(&d, &["switch", "-qc", "feat/x"]);
    let full = add_id(
        &d,
        &s,
        Some("SD"),
        &[
            "add",
            "decision",
            "two-file rule",
            "--files",
            "src/a.rs,src/b.rs",
        ],
    );
    let chunk = fael(&d, &s, Some("SD"), &["chunk", "add", "c1", "--brief", "b1"])
        .split_whitespace()
        .next()
        .unwrap()
        .to_string();
    fael(
        &d,
        &s,
        Some("SD"),
        &["chunk", "start", &chunk, "--run", "R1"],
    );
    std::fs::write(d.join("src/a.rs"), "changed\n").unwrap();
    git(&d, &["add", "src/a.rs"]);
    git(&d, &["commit", "-qm", "change a"]);
    let out = fael(
        &d,
        &s,
        Some("SD"),
        &["chunk", "done", &chunk, "h", "--run", "R1"],
    );
    assert!(out.contains(&full[..8]), "{out}");
    fael(&d, &s, Some("SD"), &["bump", &full]);
    let row = close_row(&d, &full);
    assert_eq!(row["shown"], serde_json::json!(["done"]), "{row}");
}

fn mcp_close(d: &Path, s: &Path, session: &str, id: &str) -> String {
    let mut c = Command::new(env!("CARGO_BIN_EXE_fael"));
    c.arg("mcp")
        .current_dir(d)
        .env("FAEL_STATE_DIR", s)
        .env_remove("FAEL_SESSION")
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env_remove("CODEX_THREAD_ID")
        .env("CLAUDE_CODE_SESSION_ID", session)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped());
    let mut c = c.spawn().unwrap();
    let call = serde_json::json!({"jsonrpc": "2.0", "id": 0, "method": "tools/call",
        "params": {"name": "close", "arguments": {"id": id, "text": "now in src/a.rs", "cwd": d}}});
    c.stdin
        .take()
        .unwrap()
        .write_all((call.to_string() + "\n").as_bytes())
        .unwrap();
    let o = c.wait_with_output().unwrap();
    assert!(o.status.success());
    String::from_utf8_lossy(&o.stdout).into_owned()
}

#[test]
fn an_mcp_close_after_held_stamps_held() {
    let (d, s) = repo();
    let full = hold(&d, &s, "SM");
    let out = mcp_close(&d, &s, "SM", &full);
    assert!(out.contains("recorded"), "{out}");
    let row = close_row(&d, &full);
    assert_eq!(row["shown"], serde_json::json!(["held"]), "{row}");
}

/// Every `<source>\t<id>` line in the state dir's shown files.
fn shown_text(s: &Path) -> String {
    let mut out = String::new();
    for e in std::fs::read_dir(s.join("sessions"))
        .into_iter()
        .flatten()
        .flatten()
    {
        let p = e.path();
        if p.extension().is_some_and(|x| x == "shown") {
            out += &std::fs::read_to_string(&p).unwrap_or_default();
        }
    }
    out
}

/// f2: a held line the push budget cut was never shown, so nothing is stamped
/// from it; once it fits, it is said and stamped.
#[test]
fn a_budget_cut_held_line_stamps_nothing() {
    let (d, s) = repo();
    let id = add_id(
        &d,
        &s,
        Some("S1"),
        &["add", "decision", "cut rule", "--files", "src/a.rs"],
    );
    std::fs::write(d.join("src/a.rs"), "fn x() { 1 }\n").unwrap();
    hook(&d, &s, "edit", "S1", &["src/a.rs"]);
    hook(&d, &s, "stop", "S1", &[]);
    // too small for any line: the held line is offered, then dropped
    std::fs::create_dir_all(d.join(".fael")).unwrap();
    std::fs::write(d.join(".fael/config.toml"), "[budget]\npush_tokens = 1\n").unwrap();
    let out = hook(&d, &s, "edit", "S1", &["src/c.rs"]);
    assert!(!out.contains(HELD), "nothing was said: {out}");
    assert!(
        !shown_text(&s).contains("held\t"),
        "a cut line stamps nothing: {}",
        shown_text(&s)
    );
    // room again: the same push says it and stamps it
    std::fs::write(d.join(".fael/config.toml"), "[budget]\npush_tokens = 800\n").unwrap();
    let out = hook(&d, &s, "edit", "S1", &["src/c.rs"]);
    assert!(out.contains(HELD), "{out}");
    assert!(
        shown_text(&s).contains(&format!("held\t{id}")),
        "{}",
        shown_text(&s)
    );
}

/// The full id `add` prints as its first line.
fn add_id(d: &Path, s: &Path, session: Option<&str>, args: &[&str]) -> String {
    fael(d, s, session, args)
        .split_whitespace()
        .next()
        .unwrap()
        .to_string()
}

/// f1: a chain close files one close row per version, but only the requested
/// row is the one the Held line named — an older version was never shown.
#[test]
fn a_chain_close_stamps_shown_on_the_requested_row_only() {
    let (d, s) = repo();
    let a = add_id(
        &d,
        &s,
        Some("S1"),
        &[
            "add",
            "decision",
            "old rule A",
            "--files",
            "src/a.rs",
            "--key",
            "k:t",
        ],
    );
    let b = add_id(
        &d,
        &s,
        Some("S1"),
        &[
            "add",
            "decision",
            "new rule B",
            "--files",
            "src/a.rs",
            "--key",
            "k:t",
            "--supersedes",
            &a,
        ],
    );
    std::fs::write(d.join("src/a.rs"), "fn x() { 1 }\n").unwrap();
    hook(&d, &s, "edit", "S1", &["src/a.rs"]);
    hook(&d, &s, "stop", "S1", &[]);
    let out = hook(&d, &s, "edit", "S1", &["src/c.rs"]);
    assert!(out.contains(HELD), "{out}");
    fael(&d, &s, Some("S1"), &["close", &b, "now in src/a.rs"]);
    let old = close_row(&d, &a);
    assert!(
        old.get("shown").is_none(),
        "the older version was never itself shown: {old}"
    );
    let new = close_row(&d, &b);
    assert_eq!(new["shown"], serde_json::json!(["held"]), "{new}");
}

/// `fael` with a Claude-shaped session: the CLI reads `$CLAUDE_CODE_SESSION_ID`.
fn fael_claude(d: &Path, s: &Path, uuid: &str, args: &[&str]) -> String {
    let mut c = Command::new(env!("CARGO_BIN_EXE_fael"));
    c.args(args)
        .current_dir(d)
        .env("FAEL_STATE_DIR", s)
        .env("CLAUDE_CODE_SESSION_ID", uuid)
        .env_remove("FAEL_DIR")
        .env_remove("FAEL_SESSION")
        .env_remove("CODEX_THREAD_ID");
    let o = c.output().unwrap();
    assert!(
        o.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

/// f3: the push speaks a Claude transcript path while the CLI speaks its stem
/// — the shown key must bridge the two, or nothing is stamped on the client
/// where most rows are filed.
#[test]
fn a_claude_transcript_push_and_a_stem_close_meet() {
    let (d, s) = repo();
    let uuid = "abc-123-xyz";
    let tr = d.join("t").join(format!("{uuid}.jsonl"));
    let full = add_id(
        &d,
        &s,
        None,
        &["add", "decision", "claude shaped", "--files", "src/a.rs"],
    );
    std::fs::write(d.join("src/a.rs"), "fn x() { 1 }\n").unwrap();
    hook(&d, &s, "edit", tr.to_str().unwrap(), &["src/a.rs"]);
    hook(&d, &s, "stop", tr.to_str().unwrap(), &[]);
    let out = hook(&d, &s, "edit", tr.to_str().unwrap(), &["src/c.rs"]);
    assert!(out.contains(HELD), "{out}");
    fael_claude(&d, &s, uuid, &["close", &full, "now in src/a.rs"]);
    let row = close_row(&d, &full);
    assert_eq!(row["shown"], serde_json::json!(["held"]), "{row}");
    assert_eq!(row["session"], serde_json::json!(uuid), "{row}");
}
