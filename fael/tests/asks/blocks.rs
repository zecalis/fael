//! Stop-hook blocks land with their ask type and session, transcript `usage`
//! lands as `real_tokens`, and `stats` joins blocks to the round after them.

use super::{fael, json, repo_blocking, stats_json, usage};
use std::path::PathBuf;
use std::process::Command;

fn commit(d: &std::path::Path, msg: &str) {
    std::fs::write(d.join("src/a.rs"), format!("// {msg}\n")).unwrap();
    assert!(
        Command::new("git")
            .args(["add", "-A"])
            .current_dir(d)
            .status()
            .unwrap()
            .success()
    );
    assert!(
        Command::new("git")
            .args(["commit", "-q", "-m", msg])
            .current_dir(d)
            .status()
            .unwrap()
            .success()
    );
}

/// A transcript file born strictly after the previous row, so the work after
/// it reads as this session's. With `usage`: Claude-shaped assistant lines.
fn transcript(d: &std::path::Path, name: &str, usage: &[(u64, u64, u64, u64)]) -> PathBuf {
    std::thread::sleep(std::time::Duration::from_millis(5));
    let p = d.join(name);
    let mut s = String::new();
    for (i, o, c, r) in usage {
        s.push_str(
            &serde_json::json!({"type": "assistant",
                "message": {"usage": {"input_tokens": i, "cache_creation_input_tokens": o,
                    "cache_read_input_tokens": c, "output_tokens": r}}})
            .to_string(),
        );
        s.push('\n');
    }
    std::fs::write(&p, s).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(1100));
    p
}

fn stop(d: &std::path::Path, input: &str) -> (bool, String, String) {
    fael(d, &["hook", "stop", "--client", "claude"], input)
}

#[test]
fn stop_block_records_ask_and_session() {
    let d = repo_blocking();
    let (ok, _, err) = fael(
        &d,
        &[
            "add",
            "decision",
            "old",
            "--files",
            "doc:seed",
            "--key",
            "test:seed",
        ],
        "",
    );
    assert!(ok, "{err}");
    let t = transcript(&d, "t1.jsonl", &[]);
    commit(&d, "work without a row");
    let input = format!(r#"{{"cwd":{},"transcript_path":{}}}"#, json(&d), json(&t));
    let (ok, out, _) = stop(&d, &input);
    assert!(ok && out.contains(r#""decision":"block""#), "{out}");
    let u = usage(&d);
    assert_eq!(u.len(), 1, "{u:?}");
    assert_eq!(u[0]["ask"], "stop-block", "{u:?}");
    assert_eq!(u[0]["event"], "stop-work", "{u:?}");
    assert_eq!(u[0]["session"], t.to_string_lossy().as_ref(), "{u:?}");
    assert!(u[0].get("real_tokens").is_none(), "{u:?}");
    let v = stats_json(&d);
    assert_eq!(v["asks"]["stop-block"]["events"], 1, "{v}");
    assert_eq!(v["repeat_blocks"], 0, "{v}");
    assert!(v.get("real_tokens").is_none(), "{v}");
}

#[test]
fn transcript_usage_lands_on_block_row() {
    let d = repo_blocking();
    let (ok, _, err) = fael(
        &d,
        &[
            "add",
            "decision",
            "old",
            "--files",
            "doc:seed",
            "--key",
            "test:seed",
        ],
        "",
    );
    assert!(ok, "{err}");
    let t = transcript(&d, "t1.jsonl", &[(100, 20000, 30000, 50)]);
    commit(&d, "work without a row");
    let input = format!(r#"{{"cwd":{},"transcript_path":{}}}"#, json(&d), json(&t));
    let (ok, out, _) = stop(&d, &input);
    assert!(ok && out.contains(r#""decision":"block""#), "{out}");
    let u = usage(&d);
    let real = &u[0]["real_tokens"];
    assert_eq!(real["input_tokens"], 100, "{u:?}");
    assert_eq!(real["cache_creation_input_tokens"], 20000, "{u:?}");
    assert_eq!(real["cache_read_input_tokens"], 30000, "{u:?}");
    assert_eq!(real["output_tokens"], 50, "{u:?}");
}

#[test]
fn post_block_round_cost_joins_block_to_next_push() {
    let d = repo_blocking();
    let (ok, _, err) = fael(
        &d,
        &[
            "add",
            "decision",
            "old",
            "--files",
            "doc:seed",
            "--key",
            "test:seed",
        ],
        "",
    );
    assert!(ok, "{err}");
    // the round that caused the block cost 100 in / 50 out …
    let t = transcript(&d, "t1.jsonl", &[(100, 20000, 30000, 50)]);
    commit(&d, "work without a row");
    let input = format!(r#"{{"cwd":{},"transcript_path":{}}}"#, json(&d), json(&t));
    let (ok, out, _) = stop(&d, &input);
    assert!(ok && out.contains(r#""decision":"block""#), "{out}");
    // … the agent files the row, reads back, and the read's round cost 10/5
    let (ok, _, err) = fael(
        &d,
        &["add", "note", "did the work", "--files", "src/a.rs"],
        "",
    );
    assert!(ok, "{err}");
    std::fs::write(
        &t,
        serde_json::json!({"type": "assistant",
            "message": {"usage": {"input_tokens": 10, "cache_creation_input_tokens": 0,
                "cache_read_input_tokens": 0, "output_tokens": 5}}})
        .to_string(),
    )
    .unwrap();
    let read = format!(
        r#"{{"cwd":{},"transcript_path":{},"tool_input":{{"file_path":{}}}}}"#,
        json(&d),
        json(&t),
        json(&d.join("src/a.rs"))
    );
    let (ok, _, _) = fael(&d, &["hook", "read", "--client", "claude"], &read);
    assert!(ok);
    let v = stats_json(&d);
    let real = &v["real_tokens"];
    assert_eq!(real["post_block_rounds"], 1, "{v}");
    assert_eq!(real["avg_input"], 10, "{v}");
    assert_eq!(real["avg_output"], 5, "{v}");
    let (ok, out, err) = fael(&d, &["stats"], "");
    assert!(ok, "{err}");
    assert!(out.contains("post-block rounds: 1 sample(s)"), "{out}");
}

#[test]
fn repeat_block_counts_block_after_block_before_any_row() {
    let d = repo_blocking();
    let (ok, _, err) = fael(
        &d,
        &[
            "add",
            "decision",
            "old",
            "--files",
            "doc:seed",
            "--key",
            "test:seed",
        ],
        "",
    );
    assert!(ok, "{err}");
    let t = transcript(&d, "t1.jsonl", &[]);
    commit(&d, "work without a row");
    let input = format!(r#"{{"cwd":{},"transcript_path":{}}}"#, json(&d), json(&t));
    let (ok, out, _) = stop(&d, &input);
    assert!(ok && out.contains(r#""decision":"block""#), "{out}");
    // no row filed — but now the turn also reports a bug: for `--client
    // claude` the words come from the transcript, so append an assistant
    // line (no timestamp: it defaults to the session start, which is kept);
    // a second block of another kind in the same session, still before any row
    std::fs::write(
        &t,
        serde_json::json!({"type": "assistant",
            "message": {"role": "assistant",
                "content": [{"type": "text", "text": "I found a bug in login"}]}})
        .to_string(),
    )
    .unwrap();
    let (ok, out, _) = stop(&d, &input);
    assert!(ok && out.contains(r#""decision":"block""#), "{out}");
    let v = stats_json(&d);
    assert_eq!(v["asks"]["stop-block"]["events"], 2, "{v}");
    assert_eq!(v["repeat_blocks"], 1, "{v}");
}
