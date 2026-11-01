//! Outcome joins (PLAN-fael-outcome-dayone chunk o-panel): the frozen
//! `outcome` key set plus the explicit-`--since` wiring over zero usage.

use fael_core::Config;
use fael_core::stats::{aggregate, parse};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

fn keys(v: &serde_json::Value) -> Vec<&str> {
    let mut ks: Vec<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
    ks.sort_unstable();
    ks
}

fn value_of(text: &str) -> serde_json::Value {
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
        false,
        0,
        None,
        &[],
        None,
    );
    serde_json::to_value(&stats).unwrap()
}

const BASE_ROWS: &str = concat!(
    "{\"ts\":\"2026-09-26T00:00:00.000Z\",\"repo\":\"/work/real\",\"client\":\"claude\",\"event\":\"read\",\"bytes\":10,\"est_tokens\":3,\"ids\":[\"A\"]}\n",
    "{\"ts\":\"2026-09-26T00:01:00.000Z\",\"repo\":\"/work/real\",\"client\":\"codex\",\"event\":\"edit\",\"bytes\":20,\"est_tokens\":5,\"ids\":[\"A\",\"B\"]}\n",
);

#[test]
fn json_shape_outcome_is_frozen() {
    let v = value_of(BASE_ROWS);
    assert_eq!(
        keys(&v["outcome"]),
        [
            "filed_to_close",
            "issues_closed",
            "issues_filed",
            "joined_since",
            "prs_created",
            "prs_merged",
            "prs_merged_unknown",
            "prs_with_row",
            "prs_without_row"
        ],
        "{v}"
    );
    assert_eq!(
        keys(&v["outcome"]["filed_to_close"]),
        ["median", "n", "p90"],
        "{v}"
    );
    assert!(
        v["outcome"]["prs_merged"].is_u64()
            && v["outcome"]["prs_created"].is_u64()
            && v["outcome"]["prs_merged_unknown"].is_boolean()
            && v["outcome"]["issues_filed"].is_u64()
            && v["outcome"]["issues_closed"].is_u64()
            && v["outcome"]["joined_since"].is_string()
            && v["outcome"]["prs_with_row"].is_u64()
            && v["outcome"]["prs_without_row"].is_u64(),
        "{v}"
    );
    // no gh in the test env: unknown, never an error
    assert_eq!(v["outcome"]["prs_merged_unknown"], true, "{v}");
}

#[test]
fn outcome_wires_prs_and_the_explicit_since_window() {
    // zero usage: the explicit --since still windows the outcome join
    let tmp = vec![PathBuf::from("/tmp")];
    let parsed = parse("", Path::new("/work/state/usage.jsonl"), &tmp);
    assert_eq!(parsed.n, 0);
    let issues = "{\"v\":1,\"id\":\"A\",\"ts\":\"2026-10-01T00:00:00Z\",\"by\":\"w\",\"kind\":\"issue\",\"text\":\"t\",\"files\":[]}\n";
    let closes = "{\"v\":1,\"id\":\"X\",\"ts\":\"2026-10-01T02:00:00Z\",\"by\":\"w\",\"kind\":\"close\",\"text\":\"fixed (#41)\",\"files\":[],\"ref\":\"A\"}\n";
    let (mut rows, mut w) = (vec![], vec![]);
    fael_core::parse(issues.as_bytes(), "t.jsonl", &mut rows, &mut w);
    let (mut crows, mut cw) = (vec![], vec![]);
    fael_core::parse(closes.as_bytes(), "c.jsonl", &mut crows, &mut cw);
    let logs = HashMap::from([(
        "/w/r".to_string(),
        fael_core::Log {
            rows,
            closes: crows,
            ..Default::default()
        },
    )]);
    let prs = vec![fael_core::stats::ListedPr {
        number: 41,
        branch: Some("feat/other".into()),
        created_ms: fael_core::ts_ms("2026-10-01T01:00:00Z"),
        merged_ms: None,
    }];
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
        fael_core::ts_ms("2026-10-01T00:00:00Z"),
        &prs,
        Some(7),
    );
    let v = serde_json::to_value(&s).unwrap();
    assert_eq!(v["outcome"]["prs_merged"], 7, "{v}");
    assert_eq!(v["outcome"]["issues_filed"], 1, "{v}");
    assert_eq!(v["outcome"]["issues_closed"], 1, "{v}");
    assert_eq!(v["outcome"]["filed_to_close"]["n"], 1, "{v}");
    assert_eq!(v["outcome"]["prs_with_row"], 1, "{v}");
    assert_eq!(v["outcome"]["prs_merged_unknown"], false, "{v}");
}
