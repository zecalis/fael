//! Per-row push report (`--rows`) — split out of `aggregate.rs` (file-size
//! ratchet); the `RowStatus` shape stays there, this is only the join.

use super::aggregate::RowStatus;
use super::outcomes::outcomes;
use super::parse::Parsed;
use crate::{Log, closed, superseded};
use std::collections::HashMap;

/// Per-row push report: push counts against the row's current state and its
/// observed outcomes, most pushed first. `noise` = pushed ≥ 10 times — the row
/// keeps eating budget without being resolved. A repo that is gone (or never
/// had the id) reads `unknown`. A row only ever cut has no pushes and ranks
/// after the pushed ones.
pub(super) fn row_statuses(parsed: &Parsed, logs: &HashMap<String, Log>) -> Vec<RowStatus> {
    const TOP: usize = 20;
    const NOISE_PUSHES: usize = 10;
    let mut outcomes = outcomes(parsed, logs);
    let mut ids: Vec<(String, usize)> = parsed.by_id.iter().map(|(i, n)| (i.clone(), *n)).collect();
    ids.extend(
        outcomes
            .keys()
            .filter(|i| !parsed.by_id.contains_key(*i))
            .map(|i| (i.clone(), 0)),
    );
    ids.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    ids.into_iter()
        .take(TOP)
        .map(|(id, pushes)| {
            // cut-only rows never rode a push line, so `id_repos` lacks them —
            // resolve those against every loaded log before calling them gone
            let repos: Vec<&str> = match parsed.id_repos.get(&id) {
                Some(rs) => rs.iter().map(String::as_str).collect(),
                None => logs.keys().map(String::as_str).collect(),
            };
            let status = repos
                .into_iter()
                .filter_map(|repo| {
                    let log = logs.get(repo)?;
                    let (closed_set, superseded_set) = (closed(log), superseded(log));
                    if closed_set.contains(id.as_str()) {
                        Some("closed")
                    } else if superseded_set.contains(id.as_str()) {
                        Some("superseded")
                    } else if log.rows.iter().any(|r| r.id == id) {
                        Some("open")
                    } else {
                        None
                    }
                })
                .next()
                .unwrap_or("unknown")
                .to_string();
            RowStatus {
                pushes,
                status,
                noise: pushes >= NOISE_PUSHES,
                outcomes: outcomes.remove(&id).unwrap_or_default(),
                id,
            }
        })
        .collect()
}
