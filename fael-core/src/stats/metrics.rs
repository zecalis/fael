//! Ask metrics over the kept usage rows — the read half of recording:
//! counts, repeat blocks, rows-added denominators, language share and the
//! post-block real-token cost. Pure: values in, numbers out.

use super::parse::StopBlock;
use super::{ASK_BLOCK, ASK_REJECT, ASK_WARN};
use crate::{Config, Log, row_language_check, ts_ms};
use std::collections::{HashMap, HashSet};

/// Ask kinds in fixed order — the same vocabulary `usage.jsonl` stores under
/// `ask`, so the JSON shape never drifts from what is recorded.
pub const ASK_ORDER: [&str; 3] = [ASK_REJECT, ASK_BLOCK, ASK_WARN];

/// Ask counts in fixed order (reject, stop-block, warning): (count, bytes).
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

/// Repeat stop-blocks: a block that follows another block in the same session
/// with no row filed between them. Session-less rows never count.
pub(super) fn repeat_blocks(blocks: &[StopBlock], logs: &HashMap<String, Log>) -> usize {
    let mut by_session: HashMap<&str, Vec<(i64, &str)>> = HashMap::new();
    for b in blocks {
        if !b.session.is_empty() {
            by_session
                .entry(&b.session)
                .or_default()
                .push((b.ms, &b.repo));
        }
    }
    let mut repeat = 0usize;
    for times in by_session.values() {
        let mut ts = times.clone();
        ts.sort();
        for w in ts.windows(2) {
            let [(prev, _), (cur, repo)] = w else {
                continue;
            };
            let gap_has_row = logs.get(*repo).is_some_and(|log| {
                log.rows
                    .iter()
                    .any(|r| ts_ms(&r.ts).is_some_and(|t| t > *prev && t <= *cur))
            });
            if !gap_has_row {
                repeat += 1;
            }
        }
    }
    repeat
}

/// Rows filed at or after `since_ms` — the denominator for "rows that took
/// their own round after a block vs rows that rode along".
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

/// Mean real-token cost of the round after a stop-block: each block attributes
/// the next same-session usage row that carries `real_tokens`. Returns
/// (samples, avg input, avg cache-create, avg cache-read, avg output); zero
/// samples when no transcript had `usage`.
pub(super) fn post_block_cost(rows: &[serde_json::Value]) -> (usize, u64, u64, u64, u64) {
    let mut ev: Vec<(i64, &str, bool, Option<[u64; 4]>)> = vec![];
    for r in rows {
        let (Some(ts), Some(session)) = (
            r.get("ts").and_then(|t| t.as_str()).and_then(ts_ms),
            r.get("session").and_then(|s| s.as_str()),
        ) else {
            continue;
        };
        if session.is_empty() {
            continue;
        }
        ev.push((
            ts,
            session,
            r.get("ask").and_then(|a| a.as_str()) == Some(ASK_BLOCK),
            real_in(r),
        ));
    }
    ev.sort_by_key(|e| e.0);
    let mut pending: HashMap<&str, bool> = HashMap::new();
    let (mut n, mut sums) = (0usize, [0u64; 4]);
    for (_, session, block, real) in ev {
        if block {
            pending.insert(session, true);
        } else if let Some(t) = real
            && pending.remove(session).is_some()
        {
            n += 1;
            for (i, v) in t.iter().enumerate() {
                sums[i] += v;
            }
        }
    }
    if n == 0 {
        return (0, 0, 0, 0, 0);
    }
    let avg = |i: usize| sums[i] / n as u64;
    (n, avg(0), avg(1), avg(2), avg(3))
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

#[cfg(test)]
mod tests {
    use super::post_block_cost;

    #[test]
    fn post_block_cost_joins_block_to_next_real() {
        let row = |ts: &str, ask: &str, session: &str, real: bool| {
            let mut v = serde_json::json!({"ts": ts, "session": session, "ask": ask});
            if real {
                v["real_tokens"] = serde_json::json!({"input_tokens": 1000,
                    "cache_creation_input_tokens": 2000, "cache_read_input_tokens": 3000,
                    "output_tokens": 100});
            }
            v
        };
        let rows = vec![
            row("2026-09-28T00:00:01Z", "stop-block", "s1", false),
            row("2026-09-28T00:00:02Z", "", "s1", true),
            row("2026-09-28T00:00:03Z", "stop-block", "s1", false),
            row("2026-09-28T00:00:04Z", "", "s1", false),
            row("2026-09-28T00:00:05Z", "stop-block", "s2", false),
        ];
        assert_eq!(post_block_cost(&rows), (1, 1000, 2000, 3000, 100));
        assert_eq!(post_block_cost(&[]), (0, 0, 0, 0, 0));
    }
}
