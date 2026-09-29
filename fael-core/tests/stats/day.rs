//! `DayView` (PLAN-fael-sync chunk 3): the day boundary in non-UTC zones,
//! moved repos, the empty view, share/`for_you` null rules, the frozen shape,
//! and the 10k-row budget (< 50 ms in release).

use fael_core::stats::{DAY_SCHEMA, Parsed, day, parse};
use fael_core::{Log, Row, ts_ms};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

fn parsed_of(text: &str) -> Parsed {
    parse(
        text,
        Path::new("/work/state/usage.jsonl"),
        &[PathBuf::from("/tmp")],
    )
}

fn use_row(ts: &str, repo: &str, client: &str, event: &str, toks: u32, ids: &str) -> String {
    format!(
        "{{\"ts\":\"{ts}\",\"repo\":\"{repo}\",\"client\":\"{client}\",\"event\":\"{event}\",\"bytes\":10,\"est_tokens\":{toks},\"ids\":[{ids}],\"session\":\"s1\"}}\n"
    )
}

fn row(id: &str, kind: &str, ts: &str, by: &str) -> Row {
    Row {
        id: id.into(),
        ts: ts.into(),
        by: by.into(),
        kind: kind.into(),
        text: "t".into(),
        ..Default::default()
    }
}

fn keys(v: &serde_json::Value) -> Vec<&str> {
    let mut ks: Vec<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
    ks.sort_unstable();
    ks
}

#[test]
fn midnight_plus0700_splits_the_day() {
    // now = 08:00 +07:00 on Sep 29; A is 23:30 Sep 28 local, B 00:30 Sep 29
    let now = ts_ms("2026-09-29T01:00:00Z").unwrap();
    let text = use_row("2026-09-28T16:30:00Z", "/r", "claude", "read", 3, "\"A\"")
        + &use_row("2026-09-28T17:30:00Z", "/r", "claude", "read", 3, "\"B\"");
    let p = parsed_of(&text);
    let v = day(&p, &HashMap::new(), None, now, 420);
    assert_eq!(
        (v.day.as_str(), v.tz_offset.as_str()),
        ("2026-09-29", "+07:00")
    );
    assert_eq!(v.all.delivered.rows, 1);
    assert_eq!(v.all.delivered.last[0].id, "B");
    assert_eq!(v.repos.len(), 1);
    // 00:30 local = bucket 2
    assert_eq!(v.all.timeline.delivered[2], 1);
    assert_eq!(v.all.timeline.delivered.iter().sum::<usize>(), 1);
    // the same instant in UTC: both rows are yesterday, today is empty
    let u = day(&p, &HashMap::new(), None, now, 0);
    assert_eq!(u.day, "2026-09-29");
    assert_eq!(u.all.delivered.rows, 0);
    assert!(u.repos.is_empty());
}

#[test]
fn moved_repo_resolves_unknown_without_error() {
    let now = ts_ms("2026-09-29T12:00:00Z").unwrap();
    let text = use_row(
        "2026-09-29T11:59:00Z",
        "/gone/repo",
        "claude",
        "read",
        3,
        "\"Z9\"",
    );
    let v = day(&parsed_of(&text), &HashMap::new(), None, now, 0);
    assert_eq!(v.repos.len(), 1);
    assert_eq!(v.all.delivered.rows, 1);
    assert_eq!(v.all.delivered.last[0].title, "Z9");
    assert_eq!(v.all.delivered.last[0].file, "");
}

#[test]
fn empty_view_is_zeros_not_an_error() {
    let now = ts_ms("2026-09-29T12:00:00Z").unwrap();
    let v = day(&parsed_of(""), &HashMap::new(), None, now, 0);
    assert_eq!(v.schema, DAY_SCHEMA);
    assert_eq!(v.day, "2026-09-29");
    assert!(v.repos.is_empty());
    assert_eq!(v.all.delivered.rows, 0);
    assert_eq!(v.all.context.fael_tokens, 0);
    assert!(v.all.context.share.is_none());
    assert!(v.all.for_you.is_none());
    assert_eq!(v.all.timeline.delivered.len(), 96);
}

