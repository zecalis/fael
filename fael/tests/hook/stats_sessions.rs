//! `fael stats --session <id>` and `fael stats sessions` (issue 01M75KQ5):
//! two sessions in a fixture usage log — one handed a row and inside a crew
//! run, one handed nothing and outside — read back per session and split.

use super::{fael_at, json, repo};

fn setup() -> (std::path::PathBuf, std::path::PathBuf) {
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
             INSERT INTO chunk VALUES (1,'A');
             INSERT INTO run VALUES (1,1,'/p/sess-a1.jsonl','2026-10-02T00:00:00Z','2026-10-02T02:00:00Z',NULL,NULL);",
        )
        .unwrap();
    let line = |ts: &str, s: &str, rest: &str| {
        format!(
            r#"{{"ts":"{ts}","repo":{},"client":"claude","session":"{s}",{rest}}}"#,
            json(&d)
        )
    };
    let body = [
        // opens the client's hit window before any push (`value::window_start`)
        line(
            "2026-10-01T00:00:00.000Z",
            "sess-b2",
            r#""event":"in-context","in_context":["Z"],"files":[]"#,
        ),
        line("2026-10-02T00:00:00.000Z", "sess-a1", r#""event":"session-start","bytes":9,"est_tokens":3,"ids":["A"]"#),
        line("2026-10-02T00:01:00.000Z", "sess-a1", r#""event":"edit","bytes":9,"est_tokens":4,"ids":[],"branch":"feat/x","files":["a.rs"]"#),
        line("2026-10-02T00:01:00.000Z", "sess-a1", r#""event":"in-context","in_context":["A"],"files":["a.rs"]"#),
        line("2026-10-02T00:02:00.000Z", "sess-a1", r#""event":"call","cmd":"add","outcome":"ok""#),
        line("2026-10-02T00:03:00.000Z", "sess-a1", r#""event":"call","cmd":"add","outcome":"reject","reason":"x""#),
        line("2026-10-03T00:00:00.000Z", "sess-b2", r#""event":"edit","bytes":0,"est_tokens":0,"ids":[]"#),
    ]
    .join("\n");
    std::fs::write(state.join("usage.jsonl"), body + "\n").unwrap();
    (d, state)
}

#[test]
fn session_prints_one_session_by_prefix() {
    let (d, state) = setup();
    let (ok, out, err) = fael_at(&state, &d, &["stats", "--session", "sess-a"], "");
    assert!(ok, "{err}");
    for want in [
        "session sess-a1 (",
        "client claude · 2026-10-02T00:00:00.000Z → 2026-10-02T00:03:00.000Z · branch feat/x",
        "pushed ~7 tokens: edit ~4, session-start ~3",
        "rows shown 1 · of 1 scored: in context at an edit 1 · acted 1",
        "edits 1 · calls 2 · rejects 1 · rows filed 0",
    ] {
        assert!(out.contains(want), "{want} missing: {out}");
    }
    assert!(!out.contains("sess-b2"), "{out}");
    let (ok, out, err) = fael_at(&state, &d, &["stats", "--session", "sess-a1", "--json"], "");
    assert!(ok, "{err}");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(
        (v["shown"].as_u64(), v["in_run"].as_bool()),
        (Some(1), Some(true))
    );
    // an ambiguous prefix names the stems; no session is a reject
    let (ok, _, err) = fael_at(&state, &d, &["stats", "--session", "sess"], "");
    assert!(!ok && err.contains("sess-a1, sess-b2"), "{err}");
    let (ok, _, err) = fael_at(&state, &d, &["stats", "--session", "nope"], "");
    assert!(!ok && err.contains("no session"), "{err}");
}

#[test]
fn json_lists_every_session_and_sessions_prints_the_two_tables() {
    let (d, state) = setup();
    let (ok, out, err) = fael_at(&state, &d, &["stats", "--json"], "");
    assert!(ok, "{err}");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["schema"], 6, "{out}");
    let names: Vec<_> = v["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["session"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["sess-b2", "sess-a1"], "{out}");
    assert_eq!(
        v["session_splits"]["pushed"]["with"]["sessions"], 1,
        "{out}"
    );
    let (ok, out, err) = fael_at(&state, &d, &["stats", "sessions"], "");
    assert!(ok, "{err}");
    assert!(out.contains("rows pushed: sessions 1 · edits 1 · rejects per 100 calls 50.0 (2 calls) · rows filed 0 · no row 1"), "{out}");
    assert!(
        out.contains("none pushed: sessions 1 · edits 1 · rejects per 100 calls n/a (0 calls)"),
        "{out}"
    );
    assert!(out.contains("inside a run: sessions 1"), "{out}");
    assert_eq!(
        out.matches(
            "does not show: an effect of fael — the groups differ in task, client and repo"
        )
        .count(),
        2,
        "{out}"
    );
}
