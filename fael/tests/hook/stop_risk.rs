//! Stop-event risk signals (chunk 7): Weak mentions never block — they join
//! the work block or surface once on the next push. Quoted code never
//! signals, transcripts scan past the latest user message only, and an issue
//! filed before the words does not clear them.

use super::{fael, fael_at, json, repo, repo_blocking, state};

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
    let read = format!(
        r#"{{"cwd":{},"session":"2020-01-01T00:00:00Z","files":["src/nothing.rs"]}}"#,
        json(&d)
    );
    let (ok, out, _) = fael(&d, &["hook", "read"], &read);
    assert!(
        ok && out.matches("possible problem").count() == 1 && out.contains("out of sync"),
        "{out}"
    );
    // shown once: the push after has nothing left
    let (ok, out, _) = fael(&d, &["hook", "read"], &read);
    assert!(ok && !out.contains("possible problem"), "{out}");
}

#[test]
fn stop_weak_risk_joins_work_block() {
    let d = repo_blocking();
    let (ok, _, err) = fael(
        &d,
        &["add", "decision", "old choice", "--files", "src/a.rs"],
        "",
    );
    assert!(ok, "{err}");
    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::write(d.join("src/b.rs"), "//\n").unwrap();
    let edit = format!(
        r#"{{"cwd":{},"session":"2020-01-01T00:00:00Z","files":["src/b.rs"]}}"#,
        json(&d)
    );
    assert!(fael(&d, &["hook", "edit"], &edit).0);

    // one block carrying both the edited file and the risk — not two blocks
    let stop = format!(
        r#"{{"cwd":{},"session":"2020-01-01T00:00:00Z","text":"config and schema are out of sync"}}"#,
        json(&d)
    );
    let (ok, out, _) = fael(&d, &["hook", "stop"], &stop);
    assert!(
        ok && out.contains("1 file(s) edited") && out.contains("out of sync"),
        "{out}"
    );
    let (ok, out, _) = fael(&d, &["hook", "stop"], &stop);
    assert!(ok && out.contains(r#""block":false"#), "{out}");
}

#[test]
fn stop_weak_work_block_does_not_consume_the_bug_slot() {
    let d = repo_blocking();
    let (ok, _, err) = fael(
        &d,
        &["add", "decision", "old choice", "--files", "src/a.rs"],
        "",
    );
    assert!(ok, "{err}");
    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::write(d.join("src/b.rs"), "//\n").unwrap();
    let edit = format!(
        r#"{{"cwd":{},"session":"2020-01-01T00:00:00Z","files":["src/b.rs"]}}"#,
        json(&d)
    );
    assert!(fael(&d, &["hook", "edit"], &edit).0);

    // a Weak mention rides the work block — recorded as work, not the bug slot
    let weak = format!(
        r#"{{"cwd":{},"session":"2020-01-01T00:00:00Z","text":"config and schema are out of sync"}}"#,
        json(&d)
    );
    let (ok, out, _) = fael(&d, &["hook", "stop"], &weak);
    assert!(
        ok && out.contains("1 file(s) edited") && out.contains("out of sync"),
        "{out}"
    );

    // the later real report must still block: the Weak block did not consume it
    let strong = format!(
        r#"{{"cwd":{},"session":"2020-01-01T00:00:00Z","text":"I found a bug in login"}}"#,
        json(&d)
    );
    let (ok, out, _) = fael(&d, &["hook", "stop"], &strong);
    assert!(ok && out.contains("fael add issue"), "{out}");
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
    let d = repo_blocking();
    let (ok, _, err) = fael(
        &d,
        &["add", "decision", "old choice", "--files", "src/a.rs"],
        "",
    );
    assert!(ok, "{err}");
    let base = fael_core::now_ms() + 120_000;

    // control: a lone bug report with no user message blocks
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
    let (ok, out, _) = fael(&d, &["hook", "stop"], &input);
    assert!(ok && out.contains("fael add issue"), "{out}");

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
    // a row for the silence itself: the work rule must let through on the
    // row, so the allow below proves the old planning words stayed quiet
    // (and not a stray init commit racing the session start at 1s granularity)
    let (ok, _, err) = fael(
        &d,
        &["add", "note", "renamed the helper", "--files", "src/a.rs"],
        "",
    );
    assert!(ok, "{err}");
    let input = format!(r#"{{"cwd":{},"session":{}}}"#, json(&d), json(&t));
    let s2 = state(&d).join("s2");
    let (ok, out, _) = fael_at(&s2, &d, &["hook", "stop"], &input);
    assert!(ok && out.contains(r#""block":false"#), "{out}");
}

#[test]
fn stop_issue_before_match_does_not_clear_signal() {
    let d = repo_blocking();
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
    let (ok, out, _) = fael(&d, &["hook", "stop"], &input);
    assert!(ok && out.contains("fael add issue"), "{out}");
}
