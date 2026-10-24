//! Run-telemetry t4: Claude/Codex model parity — the Stop hook stamps the
//! transcript's client-reported model onto the worktree's live run (NULL
//! fills only: a start-time model always wins), and usage rows stay
//! model-free (usage.jsonl byte-shape unchanged, Stats untouched).

use super::{fael, json, repo};
use std::path::Path;

fn crew_db(d: &Path) -> std::path::PathBuf {
    d.join(".git/fael/crew.db")
}

/// The worktree string the hook resolves: `repo_at` canonicalizes, and the
/// temp dir lives under macOS's symlinked /var — store the real path.
fn wt(d: &Path) -> std::path::PathBuf {
    d.canonicalize().unwrap_or_else(|_| d.to_path_buf())
}

/// A minimal `run` table with one live run of this worktree, like a
/// `chunk start` without launcher would leave it.
fn live_run(d: &Path, session: &str, model: Option<&str>) {
    let db = crew_db(d);
    std::fs::create_dir_all(db.parent().unwrap()).unwrap();
    let c = rusqlite::Connection::open(&db).unwrap();
    c.execute_batch(
        "CREATE TABLE run(id INTEGER PRIMARY KEY, worktree TEXT, session TEXT,
          ended TEXT, last_seen TEXT, model TEXT);",
    )
    .unwrap();
    c.execute(
        "INSERT INTO run(worktree, session, ended, last_seen, model)
         VALUES (?1, ?2, NULL, NULL, ?3)",
        rusqlite::params![wt(d).to_string_lossy(), session, model],
    )
    .unwrap();
}

fn run_model(d: &Path) -> (Option<String>, Option<String>) {
    let c = rusqlite::Connection::open(crew_db(d)).unwrap();
    c.query_row("SELECT model, last_seen FROM run", [], |r| {
        Ok((r.get(0)?, r.get(1)?))
    })
    .unwrap()
}

fn claude_transcript(d: &Path, stem: &str, model: &str) -> std::path::PathBuf {
    let p = d.join(format!("{stem}.jsonl"));
    std::fs::write(
        &p,
        serde_json::json!({
            "type": "assistant",
            "message": {"model": model,
                "usage": {"input_tokens": 10, "output_tokens": 5}},
            "requestedModel": model,
        })
        .to_string(),
    )
    .unwrap();
    p
}

#[test]
fn claude_transcript_model_stamps_the_live_run() {
    let d = repo();
    let t = claude_transcript(&d, "t1", "claude-opus-5-5");
    live_run(&d, "t1", None);
    let input = format!(
        r#"{{"cwd":{},"transcript_path":{}}}"#,
        json(&wt(&d)),
        json(&t)
    );
    let (ok, out, err) = fael(&d, &["hook", "stop", "--client", "claude"], &input);
    assert!(ok, "{err}");
    assert!(out.is_empty(), "stop says nothing: {out}");
    let (model, last_seen) = run_model(&d);
    assert_eq!(model.as_deref(), Some("claude-opus-5-5"));
    assert!(last_seen.is_some(), "liveness still stamped");
}

#[test]
fn codex_turn_context_model_stamps_the_live_run() {
    let d = repo();
    let t = d.join("c1.jsonl");
    std::fs::write(
        &t,
        serde_json::json!({"type": "turn_context", "payload": {"model": "gpt-6-luna"}}).to_string(),
    )
    .unwrap();
    live_run(&d, "c1", None);
    let input = format!(
        r#"{{"cwd":{},"transcript_path":{}}}"#,
        json(&wt(&d)),
        json(&t)
    );
    let (ok, out, err) = fael(&d, &["hook", "stop", "--client", "codex"], &input);
    assert!(ok, "{err}");
    assert!(out.is_empty(), "stop says nothing: {out}");
    assert_eq!(run_model(&d).0.as_deref(), Some("gpt-6-luna"));
}

#[test]
fn a_start_time_model_always_wins() {
    let d = repo();
    let t = claude_transcript(&d, "t1", "transcript-m");
    live_run(&d, "t1", Some("starter-m"));
    let input = format!(
        r#"{{"cwd":{},"transcript_path":{}}}"#,
        json(&wt(&d)),
        json(&t)
    );
    let (ok, _, err) = fael(&d, &["hook", "stop", "--client", "claude"], &input);
    assert!(ok, "{err}");
    assert_eq!(run_model(&d).0.as_deref(), Some("starter-m"));
}

#[test]
fn stdin_model_wins_over_the_transcript_and_usage_stays_model_free() {
    let d = repo();
    let t = claude_transcript(&d, "t1", "transcript-m");
    live_run(&d, "t1", None);
    // session-start stamps the run too, and records a usage line
    let input = format!(
        r#"{{"cwd":{},"transcript_path":{},"model":"stdin-m"}}"#,
        json(&wt(&d)),
        json(&t)
    );
    let (ok, _, err) = fael(&d, &["hook", "session-start", "--client", "claude"], &input);
    assert!(ok, "{err}");
    assert_eq!(run_model(&d).0.as_deref(), Some("stdin-m"));
    let usage = std::fs::read_to_string(d.join("state/usage.jsonl")).unwrap();
    assert!(!usage.is_empty(), "session-start records usage");
    for line in usage.lines() {
        let v: serde_json::Value = serde_json::from_str(line).unwrap();
        assert!(v.get("model").is_none(), "usage stays model-free: {line}");
    }
}
