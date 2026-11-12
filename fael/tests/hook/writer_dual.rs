//! Dual client env: newest hook record wins (`CLAUDE_CODE_SESSION_ID` +
use super::writer::log_text;
/// `CODEX_THREAD_ID`), row carries its client. Split from writer.rs at the
/// 400-line ratchet.
use super::{fael_env, json, repo};

/// Dual env: newest hook record wins, row carries its client.
#[test]
fn dual_env_picks_the_newest_hook_record_and_its_client() {
    for (first, second, want_session, want_client) in [
        ("claude", "codex", "thread-codex-9", "codex"),
        ("codex", "claude", "abc-999", "claude"),
    ] {
        let d = repo();
        std::fs::write(d.join("src/a.rs"), "//\n").unwrap();
        let (ok, _, err) = fael_env(&d, &["add", "note", "seed", "--files", "src/a.rs"], "", &[]);
        assert!(ok, "{err}");
        let claude_id = "abc-999";
        let codex_id = "thread-codex-9";
        let edit = |client: &str, id: &str| {
            let input = if client == "claude" {
                let transcript = d.join(format!("t/{id}.jsonl"));
                format!(
                    r#"{{"cwd":{},"session_id":"x","transcript_path":{},"tool_input":{{"file_path":{}}}}}"#,
                    json(&d),
                    json(&transcript),
                    json(&d.join("src/a.rs"))
                )
            } else {
                let command = format!("*** Update File: {}", d.join("src/a.rs").display());
                serde_json::json!({
                    "cwd": d.to_string_lossy(),
                    "session_id": id,
                    "tool_input": {"command": command},
                })
                .to_string()
            };
            let (ok, _, err) = fael_env(&d, &["hook", "edit", "--client", client], &input, &[]);
            assert!(ok, "{err}");
        };
        edit(
            first,
            if first == "claude" {
                claude_id
            } else {
                codex_id
            },
        );
        std::thread::sleep(std::time::Duration::from_millis(1100));
        edit(
            second,
            if second == "claude" {
                claude_id
            } else {
                codex_id
            },
        );
        let (ok, _, err) = fael_env(
            &d,
            &["add", "note", "dual", "--files", "src/a.rs"],
            "",
            &[
                ("CLAUDE_CODE_SESSION_ID", claude_id),
                ("CODEX_THREAD_ID", codex_id),
            ],
        );
        assert!(ok, "{err}");
        let log = log_text(&d);
        assert!(
            log.contains(&format!(r#""session":"{want_session}""#)),
            "{log}"
        );
        assert!(
            log.contains(&format!(r#""client":"{want_client}""#)),
            "{log}"
        );
    }
}

/// Single var keeps today's order with client.
#[test]
fn single_var_keeps_todays_order_with_client() {
    let d = repo();
    std::fs::write(d.join("src/a.rs"), "//\n").unwrap();
    let (ok, _, err) = fael_env(&d, &["add", "note", "seed", "--files", "src/a.rs"], "", &[]);
    assert!(ok, "{err}");
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
    assert!(
        log.contains(r#""session":"abc-123""#) && log.contains(r#""client":"claude""#),
        "{log}"
    );
}
