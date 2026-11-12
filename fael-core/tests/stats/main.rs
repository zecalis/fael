//! `fael stats --json` shape lock (PLAN-fael-sync chunk 2): the exact key set
//! and each field's type. A shape change fails here first — then update this
//! list, `docs/stats.md` (changelog), and `STATS_SCHEMA` iff a field was
//! removed, renamed, retyped or redefined (additions never bump the number).
//!
//! Thin entry only — the suites sit next to this file: the chunk-2 `Stats`
//! shape above, `day` (PLAN-fael-sync chunk 3: `DayView` edges + shape).

mod day;
mod outcome;
mod value;

use fael_core::Config;
use fael_core::stats::{aggregate, parse};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

fn value_of(text: &str, with_rows: bool) -> serde_json::Value {
    let tmp = vec![PathBuf::from("/tmp")];
    let parsed = parse(text, Path::new("/work/state/usage.jsonl"), &tmp);
    let stats = aggregate(
        &parsed,
        &HashMap::new(),
        &HashMap::new(),
        &[],
        &[],
        &Config::default(),
        (1, 2, 3, 4).into(),
        with_rows,
        0,
        None,
        &[],
        None,
    );
    serde_json::to_value(&stats).unwrap()
}

/// `capture` keys, and `experience` (PLAN-fael-experience-loop chunk 4):
/// every field listed, zeros included.
#[test]
fn capture_and_experience_shapes_are_frozen() {
    let v = value_of(BASE_ROWS, false);
    assert_eq!(
        keys(&v["capture"]),
        [
            "edit_session_issues",
            "manual_adds",
            "no_row_sessions",
            "sessions_with_edits",
            "sessions_with_edits_gone",
            "sessions_with_edits_no_row",
        ],
        "{v}"
    );
    assert_eq!(
        v["experience"],
        serde_json::json!({"fixed": 0, "fixed_from_review": 0, "closed_with_check": 0, "repeats_with_check": 0, "edits_after_close_with_check": 0, "fix_commits": 0, "fix_commits_linked": 0}),
        "{v}"
    );
}

/// `provenance` (plan proof chunk `provenance`): field path → shell line,
/// empty from core alone (the caller fills this run's range).
#[test]
fn provenance_key_is_present_and_empty_from_core() {
    let v = value_of(BASE_ROWS, false);
    assert!(v["provenance"].is_object(), "{v}");
    assert!(v["provenance"].as_object().unwrap().is_empty(), "{v}");
}

const BASE_ROWS: &str = concat!(
    "{\"ts\":\"2026-09-26T00:00:00.000Z\",\"repo\":\"/work/real\",\"client\":\"claude\",\"event\":\"read\",\"bytes\":10,\"est_tokens\":3,\"ids\":[\"A\"]}\n",
    "{\"ts\":\"2026-09-26T00:01:00.000Z\",\"repo\":\"/work/real\",\"client\":\"codex\",\"event\":\"edit\",\"bytes\":20,\"est_tokens\":5,\"ids\":[\"A\",\"B\"]}\n",
);

fn keys(v: &serde_json::Value) -> Vec<&str> {
    let mut ks: Vec<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
    ks.sort_unstable();
    ks
}

#[test]
fn schema_version_is_pinned() {
    assert_eq!(value_of(BASE_ROWS, false)["schema"], 7);
}

