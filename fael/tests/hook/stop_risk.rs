//! Stop-event bug and risk signals: never a block — the line surfaces once on
//! the next push. Quoted code never signals, transcripts scan past the latest
//! user prompt only (tool results are not one), the client's reply backs
//! a lagging transcript, and an issue filed before the words does not clear them.

use super::{fael, flagged, json, repo};

#[test]
fn stop_weak_risk_never_blocks_shows_once_on_next_push() {
    let d = repo();
    let (ok, _, err) = fael(
        &d,
        &["add", "decision", "old choice", "--files", "src/a.rs"],
        "",
    );
    assert!(ok, "{err}");

    // a risk mention alone: no block, but stashed for the next push
    let stop = format!(
        r#"{{"cwd":{},"session":"2020-01-01T00:00:00Z","text":"the schema and the docs are out of sync"}}"#,
        json(&d)
    );
    let (ok, out, _) = fael(&d, &["hook", "stop"], &stop);
    assert!(ok && out.contains(r#""block":false"#), "{out}");

    // the next push carries exactly one warning line, even with no rows
    let edit = format!(
        r#"{{"cwd":{},"session":"2020-01-01T00:00:00Z","files":["src/nothing.rs"]}}"#,
        json(&d)
    );
    let (ok, out, _) = fael(&d, &["hook", "edit"], &edit);
    assert!(
        ok && out.matches("possible problem").count() == 1 && out.contains("out of sync"),
        "{out}"
    );
    // shown once: the push after has nothing left
    let (ok, out, _) = fael(&d, &["hook", "edit"], &edit);
    assert!(ok && !out.contains("possible problem"), "{out}");
}

#[test]
fn stop_ignores_markers_in_code_and_quotes() {
    let d = repo();
    let (ok, _, err) = fael(
        &d,
        &["add", "decision", "old choice", "--files", "src/a.rs"],
        "",
    );
    assert!(ok, "{err}");
    for text in [
        "wrote ```\nI found a bug in login\n``` for the docs",
        "run `found a bug` to reproduce",
        "> I found a bug in login",
        "> config and schema are out of sync",
    ] {
        let input = format!(
            r#"{{"cwd":{},"session":"2020-01-01T00:00:00Z","text":{}}}"#,
            json(&d),
            serde_json::Value::String(text.into())
        );
        let (ok, out, _) = fael(&d, &["hook", "stop"], &input);
        assert!(ok && out.contains(r#""block":false"#), "{text}: {out}");
        assert!(!flagged(&d, r#""2020-01-01T00:00:00Z""#), "{text}");
    }
}

/// A transcript line timestamp in the future keeps the test green on any OS:
/// birthtime and mtime are always older, so every line passes the recency cut.
fn future_ts(base_ms: u64, secs_after: u64) -> String {
    fael_core::rfc3339(base_ms + secs_after * 1000)
}

fn transcript_line(role: &str, text: &str, ts: &str) -> String {
    serde_json::to_string(&serde_json::json!({
        "message": {"role": role, "content": [{"type": "text", "text": text}]},
        "timestamp": ts,
    }))
    .unwrap()
}

#[test]
fn stop_reads_transcript_only_after_latest_user_message() {
    let d = repo();
    let (ok, _, err) = fael(
        &d,
        &["add", "decision", "old choice", "--files", "src/a.rs"],
        "",
    );
    assert!(ok, "{err}");
    let base = fael_core::now_ms() + 120_000;

    // control: a lone bug report with no user message is flagged
    let alone = d.join("alone.jsonl");
    std::fs::write(
        &alone,
        transcript_line(
            "assistant",
            "I found a bug in the old plan",
            &future_ts(base, 0),
        ),
    )
    .unwrap();
    let input = format!(r#"{{"cwd":{},"session":{}}}"#, json(&d), json(&alone));
    assert!(fael(&d, &["hook", "stop"], &input).0);
    assert!(flagged(&d, &json(&alone)));

    // the same words before the latest user message are last turn's planning
    let t = d.join("t.jsonl");
    std::fs::write(
        &t,
        [
            transcript_line(
                "assistant",
                "I found a bug in the old plan",
                &future_ts(base, 0),
            ),
            transcript_line(
                "user",
                "thanks, next: rename the helper",
                &future_ts(base, 1),
            ),
            transcript_line("assistant", "renamed, all tests pass", &future_ts(base, 2)),
        ]
        .join("\n"),
    )
    .unwrap();
    let input = format!(r#"{{"cwd":{},"session":{}}}"#, json(&d), json(&t));
    assert!(fael(&d, &["hook", "stop"], &input).0);
    assert!(!flagged(&d, &json(&t)));
}

#[test]
fn stop_issue_before_match_does_not_clear_signal() {
    let d = repo();
    let (ok, _, err) = fael(
        &d,
        &["add", "decision", "old choice", "--files", "src/a.rs"],
        "",
    );
    assert!(ok, "{err}");
    let base = fael_core::now_ms() + 120_000;

    // the report lands after the issue row: the older issue must not clear it
    let t = d.join("t.jsonl");
    std::fs::write(
        &t,
        transcript_line("assistant", "I found a bug in login", &future_ts(base, 0)),
    )
    .unwrap();
    let (ok, _, err) = fael(
        &d,
        &[
            "add",
            "issue",
            "login loops on retry",
            "--files",
            "src/a.rs",
        ],
        "",
    );
    assert!(ok, "{err}");
    let input = format!(r#"{{"cwd":{},"session":{}}}"#, json(&d), json(&t));
    assert!(fael(&d, &["hook", "stop"], &input).0);
    assert!(flagged(&d, &json(&t)));
}

/// Claude can fire Stop before the final reply reaches the transcript (vela
/// session 9707ed35, line 1924): the client's `reply` is read when the file
/// has no match.
#[test]
fn stop_reads_reply_when_transcript_lags() {
    let d = repo();
    let (ok, _, err) = fael(
        &d,
        &["add", "decision", "old choice", "--files", "src/a.rs"],
        "",
    );
    assert!(ok, "{err}");
    let base = fael_core::now_ms() + 120_000;
    let t = d.join("t.jsonl");
    std::fs::write(
        &t,
        transcript_line("user", "ตรวจหน้าเว็บให้หน่อย", &future_ts(base, 0)),
    )
    .unwrap();
    let reply = "**รอบแรกเจอบั๊กจริง 3 จุด และแก้แล้วใน #283**";
    let input = format!(
        r#"{{"cwd":{},"session":{},"reply":{}}}"#,
        json(&d),
        json(&t),
        serde_json::Value::String(reply.into())
    );
    assert!(fael(&d, &["hook", "stop"], &input).0);
    assert!(flagged(&d, &json(&t)));
}

/// A tool result is a user line, not a new turn: words said before the
/// turn's last tool call still count.
#[test]
fn stop_scans_past_tool_results() {
    let d = repo();
    let (ok, _, err) = fael(
        &d,
        &["add", "decision", "old choice", "--files", "src/a.rs"],
        "",
    );
    assert!(ok, "{err}");
    let base = fael_core::now_ms() + 120_000;
    let tool_result = serde_json::to_string(&serde_json::json!({
        "message": {"role": "user", "content": [
            {"type": "tool_result", "tool_use_id": "t1", "content": "ok"}
        ]},
        "timestamp": future_ts(base, 2),
    }))
    .unwrap();
    let t = d.join("t.jsonl");
    std::fs::write(
        &t,
        [
            transcript_line("user", "rename the helper", &future_ts(base, 0)),
            transcript_line("assistant", "I found a bug in login", &future_ts(base, 1)),
            tool_result,
            transcript_line("assistant", "renamed, tests pass", &future_ts(base, 3)),
        ]
        .join("\n"),
    )
    .unwrap();
    let input = format!(r#"{{"cwd":{},"session":{}}}"#, json(&d), json(&t));
    assert!(fael(&d, &["hook", "stop"], &input).0);
    assert!(flagged(&d, &json(&t)));
}
