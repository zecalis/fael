//! f4 liveness: every edit and every session-start stamps the live run's
//! `last_seen`, not Stop only — a live turn longer than 30 min without Stop
//! must not read stalled.

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

/// A minimal `run` table with one unstamped live run of this worktree.
fn live_run(d: &Path, session: &str) {
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
         VALUES (?1, ?2, NULL, NULL, NULL)",
        rusqlite::params![wt(d).to_string_lossy(), session],
    )
    .unwrap();
}

fn last_seen(d: &Path) -> Option<String> {
    let c = rusqlite::Connection::open(crew_db(d)).unwrap();
    c.query_row("SELECT last_seen FROM run", [], |r| r.get(0))
        .unwrap()
}

#[test]
fn claude_edit_stamps_the_live_run() {
    let d = repo();
    live_run(&d, "s1");
    assert_eq!(last_seen(&d), None);
    let input = format!(
        r#"{{"cwd":{},"session_id":"s1","tool_input":{{"file_path":"src/a.rs"}}}}"#,
        json(&wt(&d)),
    );
    let (ok, _, err) = fael(&d, &["hook", "edit", "--client", "claude"], &input);
    assert!(ok, "{err}");
    assert!(last_seen(&d).is_some(), "edit stamps liveness");
}

#[test]
fn opencode_edit_stamps_the_live_run() {
    let d = repo();
    live_run(&d, "s1");
    assert_eq!(last_seen(&d), None);
    // the plugin speaks neutral: the client rides in the body, no --client flag
    let input = format!(
        r#"{{"cwd":{},"client":"opencode","session":"s1","files":["src/a.rs"]}}"#,
        json(&wt(&d)),
    );
    let (ok, _, err) = fael(&d, &["hook", "edit"], &input);
    assert!(ok, "{err}");
    assert!(last_seen(&d).is_some(), "edit stamps liveness");
}

#[test]
fn session_start_stamps_the_live_run() {
    let d = repo();
    live_run(&d, "s1");
    assert_eq!(last_seen(&d), None);
    let input = format!(r#"{{"cwd":{},"session_id":"s1"}}"#, json(&wt(&d)));
    let (ok, _, err) = fael(&d, &["hook", "session-start", "--client", "claude"], &input);
    assert!(ok, "{err}");
    assert!(last_seen(&d).is_some(), "session-start stamps liveness");
}
