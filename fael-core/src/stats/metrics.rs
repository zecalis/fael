//! Ask metrics over the kept usage rows — the read half of recording:
//! counts, rows-added denominators and language share. Pure: values in,
//! numbers out.

use super::{ASK_REJECT, ASK_WARN};
use crate::{Config, Log, row_language_check, ts_ms};
use std::collections::{HashMap, HashSet};

/// Ask kinds in fixed order — the same vocabulary `usage.jsonl` stores under
/// `ask`, so the JSON shape never drifts from what is recorded.
pub const ASK_ORDER: [&str; 2] = [ASK_REJECT, ASK_WARN];

/// Ask counts in fixed order (reject, warning): (count, bytes).
/// Plain pushes carry no `ask` and never count here.
pub(super) fn ask_totals(rows: &[serde_json::Value]) -> Vec<(&'static str, usize, u64)> {
    ASK_ORDER
        .into_iter()
        .map(|ask| {
            let (mut n, mut b) = (0usize, 0u64);
            for r in rows {
                if r.get("ask").and_then(|a| a.as_str()) == Some(ask) {
                    n += 1;
                    b += r
                        .get("bytes")
                        .and_then(serde_json::Value::as_u64)
                        .unwrap_or(0);
                }
            }
            (ask, n, b)
        })
        .collect()
}

/// Rows filed at or after `since_ms` (the repo's first usage).
pub(super) fn added_since(log: &Log, since_ms: i64) -> usize {
    log.rows
        .iter()
        .filter(|r| ts_ms(&r.ts).is_some_and(|t| t >= since_ms))
        .count()
}

/// (rows, rows outside the accepted `[lang] rows` scripts): one global dedup
/// by id — repos in one clone share the journal, so the same row must not
/// count twice.
pub(super) fn non_english_share(logs: &HashMap<String, Log>, cfg: &Config) -> (usize, usize) {
    let mut seen = HashSet::new();
    let (mut n, mut foreign_rows) = (0usize, 0usize);
    for log in logs.values() {
        for r in &log.rows {
            if !seen.insert(r.id.as_str()) {
                continue;
            }
            n += 1;
            if row_language_check(cfg, r.title.as_deref(), &r.text).is_some() {
                foreign_rows += 1;
            }
        }
    }
    (n, foreign_rows)
}

pub(super) fn real_in(r: &serde_json::Value) -> Option<[u64; 4]> {
    let u = r.get("real_tokens")?;
    let part = |k: &str| u.get(k).and_then(serde_json::Value::as_u64).unwrap_or(0);
    Some([
        u.get("input_tokens")?.as_u64()?,
        part("cache_creation_input_tokens"),
        part("cache_read_input_tokens"),
        part("output_tokens"),
    ])
}