#[test]
fn day_json_shape_is_frozen() {
    let text = concat!(
        "{\"ts\":\"2026-09-29T10:00:00Z\",\"repo\":\"/r\",\"client\":\"claude\",\"event\":\"read\",\"bytes\":10,\"est_tokens\":100,\"ids\":[\"A\"],\"session\":\"s1\"}\n",
        "{\"ts\":\"2026-09-29T10:01:00Z\",\"repo\":\"/r\",\"client\":\"claude\",\"event\":\"read\",\"bytes\":10,\"est_tokens\":50,\"ids\":[],\"session\":\"s1\",",
        "\"real_tokens\":{\"input_tokens\":1000,\"cache_creation_input_tokens\":2000,\"cache_read_input_tokens\":3000,\"output_tokens\":100}}\n",
    );
    let now = ts_ms("2026-09-29T12:00:00Z").unwrap();
    let v = day(&parsed_of(text), &HashMap::new(), None, now, 0);
    let j = serde_json::to_value(&v).unwrap();
    assert_eq!(
        keys(&j),
        ["all", "day", "repos", "schema", "tz_offset"],
        "{j}"
    );
    assert_eq!(j["schema"], 1, "{j}");
    assert_eq!(
        keys(&j["all"]),
        [
            "context",
            "delivered",
            "for_you",
            "health",
            "memory",
            "timeline"
        ],
        "{j}"
    );
    assert_eq!(
        keys(&j["all"]["delivered"]),
        ["by_client", "last", "rows"],
        "{j}"
    );
    assert_eq!(
        keys(&j["all"]["delivered"]["last"][0]),
        ["file", "id", "title"],
        "{j}"
    );
    assert_eq!(
        keys(&j["all"]["context"]),
        ["fael_tokens", "session_tokens", "share"],
        "{j}"
    );
    assert_eq!(
        keys(&j["all"]["memory"]),
        ["added", "closed", "open_issues", "superseded"],
        "{j}"
    );
    assert_eq!(
        keys(&j["all"]["health"]),
        ["ignored_blocks", "stale_issues"],
        "{j}"
    );
    assert_eq!(
        keys(&j["all"]["timeline"]),
        ["bucket_min", "delivered", "fael_tokens"],
        "{j}"
    );
    assert_eq!(j["all"]["timeline"]["bucket_min"], 15, "{j}");
    assert_eq!(
        j["all"]["timeline"]["delivered"].as_array().unwrap().len(),
        96,
        "{j}"
    );
    assert_eq!(
        j["all"]["timeline"]["fael_tokens"]
            .as_array()
            .unwrap()
            .len(),
        96,
        "{j}"
    );
    assert!(j["all"]["for_you"].is_null(), "no writer, so null: {j}");
    // share is measured here: 150 fael / 6000 input-side session
    assert_eq!(v.all.context.fael_tokens, 150);
    assert_eq!(v.all.context.session_tokens, 6000);
    assert_eq!(v.all.context.share, Some(150.0 / 6000.0));
    // …and null (never zero) when nothing was measured
    let bare = use_row("2026-09-29T10:00:00Z", "/r", "claude", "read", 100, "\"A\"");
    let w = day(&parsed_of(&bare), &HashMap::new(), None, now, 0);
    assert!(serde_json::to_value(&w).unwrap()["all"]["context"]["share"].is_null());
}

