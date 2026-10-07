//! `fael stats --json` shape lock (PLAN-fael-sync chunk 2): the exact key set
//! and each field's type. A shape change fails here first — then update this
//! list, `docs/stats.md` (changelog), and `STATS_SCHEMA` iff a field was
//! removed, renamed, retyped or redefined (additions never bump the number).
//!
//! Thin entry only — the suites sit next to this file: the chunk-2 `Stats`
//! shape above, `day` (PLAN-fael-sync chunk 3: `DayView` edges + shape).

mod day;

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
        &Config::default(),
        (1, 2, 3, 4).into(),
        with_rows,
    );
    serde_json::to_value(&stats).unwrap()
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
            "context_loop",
            "est_tokens",
            "events",
            "file_verdict",
            "friction",
            "incidents",
            "non_english_rows",
            "outcomes_v",
            "retired",
            "rounds",
            "said",
            "schema",
            "skipped_temp",
            "top_rows",
            "unused_rows",
            "value",
        ],
        "{v}"
    );
    assert_eq!(v["schema"], 2, "{v}");
    // every kind listed, zeros included
    assert_eq!(
        keys(&v["said"]),
        [
            "ask", "bodies", "brief", "cited", "count", "note", "notice", "pointer", "row"
        ],
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
    assert_eq!(
        keys(&v["capture"]),
        [
            "manual_adds",
            "no_row_sessions",
            "reply_lines",
            "reply_rejected",
            "reply_stored",
            "sessions_with_edits",
            "sessions_with_edits_no_row",
        ],
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
    assert_eq!(keys(&v["asks"]), ["reject", "warning"], "{v}");
    for k in ["reject", "warning"] {
        assert!(
            v["asks"][k]["events"].is_u64() && v["asks"][k]["bytes"].is_u64(),
            "{k}: {v}"
        );
    }
    assert_eq!(
        keys(&v["constants"]),
        [
            "mcp_schema_bytes",
            "mcp_schema_est",
            "skill_bytes",
            "skill_est"
        ],
        "{v}"
    );
    assert_eq!(keys(&v["rounds"]), ["rows_added", "since"], "{v}");
    assert_eq!(keys(&v["non_english_rows"]), ["non_english", "rows"], "{v}");
    let top = v["top_rows"].as_array().unwrap();
    assert_eq!(top.len(), 2, "{v}");
    assert_eq!(keys(&top[0]), ["id", "pushes"], "{v}");
    assert!(top[0]["id"].is_string() && top[0]["pushes"].is_u64(), "{v}");
}

#[test]
fn json_shape_retired_value_and_verdict_are_frozen() {
    let v = value_of(BASE_ROWS, false);
    assert_eq!(keys(&v["retired"]), ["at_touch", "pushed"], "{v}");
    assert!(
        v["retired"]["pushed"].is_u64() && v["retired"]["at_touch"].is_u64(),
        "{v}"
    );
    let value = ["handoffs_picked_up", "in_context_at_edit", "issues_closed"];
    let mut value_keys = value.to_vec();
    value_keys.insert(0, "by_event");
    value_keys.insert(1, "cross_agent");
    assert_eq!(keys(&v["value"]), value_keys, "{v}");
    assert!(value.iter().all(|k| v["value"][k].is_u64()), "{v}");
    assert!(v["value"]["by_event"].is_object(), "{v}");
    // BASE_ROWS predate the shadow keys: nothing measured, all zero
    assert_eq!(
        v["file_verdict"],
        serde_json::json!({"changed": 0, "unchanged": 0, "no_verdict": 0, "no_fh": 0, "retire": {"changed": {"pairs": 0, "retired": 0}, "unchanged": {"pairs": 0, "retired": 0}}}),
        "{v}"
    );
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
    // outcomes_v 1: the keys a reader of `tune` relies on
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
    assert_eq!(keys(&v["asks"]), ["reject", "warning"], "{v}");
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
        &Config::default(),
        (1, 2, 3, 4).into(),
        false,
    );
    let v = serde_json::to_value(&s).unwrap();
    assert_eq!(v["rounds"]["rows_added"], 1, "{v}");
    assert_eq!(v["non_english_rows"]["rows"], 1, "{v}");
}
