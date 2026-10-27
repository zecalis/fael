//! `fael stats` prints the after-handoff line from crew runs and usage lines.

use super::{fael_at, json, repo};

#[test]
fn stats_prints_the_after_handoff_line_from_crew_runs_and_usage() {
    // two closed runs: B started after A's hand-off (with), C has no `after` (without)
    let d = repo();
    let state = d.join("state");
    std::fs::create_dir_all(&state).unwrap();
    let db = d.join(".git/fael/crew.db");
    std::fs::create_dir_all(db.parent().unwrap()).unwrap();
    rusqlite::Connection::open(&db)
        .unwrap()
        .execute_batch(
            "CREATE TABLE chunk(id INTEGER PRIMARY KEY, uid TEXT);
             CREATE TABLE edge(src INTEGER, dst INTEGER, kind TEXT);
             CREATE TABLE run(id INTEGER PRIMARY KEY, chunk INTEGER, session TEXT,
               started TEXT, ended TEXT, pr INTEGER, branch TEXT);
             CREATE TABLE event(id INTEGER PRIMARY KEY, chunk_uid TEXT, at TEXT,
               by TEXT, to_state TEXT, text TEXT);
             INSERT INTO chunk VALUES (1,'A'),(2,'B'),(3,'C');
             INSERT INTO edge VALUES (2,1,'after');
             INSERT INTO event VALUES (1,'A','2026-10-01T00:00:00Z','agent','done','word');
             INSERT INTO event VALUES (2,'B','2026-10-02T01:00:00Z','agent','done','ok');
             INSERT INTO run VALUES (1,2,'/p/sb.jsonl','2026-10-02T00:00:00Z','2026-10-02T02:00:00Z',NULL,NULL);
             INSERT INTO run VALUES (2,3,'sc','2026-10-02T00:00:00Z','2026-10-02T02:00:00Z',NULL,NULL);",
        )
        .unwrap();
    let line = |ts: &str, s: &str, rest: &str| {
        format!(
            r#"{{"ts":"{ts}","repo":{},"client":"claude","session":"{s}",{rest}}}"#,
            json(&d)
        )
    };
    let body = [
        // one counted push opens the window (first usage per repo)
        line(
            "2026-10-01T12:00:00.000Z",
            "sa",
            r#""event":"read","bytes":10,"est_tokens":3,"ids":["A"]"#,
        ),
        line(
            "2026-10-02T00:10:00.000Z",
            "sb",
            r#""event":"find","found":["X"]"#,
        ),
        line(
            "2026-10-02T00:20:00.000Z",
            "sb",
            r#""event":"find","found":["X"]"#,
        ),
        line(
            "2026-10-02T00:30:00.000Z",
            "sc",
            r#""event":"call","cmd":"add","outcome":"reject","reason":"r""#,
        ),
    ]
    .join("\n");
    std::fs::write(state.join("usage.jsonl"), body + "\n").unwrap();
    let (ok, out, err) = fael_at(&state, &d, &["stats"], "");
    assert!(ok, "{err}");
    assert!(
        out.contains("  after-handoff runs: with 1 (done 1 · re-finds 1/2 finds · rejects 0) · without 1 (done 0 · re-finds 0/0 finds · rejects 1)"),
        "{out}"
    );
}
