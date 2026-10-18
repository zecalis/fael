//! Stamped retire counts (PLAN-fael-row-retire r2): retire events whose row
//! carries `extra.shown` — the captured fact of which line (`held`, `done`,
//! later `add`/`start`) named the target in the writing session (r1). One
//! entry per stamped event, not per row: a row bumped then closed, both
//! stamped, counts in both. Pure: loaded logs and the stats window in.
//!
//! The window is the stats window: a retire counts when stamped at or after
//! its repo's first usage (`Parsed::first_seen`, which already reflects
//! `--since`). Rows filed before r1 have no stamp and never count — the old
//! `said.held` earned rule (24h after the line, earliest event only) is
//! unchanged, and m2 reads this block, not `earned`.

use crate::{Log, reverted, ts_ms};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};

/// Retire events stamped with one source, split by kind.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ActedCounts {
    pub close: usize,
    pub supersede: usize,
    pub bump: usize,
}

/// The `shown` sources a retire row names.
fn shown_of(row: &crate::Row) -> impl Iterator<Item = &str> {
    row.extra
        .get("shown")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
        .filter_map(|s| s.as_str())
}

/// Per-source stamped retires in the window, over every repo with usage.
/// A retire stamped `["held", "done"]` counts under both sources — each line
/// named it. Reverted supersede edges are skipped, like `retire_events`.
pub(super) fn count(
    logs: &HashMap<String, Log>,
    first_seen: &HashMap<String, i64>,
) -> BTreeMap<String, ActedCounts> {
    let mut out: BTreeMap<String, ActedCounts> = BTreeMap::new();
    for (repo, log) in logs {
        let Some(since) = first_seen.get(repo) else {
            continue;
        };
        for c in &log.closes {
            if ts_ms(&c.ts).is_none_or(|t| t < *since) {
                continue;
            }
            for s in shown_of(c) {
                out.entry(s.to_string()).or_default().close += 1;
            }
        }
        let rev = reverted(log);
        for r in &log.rows {
            if r.supersedes.is_some()
                && !rev.contains(r.id.as_str())
                && ts_ms(&r.ts).is_some_and(|t| t >= *since)
            {
                for s in shown_of(r) {
                    out.entry(s.to_string()).or_default().supersede += 1;
                }
            }
            if r.bumps.is_some() && ts_ms(&r.ts).is_some_and(|t| t >= *since) {
                for s in shown_of(r) {
                    out.entry(s.to_string()).or_default().bump += 1;
                }
            }
        }
    }
    out
}
