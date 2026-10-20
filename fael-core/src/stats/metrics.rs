//! Ask metrics over the kept usage rows — the read half of recording:
//! counts, rows-added denominators and language share. Pure: values in,
//! numbers out.

use super::ASK_WARN;
use super::aggregate::AskCount;
use crate::{Config, Log, is_carrier_row, row_language_check, ts_ms};
use std::collections::{BTreeMap, HashMap, HashSet};

/// Ask kinds — the same vocabulary `usage.jsonl` stores under `ask`, so the
/// JSON shape never drifts from what is recorded. Rejects left it on
/// 2026-10-13: the `call` line counts each once, with its reason (`friction`).
pub const ASK_ORDER: [&str; 1] = [ASK_WARN];

/// Ask counts by kind, each split by `code` (`uncoded`: a line written before
/// codes). Plain pushes carry no `ask` and never count here.
pub(super) fn ask_totals(rows: &[serde_json::Value]) -> BTreeMap<String, AskCount> {
    let mut out: BTreeMap<String, AskCount> = ASK_ORDER
        .iter()
        .map(|a| (a.to_string(), AskCount::default()))
        .collect();
    for r in rows {
        let Some(c) = r["ask"].as_str().and_then(|a| out.get_mut(a)) else {
            continue;
        };
        c.events += 1;
        c.bytes += r["bytes"].as_u64().unwrap_or(0);
        let code = r["code"].as_str().unwrap_or("uncoded");
        *c.codes.entry(code.into()).or_default() += 1;
    }
    out
}

/// Rows filed at or after their repo's first usage (`first_seen`), each row
/// once: repos in one clone share the journal, so a row seen by several
/// repos' logs counts once. Carriers (bump, restore, alias events) move
/// rows, they file none.
pub(super) fn rows_added(logs: &HashMap<String, Log>, first_seen: &HashMap<String, i64>) -> usize {
    let mut seen = HashSet::new();
    for (repo, first) in first_seen {
        let Some(log) = logs.get(repo) else { continue };
        seen.extend(
            log.rows
                .iter()
                .filter(|r| !is_carrier_row(r) && ts_ms(&r.ts).is_some_and(|t| t >= *first))
                .map(|r| r.id.as_str()),
        );
    }
    seen.len()
}

/// (rows, rows outside the accepted `[lang] rows` scripts): one global dedup
/// by id — repos in one clone share the journal, so the same row must not
/// count twice.
pub(super) fn non_english_share(logs: &HashMap<String, Log>, cfg: &Config) -> (usize, usize) {
    let mut seen = HashSet::new();
    let (mut n, mut foreign_rows) = (0usize, 0usize);
    for log in logs.values() {
        for r in log.rows.iter().filter(|r| !is_carrier_row(r)) {
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