#[test]
fn json_shape_keys_and_types_are_frozen() {
    let v = value_of(BASE_ROWS, false);
    assert_eq!(
        keys(&v),
        [
            "asks",
            "by_client",
            "by_event",
            "bytes",
            "capture",
            "constants",
            "context_by_client",
            "context_loop",
            "days",
            "est_tokens",
            "events",
            "experience",
            "flow",
            "friction",
            "handoff_runs",
            "lifecycle",
            "models",
            "non_english_rows",
            "outcome",
            "outcomes_v",
            "provenance",
            "retired",
            "rounds",
            "said",
            "schema",
            "session_splits",
            "sessions",
            "skipped_temp",
            "throughput",
            "top_rows",
            "unused_rows",
            "usage",
            "value",
        ],
        "{v}"
    );
    assert_eq!(
        keys(&v["said"]).join(" "),
        "ask bodies brief briefdrift checkpoint chunkbrief cited finding fixcommit fixed handoff held inbox note notice overlap peer pointer retired row",
        "{v}"
    );
    assert_eq!(
        v["said"]["row"],
        serde_json::json!({"said": 0, "earned": 0}),
        "{v}"
    );
    assert_eq!(
        v["context_loop"],
        serde_json::json!({"confirmed_repeats": 0, "edits_after_close": 0, "useful_shows": 0}),
        "{v}"
    );
    for k in ["events", "bytes", "est_tokens", "skipped_temp"] {
        assert!(v[k].is_u64(), "{k}: {v}");
    }
    for k in [
        "by_event",
        "by_client",
        "constants",
        "rounds",
        "non_english_rows",
    ] {
        assert!(v[k].is_object(), "{k}: {v}");
    }
    assert!(v["top_rows"].is_array(), "{v}");
    assert!(v.get("rows").is_none(), "no --rows, so absent: {v}");
    assert_eq!(keys(&v["asks"]), ["warning"], "{v}");
    let w = &v["asks"]["warning"];
    assert!(
        w["events"].is_u64() && w["bytes"].is_u64() && w["codes"].is_object(),
        "{v}"
    );
    assert_eq!(
        keys(&v["constants"]).join(" "),
        "mcp_schema_bytes mcp_schema_est skill_bytes skill_est",
        "{v}"
    );
    assert_eq!(
        keys(&v["rounds"]),
        [
            "rows_added",
            "since",
            "since_local",
            "since_utc",
            "tz_offset"
        ],
        "{v}"
    );
    assert_eq!(keys(&v["non_english_rows"]), ["non_english", "rows"], "{v}");
    let top = v["top_rows"].as_array().unwrap();
    assert_eq!(top.len(), 2, "{v}");
    assert_eq!(keys(&top[0]), ["id", "pushes"], "{v}");
    assert!(top[0]["id"].is_string() && top[0]["pushes"].is_u64(), "{v}");
}

/// Chunk stats-unknown + hint-split: run attribution with reported / hint-only
/// / none split and unknown-% beside every number, plus input-side transcript
/// tokens per client — additions only, so the frozen key list above just gains
/// the keys.
#[test]
fn json_shape_models_attribution_is_frozen() {
    let v = value_of(BASE_ROWS, false);
    assert_eq!(
        keys(&v["models"]),
        [
            "by_model",
            "hint_only",
            "none",
            "total",
            "unknown_cost_pct",
            "unknown_model_pct",
            "with_client_model",
            "with_cost"
        ],
        "{v}"
    );
    assert!(
        v["models"]["total"].is_u64()
            && v["models"]["with_client_model"].is_u64()
            && v["models"]["hint_only"].is_u64()
            && v["models"]["none"].is_u64()
            && v["models"]["with_cost"].is_u64()
            && v["models"]["unknown_model_pct"].is_f64()
            && v["models"]["unknown_cost_pct"].is_f64()
            && v["models"]["by_model"].is_object(),
        "{v}"
    );
    assert!(v["context_by_client"].is_object(), "{v}");
}

#[test]
fn json_shape_flow_and_throughput_are_frozen() {
    let v = value_of(BASE_ROWS, false);
    assert_eq!(
        keys(&v["flow"]),
        [
            "done_to_merge",
            "filed_to_close",
            "runs_total",
            "runs_with_pr",
            "start_to_done"
        ],
        "{v}"
    );
    for k in ["filed_to_close", "start_to_done", "done_to_merge"] {
        assert_eq!(keys(&v["flow"][k]), ["median", "n", "p90"], "{v}");
        assert!(
            v["flow"][k]["median"].is_u64()
                && v["flow"][k]["p90"].is_u64()
                && v["flow"][k]["n"].is_u64(),
            "{k}: {v}"
        );
    }
    assert!(
        v["flow"]["runs_with_pr"].is_u64() && v["flow"]["runs_total"].is_u64(),
        "{v}"
    );
    assert!(v["throughput"].is_array(), "{v}");
    assert_eq!(v["days"][0]["day"], "2026-09-26", "{v}");
}

