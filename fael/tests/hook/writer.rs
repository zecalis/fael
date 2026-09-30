//! A row records the session that filed it (`session`), so a later push can tell
//! "written by session A, used by session B" from "written and used by A".

use super::{fael_env, json, repo};

fn log_text(d: &std::path::Path) -> String {
    let mut out = String::new();
    let mut stack = vec![d.join(".fael/log")];
    while let Some(p) = stack.pop() {
        for e in std::fs::read_dir(&p).unwrap().flatten() {
            let q = e.path();
            if q.is_dir() {
                stack.push(q);
            } else {
                out += &std::fs::read_to_string(q).unwrap();
            }
        }
    }
    out
}

#[test]
fn a_row_carries_the_writer_session_id_never_a_path() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "//\n").unwrap();
    // the edit hook keys the session by transcript path; the env holds the stem
    let transcript = d.join("t/abc-123.jsonl");
    let input = format!(
        r#"{{"cwd":{},"session_id":"x","transcript_path":{},"tool_input":{{"file_path":{}}}}}"#,
        json(&d),
        json(&transcript),
        json(&d.join("src/a.rs"))
    );
    let (ok, _, err) = fael_env(&d, &["hook", "edit", "--client", "claude"], &input, &[]);
    assert!(ok, "{err}");
    let (ok, _, err) = fael_env(
        &d,
        &["add", "note", "first", "--files", "src/a.rs"],
        "",
        &[("CLAUDE_CODE_SESSION_ID", "abc-123")],
    );
    assert!(ok, "{err}");
    let log = log_text(&d);
    assert!(log.contains(r#""session":"abc-123""#), "{log}");
    assert!(!log.contains("/t/"), "no local path in a shared row: {log}");
}

#[test]
fn outside_a_session_a_row_has_no_session() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "//\n").unwrap();
    let mut c = std::process::Command::new(env!("CARGO_BIN_EXE_fael"));
    let o = c
        .args(["add", "note", "first", "--files", "src/a.rs"])
        .current_dir(&d)
        .env("FAEL_STATE_DIR", d.join("state"))
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .output()
        .unwrap();
    assert!(o.status.success());
    assert!(!log_text(&d).contains("session"));
}