#[test]
fn panels_join_usage_with_the_log() {
    let now = ts_ms("2026-09-29T12:00:00Z").unwrap();
    let mut i1 = row("I1", "issue", "2026-09-29T09:00:00Z", "ploy-x");
    i1.to = Some("kire-abc1".into());
    i1.urgent = Some(1.0);
    i1.revisit = Some("2026-09-20".into());
    i1.title = Some("Fix auth".into());
    i1.files = vec!["src/a.rs".into()];
    let mut i2 = row("I2", "issue", "2026-09-29T10:00:00Z", "ploy-x");
    i2.to = Some("kire".into());
    i2.revisit = Some("2099-01-01".into());
    let i3 = row("I3", "issue", "2026-09-01T00:00:00Z", "kire-abc1");
    let d1 = row("D1", "decision", "2026-09-29T08:00:00Z", "kire-abc1");
    let mut n1 = row("N1", "note", "2026-09-29T08:30:00Z", "kire-abc1");
    n1.supersedes = Some("D0".into());
    let mut close_old = row("C1", "", "2026-09-29T11:00:00Z", "kire-abc1");
    close_old.reference = Some("OLD".into());
    let log = Log {
        rows: vec![i1, i2, i3, d1, n1],
        closes: vec![close_old],
        warnings: vec![],
    };
    let logs: HashMap<String, Log> = [("/r".to_string(), log)].into_iter().collect();
    // one push of I1 (title + file resolve), two stop events: 07:00 was
    // followed by rows, 11:30 was ignored
    let text = use_row("2026-09-29T09:30:00Z", "/r", "claude", "read", 10, "\"I1\"")
        + &use_row("2026-09-29T07:00:00Z", "/r", "claude", "stop-warn", 1, "")
        + &use_row("2026-09-29T11:30:00Z", "/r", "claude", "stop-warn", 1, "");
    let v = day(&parsed_of(&text), &logs, Some("kire-abc1"), now, 0);
    assert_eq!(v.all.delivered.rows, 1);
    assert_eq!(v.all.delivered.last[0].title, "Fix auth");
    assert_eq!(v.all.delivered.last[0].file, "src/a.rs");
    let m = &v.all.memory;
    assert_eq!(m.added.get("issue"), Some(&2));
    assert_eq!(m.added.get("decision"), Some(&1));
    assert_eq!(m.added.get("note"), Some(&1));
    assert_eq!((m.closed, m.open_issues, m.superseded), (1, 3, 1));
    let f = v.all.for_you.as_ref().unwrap();
    assert_eq!(f.rows, 2);
    assert_eq!(f.from.get("ploy-x"), Some(&2));
    assert_eq!((f.urgent, f.revisit_due), (1, 1));
    assert_eq!(v.all.health.ignored_blocks, 1);
    assert_eq!(v.all.health.stale_issues, 1);
}

#[test]
fn repos_split_and_sum() {
    let now = ts_ms("2026-09-29T12:00:00Z").unwrap();
    let text = use_row("2026-09-29T10:00:00Z", "/b", "codex", "edit", 7, "\"X\"")
        + &use_row("2026-09-29T10:05:00Z", "/a", "claude", "read", 3, "\"Y\"")
        + &use_row("2026-09-29T10:10:00Z", "/a", "claude", "read", 5, "");
    let v = day(&parsed_of(&text), &HashMap::new(), Some("me-1"), now, 0);
    assert_eq!(v.repos.len(), 2);
    assert_eq!(v.repos[0].repo, "/a");
    assert_eq!(v.repos[1].repo, "/b");
    assert_eq!(v.repos[0].panels.delivered.rows, 1);
    assert_eq!(v.repos[1].panels.delivered.rows, 1);
    assert_eq!(v.all.delivered.rows, 2);
    assert_eq!(v.all.context.fael_tokens, 15);
    assert_eq!(v.all.timeline.delivered.iter().sum::<usize>(), 2);
    assert_eq!(v.all.timeline.fael_tokens.iter().sum::<usize>(), 15);
    assert!(v.all.for_you.as_ref().is_some_and(|f| f.rows == 0));
}

#[test]
fn log_only_repo_still_shows() {
    let now = ts_ms("2026-09-29T12:00:00Z").unwrap();
    let log = Log {
        rows: vec![row("D9", "decision", "2026-09-29T08:00:00Z", "me-1")],
        closes: vec![],
        warnings: vec![],
    };
    let logs: HashMap<String, Log> = [("/l".to_string(), log)].into_iter().collect();
    let v = day(&parsed_of(""), &logs, None, now, 0);
    assert_eq!(v.repos.len(), 1);
    assert_eq!(v.all.delivered.rows, 0);
    assert_eq!(v.all.memory.added.get("decision"), Some(&1));
}

#[test]
fn ten_thousand_rows_stay_under_budget() {
    let base = ts_ms("2026-09-29T00:00:00Z").unwrap();
    let mut text = String::new();
    for i in 0..10_000 {
        let ts = fael_core::rfc3339((base + i as i64 * 8_000) as u64);
        text.push_str(&use_row(&ts, "/r", "claude", "read", 10, "\"A\""));
    }
    let now = ts_ms("2026-09-29T23:00:00Z").unwrap();
    let t = std::time::Instant::now();
    let p = parsed_of(&text);
    let v = day(&p, &HashMap::new(), None, now, 0);
    assert_eq!(v.all.delivered.rows, 10_000);
    assert_eq!(v.all.timeline.delivered.iter().sum::<usize>(), 10_000);
    if !cfg!(debug_assertions) {
        assert!(t.elapsed().as_millis() < 50, "{:?}", t.elapsed());
    }
}
