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
            "est_tokens",
            "events",
            "non_english_rows",
            "repeat_blocks",
            "retired",
            "rounds",
            "schema",
            "skipped_temp",
            "stop_blocks",
            "top_rows",
        ],
        "{v}"
    );
    assert_eq!(v["schema"], 1, "{v}");
    assert_eq!(
        keys(&v["capture"]),
        [
            "manual_adds",
            "no_row_sessions",
            "post_stop_rounds",
            "reply_lines",
            "reply_rejected",
            "reply_stored",
            "sessions_with_edits",
            "sessions_with_edits_no_row",
        ],
        "{v}"
    );
    for k in [
        "events",
        "bytes",
        "est_tokens",
        "skipped_temp",
        "repeat_blocks",
    ] {
        assert!(v[k].is_u64(), "{k}: {v}");
    }
    for k in [
        "by_event",
        "by_client",
        "stop_blocks",
        "constants",
        "rounds",
        "non_english_rows",
    ] {
        assert!(v[k].is_object(), "{k}: {v}");
    }
    assert_eq!(keys(&v["retired"]), ["at_touch", "pushed"], "{v}");
    assert!(
        v["retired"]["pushed"].is_u64() && v["retired"]["at_touch"].is_u64(),
        "{v}"
    );
    assert!(v["top_rows"].is_array(), "{v}");
    assert!(v.get("real_tokens").is_none(), "no samples, so absent: {v}");
    assert!(v.get("rows").is_none(), "no --rows, so absent: {v}");
    assert_eq!(keys(&v["asks"]), ["reject", "stop-block", "warning"], "{v}");
    for k in ["reject", "stop-block", "warning"] {
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
    assert_eq!(
        keys(&v["rounds"]),
        ["after_block", "rows_added", "since"],
        "{v}"
    );
    assert_eq!(keys(&v["non_english_rows"]), ["non_english", "rows"], "{v}");
    let top = v["top_rows"].as_array().unwrap();
    assert_eq!(top.len(), 2, "{v}");
    assert_eq!(keys(&top[0]), ["id", "pushes"], "{v}");
    assert!(top[0]["id"].is_string() && top[0]["pushes"].is_u64(), "{v}");
}

#[test]
fn json_rows_shape_with_flag() {
    let v = value_of(BASE_ROWS, true);
    let rows = v["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 2, "{v}");
    assert_eq!(keys(&rows[0]), ["id", "noise", "pushes", "status"], "{v}");
    assert!(
        rows[0]["id"].is_string()
            && rows[0]["pushes"].is_u64()
            && rows[0]["status"].is_string()
            && rows[0]["noise"].is_boolean(),
        "{v}"
    );
}

#[test]
fn json_real_tokens_shape_after_a_block() {
    let text = concat!(
        "{\"ts\":\"2026-09-28T00:00:01Z\",\"repo\":\"/work/real\",\"client\":\"claude\",\"event\":\"read\",\"ask\":\"stop-block\",\"session\":\"s1\",\"bytes\":10,\"est_tokens\":3,\"ids\":[]}\n",
        "{\"ts\":\"2026-09-28T00:00:02Z\",\"repo\":\"/work/real\",\"client\":\"claude\",\"event\":\"read\",\"session\":\"s1\",\"bytes\":10,\"est_tokens\":3,\"ids\":[],\"real_tokens\":{\"input_tokens\":100,\"cache_creation_input_tokens\":200,\"cache_read_input_tokens\":300,\"output_tokens\":5}}\n",
    );
    let v = value_of(text, false);
    assert_eq!(
        keys(&v["real_tokens"]),
        [
            "avg_cache_create",
            "avg_cache_read",
            "avg_input",
            "avg_output",
            "post_block_rounds"
        ],
        "{v}"
    );
    assert_eq!(v["real_tokens"]["post_block_rounds"], 1, "{v}");
    assert_eq!(v["real_tokens"]["avg_input"], 100, "{v}");
}
