//! Join a `Parsed` usage with the repos' logs into one `Stats` — pure:
//! the caller loads every log in `parsed.repos()` first, plus the config and
//! the per-session constants. No spawn, no clock, no filesystem here.

use super::capture::{Capture, capture};
use super::experience::{Commits, Experience, experience};
use super::flow::{Flow, WeekCount, flow as flow_stats, throughput as throughput_stats};
use super::friction::{Friction, friction};
use super::metrics::{ask_totals, non_english_share, rows_added as count_rows_added};
use super::outcomes::{OUTCOMES_V, RowOutcomes, outcomes};
use super::parse::Parsed;
use super::retire::{Retired, retired};
use super::said::{KindYield, yields};
use super::unused::{UnusedRow, unused};
use super::value::{Value, value};
use crate::{Config, Log, closed, rfc3339, rfc3339_in, superseded, tz_offset_string};
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

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct AskCount {
    pub events: usize,
    pub bytes: u64,
    /// By warning code (`hook/codes.rs`: untitled, long_text, no_key, …).
    pub codes: BTreeMap<String, usize>,
}

/// Schema of the `Stats` JSON shape below. Bump it only when a field is
/// removed, renamed, retyped or redefined — adding a field never bumps
/// (readers skip unknown keys), it just gets a `docs/stats.md` changelog line.
pub const STATS_SCHEMA: u32 = 5;

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
    /// Canonical UTC instant of the first usage (`""` when there is no
    /// usage) — `since` above is its UTC calendar day, `since_local` the
    /// same instant in `tz_offset`, so evidence reconstructs cross-timezone.
    pub since_utc: String,
    pub since_local: String,
    pub tz_offset: String,
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
    /// What happened to the row after fael said or cut it (`outcomes.rs`).
    pub outcomes: RowOutcomes,
}

/// The whole of `fael stats --json`: one struct serialised as-is, so the CLI,
/// the desktop app and any outside reader share the shape by construction.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Stats {
    pub schema: u32,
    /// Version of the outcome definitions behind `rows[].outcomes`.
    pub outcomes_v: u32,
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
    /// Open decisions/issues pushed ≥ `UNUSED_PUSHES` times, never in context
    /// at an edit — for a human to close or bump (`unused.rs`).
    pub unused_rows: Vec<UnusedRow>,
    /// Yield per line kind: what hooks said, and what the same session acted
    /// on after (`said.rs`) — an upper bound, read only to cut.
    pub said: BTreeMap<String, KindYield>,
    /// Rejects, helps and repeat finds per agent call, and first-call
    /// success (`friction.rs`) — what an agent paid to get its answer.
    pub friction: Friction,
    /// Calls per command and flag, agent and human, ok/help/reject
    /// (`by_flag.rs`) — which flags are really used.
    pub usage: super::by_flag::FlagUsage,
    /// Call shapes repeated like a test run that forgot `FAEL_STATE_DIR`
    /// (`leak.rs`); absent from the JSON when none.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub test_leaks: Vec<super::Leak>,
    /// Confirmed repeats, the edits they read against, and useful shows
    /// per token (`repeat.rs`, PLAN-fael-context-loop §3).
    pub context_loop: super::repeat::ContextLoop,
    /// Fixes, checks and capture recall (`experience.rs`,
    /// PLAN-fael-experience-loop chunk 4).
    pub experience: Experience,
    /// This repo's rows by state per kind (`lifecycle.rs`): empty from `aggregate`,
    /// which never sees the disk — the adapter fills it for the repo it runs in.
    pub lifecycle: super::Lifecycle,
    /// Flow time (filed→closed, start→done, done→merged) with merge coverage.
    pub flow: Flow,
    /// Chunk dones per calendar week (Monday, UTC) from `Run.ended`.
    pub throughput: Vec<WeekCount>,
    /// Runs with and without an `after` hand-off in the brief, side by side (`handoff.rs`).
    pub handoff_runs: super::HandoffRuns,
    /// Push events and fael tokens per UTC day over the window (`days.rs`).
    pub days: Vec<super::days::DayCount>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rows: Option<Vec<RowStatus>>,
}