#[test]
fn json_rows_shape_with_flag() {
    let v = value_of(BASE_ROWS, true);
    let rows = v["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 2, "{v}");
    assert_eq!(
        keys(&rows[0]),
        ["id", "noise", "outcomes", "pushes", "status"],
        "{v}"
    );
    assert!(
        rows[0]["id"].is_string()
            && rows[0]["pushes"].is_u64()
            && rows[0]["status"].is_string()
            && rows[0]["noise"].is_boolean(),
        "{v}"
    );
    // outcomes_v 1: the keys a reader of `rows[].outcomes` relies on
    assert_eq!(v["outcomes_v"], 1, "{v}");
    assert_eq!(
        keys(&rows[0]["outcomes"]),
        [
            "acted",
            "cited",
            "cut",
            "missed_push",
            "pulled",
            "retrieved_after_cut",
            "shown"
        ],
        "{v}"
    );
    assert_eq!(
        keys(&rows[0]["outcomes"]["pulled"]),
        ["agent_initiated", "fael_induced"],
        "{v}"
    );
}

/// Schema 2: usage rows from the removed Stop-block mode stay counted as
/// events, but no stop-block field reads them any more.
#[test]
fn old_stop_block_rows_surface_nowhere() {
    let text = concat!(
        "{\"ts\":\"2026-09-28T00:00:01Z\",\"repo\":\"/work/real\",\"client\":\"claude\",\"event\":\"stop-work\",\"ask\":\"stop-block\",\"session\":\"s1\",\"bytes\":10,\"est_tokens\":3,\"ids\":[]}\n",
        "{\"ts\":\"2026-09-28T00:00:02Z\",\"repo\":\"/work/real\",\"client\":\"claude\",\"event\":\"read\",\"session\":\"s1\",\"bytes\":10,\"est_tokens\":3,\"ids\":[],\"real_tokens\":{\"input_tokens\":100,\"cache_creation_input_tokens\":200,\"cache_read_input_tokens\":300,\"output_tokens\":5}}\n",
    );
    let v = value_of(text, false);
    assert_eq!(v["events"], 2, "{v}");
    assert_eq!(keys(&v["asks"]), ["warning"], "{v}");
    for k in ["real_tokens", "stop_blocks", "repeat_blocks"] {
        assert!(v.get(k).is_none(), "{k}: {v}");
    }
}

#[test]
fn carriers_never_count_as_rows_added() {
    // a bump moves a row, a restore reverts an edge: neither files a row
    let tmp = vec![PathBuf::from("/tmp")];
    let parsed = parse(BASE_ROWS, Path::new("/work/state/usage.jsonl"), &tmp);
    let row = fael_core::Row::new("me", "note", "kept", vec!["src/a.rs".into()]);
    let mut ev = fael_core::Row::bumped("me", &row.id);
    ev.id = fael_core::ulid();
    let log = fael_core::Log {
        rows: vec![row, ev],
        ..Default::default()
    };
    let logs = HashMap::from([("/work/real".to_string(), log)]);
    let s = aggregate(
        &parsed,
        &logs,
        &HashMap::new(),
        &[],
        &[],
        &Config::default(),
        (1, 2, 3, 4).into(),
        false,
        0,
        None,
        &[],
        None,
    );
    let v = serde_json::to_value(&s).unwrap();
    assert_eq!(v["rounds"]["rows_added"], 1, "{v}");
    assert_eq!(v["non_english_rows"]["rows"], 1, "{v}");
}

#[test]
fn worktrees_sharing_a_journal_count_rows_added_once() {
    let rows = concat!(
        "{\"ts\":\"2026-09-26T00:00:00.000Z\",\"repo\":\"/work/a\",\"client\":\"claude\",\"event\":\"read\",\"bytes\":10,\"est_tokens\":3,\"ids\":[\"A\"]}\n",
        "{\"ts\":\"2026-09-26T00:00:00.000Z\",\"repo\":\"/work/b\",\"client\":\"claude\",\"event\":\"read\",\"bytes\":10,\"est_tokens\":3,\"ids\":[\"A\"]}\n",
    );
    let tmp = vec![PathBuf::from("/tmp")];
    let parsed = parse(rows, Path::new("/work/state/usage.jsonl"), &tmp);
    let row = fael_core::Row::new("me", "note", "kept", vec!["src/a.rs".into()]);
    let log = fael_core::Log {
        rows: vec![row],
        ..Default::default()
    };
    let logs = HashMap::from([
        ("/work/a".to_string(), log.clone()),
        ("/work/b".to_string(), log),
    ]);
    let s = aggregate(
        &parsed,
        &logs,
        &HashMap::new(),
        &[],
        &[],
        &Config::default(),
        (1, 2, 3, 4).into(),
        false,
        0,
        None,
        &[],
        None,
    );
    let v = serde_json::to_value(&s).unwrap();
    assert_eq!(v["rounds"]["rows_added"], 1, "{v}");
}
