//! Chunk stats-unknown: `fael stats` names the model/cost unknown share.
//!
//! A fixture with one known and one unknown-model crew.db run prints the
//! per-client real tokens, the per-model runs/cost, and one
//! `unknown model N% · unknown cost M%` line — `--json` carries the same
//! numbers. No runs (or no measured tokens) prints no line, so old output
//! is byte-identical (pinned in `stats_golden.rs`).

use super::{fael_at, json, repo};

/// One measured usage row plus one known and one unknown run row.
fn fixture() -> std::path::PathBuf {
    let d = repo();
    let state = d.join("state");
    std::fs::create_dir_all(&state).unwrap();
    let usage = format!(
        r#"{{"ts":"2026-09-26T00:00:00.000Z","repo":{},"client":"claude","event":"read","bytes":10,"est_tokens":3,"ids":[],"real_tokens":{{"input_tokens":100,"cache_creation_input_tokens":20,"cache_read_input_tokens":30,"output_tokens":5}}}}"#,
        json(&d)
    );
    std::fs::write(state.join("usage.jsonl"), usage + "\n").unwrap();
    let db = d.join(".git/fael/crew.db");
    std::fs::create_dir_all(db.parent().unwrap()).unwrap();
    let c = rusqlite::Connection::open(&db).unwrap();
    c.execute_batch(
        "CREATE TABLE run(id INTEGER PRIMARY KEY, client TEXT, model TEXT, cost REAL);
         INSERT INTO run(client, model, cost) VALUES ('claude', 'opus', 2.17);
         INSERT INTO run(client, model, cost) VALUES (NULL, NULL, NULL);",
    )
    .unwrap();
    d
}

#[test]
fn stats_names_unknown_model_and_cost() {
    let d = fixture();
    let state = d.join("state");
    let (ok, out, err) = fael_at(&state, &d, &["stats"], "");
    assert!(ok, "{err}");
    assert!(
        out.contains("  models: 2 runs · unknown model 50.0% · unknown cost 50.0%"),
        "{out}"
    );
    assert!(
        out.contains("  model claude/opus: 1 runs $2.170000 (unknown 0.0%)"),
        "{out}"
    );
    assert!(
        out.contains("  model unknown/unknown: 1 runs $0.000000 (unknown 100.0%)"),
        "{out}"
    );
    assert!(
        out.contains("  client claude: ×1 (~3 tokens, ~150 real)"),
        "{out}"
    );
    let (ok, out, err) = fael_at(&state, &d, &["stats", "--json"], "");
    assert!(ok, "{err}");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(
        v["models"],
        serde_json::json!({
            "total": 2, "with_client_model": 1, "with_cost": 1,
            "unknown_model_pct": 50.0, "unknown_cost_pct": 50.0,
            "by_model": {
                "claude/opus": {"runs": 1, "cost": 2.17, "unknown_cost_pct": 0.0},
                "unknown/unknown": {"runs": 1, "cost": 0.0, "unknown_cost_pct": 100.0},
            },
        }),
        "{out}"
    );
    assert_eq!(
        v["real_by_client"],
        serde_json::json!({"claude": 150}),
        "{out}"
    );
}
