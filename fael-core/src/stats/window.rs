//! The usage window: `--since` cuts the usage text to a window before anything
//! is counted, and the outcome/flow joins share one window for the whole
//! screen. Split out of `parse.rs` (file-size ratchet); pure, like the rest
//! of core. `parse_since` in `parse.rs` applies the same cut in its own pass
//! (one parse per line); `since` below stays for its unit tests.

use crate::{Log, ts_ms};
use std::collections::HashMap;

/// `--since`: the usage lines stamped at or after `ms`, cut before `parse`
/// so every number downstream — first use per repo, rows added, capture —
/// reads as that window. A line with no readable `ts` falls outside it.
pub fn since(text: &str, ms: i64) -> String {
    text.lines()
        .filter(|l| {
            serde_json::from_str::<serde_json::Value>(l)
                .ok()
                .and_then(|v| v["ts"].as_str().and_then(ts_ms))
                .is_some_and(|t| t >= ms)
        })
        .flat_map(|l| [l, "\n"])
        .collect()
}

/// A `--since` value in `tz_min` minutes east of UTC: `YYYY-MM-DD` is that
/// day's local midnight (a BKK `2026-10-30` is `2026-10-29T17:00:00Z`), a
/// full RFC 3339 time keeps its own zone and ignores `tz_min`.
pub fn since_arg(s: &str, tz_min: i32) -> Option<i64> {
    match s.len() {
        10 => ts_ms(&format!("{s}T00:00:00Z")).map(|ms| ms - tz_min as i64 * 60_000),
        _ => ts_ms(s),
    }
}

/// One window for the whole screen: explicit `--since` (`Some(0)` = all),
/// else the oldest usage read, else the oldest log row (fresh clone).
/// `None` = nothing to window on; joins read unwindowed, never error.
pub fn outcome_window(
    since: Option<i64>,
    logs: &HashMap<String, Log>,
    first_seen: Option<i64>,
) -> Option<i64> {
    if let Some(ms) = since {
        return Some(ms);
    }
    if let Some(ms) = first_seen {
        return Some(ms);
    }
    logs.values()
        .flat_map(|log| log.rows.iter().chain(log.closes.iter()))
        .filter_map(|r| ts_ms(&r.ts))
        .min()
}

/// Where the outcome window came from — printed beside the numbers so two
/// `since` lines on one screen can never be mistaken for one window.
pub fn window_source(
    since: Option<i64>,
    first_seen: Option<i64>,
    logs: &HashMap<String, Log>,
) -> &'static str {
    if let Some(ms) = since {
        return if ms == 0 { "all" } else { "since" };
    }
    if first_seen.is_some() {
        return "usage";
    }
    let any = logs
        .values()
        .flat_map(|log| log.rows.iter().chain(log.closes.iter()))
        .any(|r| ts_ms(&r.ts).is_some());
    if any { "ledger" } else { "none" }
}

/// `YYYY-MM-DD` of `outcome_window` (`"all"` / `""`), beside it by design.
pub fn window_str(window: Option<i64>) -> String {
    match window {
        Some(0) => "all".to_string(),
        Some(ms) => crate::rfc3339(ms.max(0) as u64)
            .get(..10)
            .unwrap_or("")
            .to_string(),
        None => String::new(),
    }
}