/// Join parsed usage with pre-loaded logs. `logs` must cover `parsed.repos()`;
/// a repo with no entry reads as empty (its rows resolve `unknown`). A repo
/// with no `commits` entry has no fix commits. `runs`/`merges` feed the flow
/// spans; empty when the clone has no crew.db or git. `tz_min` is minutes
/// east of UTC for the `since_local` render — storage stays UTC.
#[allow(clippy::too_many_arguments)]
pub fn aggregate(
    parsed: &Parsed,
    logs: &HashMap<String, Log>,
    commits: &Commits,
    runs: &[super::flow::FlowRun],
    merges: &[super::flow::FlowMerge],
    cfg: &Config,
    constants: Constants,
    with_rows: bool,
    tz_min: i32,
) -> Stats {
    let rows_added = count_rows_added(logs, &parsed.first_seen);
    let since = parsed
        .first_seen
        .values()
        .copied()
        .min()
        .unwrap_or(i64::MAX);
    let (row_total, foreign_rows) = non_english_share(logs, cfg);
    let (capture, retired) = (capture(parsed, logs), retired(parsed, logs));
    let value = value(parsed, logs);
    let mut top: Vec<(&String, &usize)> = parsed.by_id.iter().collect();
    top.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
    Stats {
        schema: STATS_SCHEMA,
        outcomes_v: OUTCOMES_V,
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
        asks: ask_totals(&parsed.kept),
        constants,
        rounds: Rounds {
            rows_added,
            since: since_day(since),
            since_utc: since_stamp(since),
            since_local: since_stamp_local(since, tz_min),
            tz_offset: tz_offset_string(tz_min),
        },
        non_english_rows: NonEnglish {
            rows: row_total,
            non_english: foreign_rows,
        },
        capture,
        retired,
        value,
        unused_rows: unused(parsed, logs),
        said: yields(parsed, logs),
        friction: friction(parsed),
        usage: super::by_flag::usage(parsed),
        test_leaks: super::leak::leaks(parsed),
        context_loop: super::repeat::context_loop(parsed, logs),
        experience: experience(parsed, logs, commits),
        lifecycle: Default::default(),
        flow: flow_stats(parsed, logs, runs, merges),
        throughput: throughput_stats(parsed, runs),
        handoff_runs: super::handoff_runs(parsed, runs),
        days: super::days::days(parsed),
        rows: with_rows.then(|| row_statuses(parsed, logs)),
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

/// The first-usage instant in canonical UTC; empty when there is no usage.
fn since_stamp(since: i64) -> String {
    if since == i64::MAX {
        String::new()
    } else {
        rfc3339(since.max(0) as u64)
    }
}

/// The same instant in the viewer's zone; empty when there is no usage.
fn since_stamp_local(since: i64, tz_min: i32) -> String {
    if since == i64::MAX {
        String::new()
    } else {
        rfc3339_in(since.max(0) as u64, tz_min)
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

/// Per-row push report: push counts against the row's current state and its
/// observed outcomes, most pushed first. `noise` = pushed ≥ 10 times — the row
/// keeps eating budget without being resolved. A repo that is gone (or never
/// had the id) reads `unknown`. A row only ever cut has no pushes and ranks
/// after the pushed ones.
fn row_statuses(parsed: &Parsed, logs: &HashMap<String, Log>) -> Vec<RowStatus> {
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
            &HashMap::new(),
            &[],
            &[],
            &Config::default(),
            (1, 2, 3, 4).into(),
            true,
            420,
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
        assert_eq!(s.rounds.since_utc, "2026-09-26T00:00:00.000Z");
        assert_eq!(s.rounds.since_local, "2026-09-26T07:00:00.000+07:00");
        assert_eq!(s.rounds.tz_offset, "+07:00");
        let rows = s.rows.unwrap();
        assert!(rows.iter().all(|r| r.status == "unknown"));
    }
}
