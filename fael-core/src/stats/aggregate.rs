//! Join a `Parsed` usage with the repos' logs into one `Stats` — pure:
//! the caller loads every log in `parsed.repos()` first, plus the config and
//! the per-session constants. No spawn, no clock, no filesystem here.

use super::capture::{Capture, capture};
use super::metrics::{added_since, ask_totals, non_english_share};
use super::parse::Parsed;
use super::retire::{Retired, retired};
use super::value::{Value, value};
use crate::{Config, Log, closed, rfc3339, superseded};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Count {
    pub events: usize,
    pub est_tokens: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TopRow {
    pub id: String,
    pub pushes: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AskCount {
    pub events: usize,
    pub bytes: u64,
}

/// Schema of the `Stats` JSON shape below. Bump it only when a field is
/// removed, renamed, retyped or redefined — adding a field never bumps
/// (readers skip unknown keys), it just gets a `docs/stats.md` changelog line.
pub const STATS_SCHEMA: u32 = 2;

/// Bytes the agent pays every session before saying anything — measured by
/// the caller (CLI), never estimated here.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Constants {
    pub skill_bytes: usize,
    pub skill_est: usize,
    pub mcp_schema_bytes: usize,
    pub mcp_schema_est: usize,
}

impl From<(usize, usize, usize, usize)> for Constants {
    fn from(t: (usize, usize, usize, usize)) -> Self {
        Constants {
            skill_bytes: t.0,
            skill_est: t.1,
            mcp_schema_bytes: t.2,
            mcp_schema_est: t.3,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Rounds {
    pub rows_added: usize,
    pub since: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NonEnglish {
    pub rows: usize,
    pub non_english: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RowStatus {
    pub id: String,
    pub pushes: usize,
    pub status: String,
    pub noise: bool,
}

/// The whole of `fael stats --json`: one struct serialised as-is, so the CLI,
/// the desktop app and any outside reader share the shape by construction.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Stats {
    pub schema: u32,
    pub events: usize,
    pub bytes: usize,
    pub est_tokens: usize,
    pub skipped_temp: usize,
    pub by_event: BTreeMap<String, Count>,
    pub by_client: BTreeMap<String, Count>,
    pub top_rows: Vec<TopRow>,
    pub asks: BTreeMap<String, AskCount>,
    pub constants: Constants,
    pub rounds: Rounds,
    pub non_english_rows: NonEnglish,
    pub capture: Capture,
    /// Pushed rows closed or superseded soon after a push (`retire.rs`).
    pub retired: Retired,
    /// The value line `fael stats` prints first (`value.rs`).
    pub value: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rows: Option<Vec<RowStatus>>,
}

/// Join parsed usage with pre-loaded logs. `logs` must cover `parsed.repos()`;
/// a repo with no entry reads as empty (its rows resolve `unknown`).
pub fn aggregate(
    parsed: &Parsed,
    logs: &HashMap<String, Log>,
    cfg: &Config,
    constants: Constants,
    with_rows: bool,
) -> Stats {
    let (mut rows_added, mut since) = (0usize, i64::MAX);
    for (repo, first) in &parsed.first_seen {
        since = since.min(*first);
        if let Some(log) = logs.get(repo) {
            rows_added += added_since(log, *first);
        }
    }
    let (row_total, foreign_rows) = non_english_share(logs, cfg);
    let (capture, retired) = (capture(parsed, logs), retired(parsed, logs));
    let value = value(parsed, logs);
    let mut top: Vec<(&String, &usize)> = parsed.by_id.iter().collect();
    top.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
    Stats {
        schema: STATS_SCHEMA,
        events: parsed.n,
        bytes: parsed.bytes,
        est_tokens: parsed.toks,
        skipped_temp: parsed.skipped,
        by_event: counts(&parsed.by_event),
        by_client: counts(&parsed.by_client),
        top_rows: top
            .into_iter()
            .take(10)
            .map(|(id, c)| TopRow {
                id: id.clone(),
                pushes: *c,
            })
            .collect(),
        asks: ask_totals(&parsed.kept)
            .into_iter()
            .map(|(a, c, b)| {
                (
                    a.to_string(),
                    AskCount {
                        events: c,
                        bytes: b,
                    },
                )
            })
            .collect(),
        constants,
        rounds: Rounds {
            rows_added,
            since: since_day(since),
        },
        non_english_rows: NonEnglish {
            rows: row_total,
            non_english: foreign_rows,
        },
        capture,
        retired,
        value,
        rows: with_rows.then(|| row_statuses(&parsed.by_id, &parsed.id_repos, logs)),
    }
}

/// The first-usage day, `YYYY-MM-DD`; empty when there is no usage.
fn since_day(since: i64) -> String {
    if since == i64::MAX {
        String::new()
    } else {
        rfc3339(since.max(0) as u64)
            .get(..10)
            .unwrap_or("")
            .to_string()
    }
}

fn counts(into: &HashMap<String, (usize, usize)>) -> BTreeMap<String, Count> {
    into.iter()
        .map(|(k, (c, t))| {
            (
                k.clone(),
                Count {
                    events: *c,
                    est_tokens: *t,
                },
            )
        })
        .collect()
}

/// Per-row push report: push counts against the row's current state, most
/// pushed first. `noise` = pushed ≥ 10 times — the row keeps eating budget
/// without being resolved. A repo that is gone (or never had the id) reads
/// `unknown`.
fn row_statuses(
    by_id: &HashMap<String, usize>,
    id_repos: &HashMap<String, Vec<String>>,
    logs: &HashMap<String, Log>,
) -> Vec<RowStatus> {
    const TOP: usize = 20;
    const NOISE_PUSHES: usize = 10;
    let mut ids: Vec<_> = by_id.iter().collect();
    ids.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
    ids.into_iter()
        .take(TOP)
        .map(|(id, pushes)| {
            let status = id_repos
                .get(id)
                .into_iter()
                .flatten()
                .filter_map(|repo| {
                    let log = logs.get(repo)?;
                    let (closed_set, superseded_set) = (closed(log), superseded(log));
                    if closed_set.contains(id.as_str()) {
                        Some("closed")
                    } else if superseded_set.contains(id.as_str()) {
                        Some("superseded")
                    } else if log.rows.iter().any(|r| &r.id == id) {
                        Some("open")
                    } else {
                        None
                    }
                })
                .next()
                .unwrap_or("unknown")
                .to_string();
            RowStatus {
                id: id.clone(),
                pushes: *pushes,
                status,
                noise: *pushes >= NOISE_PUSHES,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::aggregate;
    use crate::Config;
    use std::collections::HashMap;

    fn parsed_of(text: &str) -> super::super::parse::Parsed {
        let tmp = vec![std::path::PathBuf::from("/tmp")];
        super::super::parse::parse(text, std::path::Path::new("/work/state/usage.jsonl"), &tmp)
    }

    #[test]
    fn aggregate_counts_match_the_old_json_shape() {
        let text = concat!(
            "{\"ts\":\"2026-09-26T00:00:00.000Z\",\"repo\":\"/work/real\",\"client\":\"claude\",\"event\":\"read\",\"bytes\":10,\"est_tokens\":3,\"ids\":[\"A\"]}\n",
            "{\"ts\":\"2026-09-26T00:01:00.000Z\",\"repo\":\"/work/real\",\"client\":\"codex\",\"event\":\"edit\",\"bytes\":20,\"est_tokens\":5,\"ids\":[\"A\",\"B\"]}\n",
        );
        let p = parsed_of(text);
        let s = aggregate(
            &p,
            &HashMap::new(),
            &Config::default(),
            (1, 2, 3, 4).into(),
            true,
        );
        assert_eq!(
            (s.events, s.bytes, s.est_tokens, s.skipped_temp),
            (2, 30, 8, 0)
        );
        assert_eq!(
            s.by_event.get("read"),
            Some(&super::Count {
                events: 1,
                est_tokens: 3
            })
        );
        assert_eq!(s.top_rows.len(), 2);
        assert_eq!(s.rounds.since, "2026-09-26");
        let rows = s.rows.unwrap();
        assert!(rows.iter().all(|r| r.status == "unknown"));
    }
}
