//! Today's panels for the desktop popover (PLAN-fael-desktop §2) — pure:
//! one local day out of parsed usage + pre-loaded logs, per repo and summed.
//! `docs/stats.md` freezes the `DayView` JSON shape (`DAY_SCHEMA`).
//!
//! Thin entry only: the shapes, `day`, and the `all` rollup live here; the
//! per-repo panel builders sit in `panels`.

mod panels;

use super::parse::{Parsed, UsageRow};
use crate::{Log, is_alias_row, is_carrier_row, ts_ms};
use panels::{context_of, for_you_of, health_of, memory_of, timeline_of};
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};

/// Schema of the `DayView` JSON shape below — versioned on its own, apart
/// from `STATS_SCHEMA`: bumping one never bumps the other.
pub const DAY_SCHEMA: u32 = 2;
/// Timeline resolution: 15-minute buckets, 96 per day.
pub const BUCKET_MIN: usize = 15;
pub const BUCKETS: usize = 96;
/// An open issue older than this reads as stale in the health panel.
pub const STALE_DAYS: i64 = 14;
/// Last delivered rows shown per panel.
pub(super) const LAST_N: usize = 5;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LastRow {
    pub id: String,
    pub title: String,
    pub file: String,
}

/// Rows handed to agents today: pushes (usage events carrying ≥1 id), not
/// distinct ids — the number ticks with every injection, like the menu bar.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Delivered {
    pub rows: usize,
    pub by_client: BTreeMap<String, usize>,
    pub last: Vec<LastRow>,
}

/// Fael's input tokens vs the sessions' input-side context (`real_tokens`
/// in + cache-create + cache-read; output is not context). `share` is
/// `None` ("—", never an estimate) when nothing was measured — or when the
/// measured session sum is zero, where a ratio means nothing.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Context {
    pub fael_tokens: usize,
    pub session_tokens: u64,
    pub share: Option<f64>,
}

/// `added` counts log rows filed today by kind (carriers never count);
/// `closed` counts close rows filed today. `open_issues` and `superseded`
/// are current state, not day-scoped — the backlog as it stands tonight.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Memory {
    pub added: BTreeMap<String, usize>,
    pub closed: usize,
    pub open_issues: usize,
    pub superseded: usize,
}

/// Open rows routed to the viewer. `None` (null) when no writer is set —
/// hidden, never guessed.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ForYou {
    pub rows: usize,
    pub from: BTreeMap<String, usize>,
    pub urgent: usize,
    pub revisit_due: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Health {
    pub stale_issues: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Timeline {
    pub bucket_min: usize,
    pub delivered: Vec<usize>,
    pub fael_tokens: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DayPanels {
    pub delivered: Delivered,
    pub context: Context,
    pub memory: Memory,
    pub for_you: Option<ForYou>,
    pub health: Health,
    pub timeline: Timeline,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RepoDay {
    pub repo: String,
    #[serde(flatten)]
    pub panels: DayPanels,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DayView {
    pub schema: u32,
    pub day: String,
    pub tz_offset: String,
    pub all: DayPanels,
    pub repos: Vec<RepoDay>,
}

/// One local day out of parsed usage. `logs` covers the repos (a repo with
/// no entry reads empty); `me` is the viewer's writer id (`None` hides
/// `for_you`); `now_ms` picks the day, `tz_min` its timezone in minutes
/// east of UTC. No spawn, no clock, no filesystem here.
pub fn day(
    parsed: &Parsed,
    logs: &HashMap<String, Log>,
    me: Option<&str>,
    now_ms: i64,
    tz_min: i32,
) -> DayView {
    let noon = day_number(now_ms, tz_min);
    let start = noon * 86_400_000 - tz_min as i64 * 60_000;
    let label = crate::rfc3339((noon * 86_400_000).max(0) as u64);
    let ctx = Ctx {
        me,
        noon,
        tz_min,
        day: label.get(..10).unwrap_or("").to_string(),
        now_ms,
        start_ms: start,
    };
    let today: Vec<&UsageRow> = parsed
        .rows
        .iter()
        .filter(|r| day_number(r.ms, tz_min) == noon)
        .collect();
    let mut names: Vec<String> = vec![];
    for r in &today {
        if !names.iter().any(|n| n == &r.repo) {
            names.push(r.repo.clone());
        }
    }
    for k in logs.keys() {
        if !names.iter().any(|n| n == k)
            && logs
                .get(k)
                .is_some_and(|log| log_added_today(log, noon, tz_min))
        {
            names.push(k.clone());
        }
    }
    names.sort();
    let empty = Log::default();
    // worktrees of one clone share one journal, so the `all` log panels read
    // the union of the logs deduped by id — summing per repo counted each
    // clone's rows once per worktree
    let mut union = Log::default();
    for repo in &names {
        if let Some(log) = logs.get(repo) {
            union.rows.extend(log.rows.iter().cloned());
            union.closes.extend(log.closes.iter().cloned());
        }
    }
    crate::log::dedupe_ids(&mut union.rows);
    crate::log::dedupe_ids(&mut union.closes);
    let mut repos = Vec::with_capacity(names.len());
    for repo in &names {
        let log = logs.get(repo).unwrap_or(&empty);
        let rows: Vec<&UsageRow> = today
            .iter()
            .filter(|r| r.repo.as_str() == repo.as_str())
            .copied()
            .collect();
        repos.push(RepoDay {
            repo: repo.clone(),
            panels: panels::panels(&rows, &panels::lookup(log), log, &ctx),
        });
    }
    DayView {
        schema: DAY_SCHEMA,
        day: ctx.day.clone(),
        tz_offset: tz_string(tz_min),
        all: sum_all(&today, &repos, &union, &ctx),
        repos,
    }
}

pub(super) struct Ctx<'a> {
    me: Option<&'a str>,
    noon: i64,
    tz_min: i32,
    day: String,
    now_ms: i64,
    start_ms: i64,
}

/// The `all` rollup: context/timeline recomputed from the usage union (so
/// `share` is exact, not averaged), the log panels from `union` (the repos'
/// logs deduped by id), `delivered` summed, `last`
/// newest-first.
fn sum_all(today: &[&UsageRow], repos: &[RepoDay], union: &Log, ctx: &Ctx) -> DayPanels {
    let mut all = DayPanels {
        delivered: Delivered {
            rows: 0,
            by_client: BTreeMap::new(),
            last: vec![],
        },
        context: context_of(today),
        memory: memory_of(union, ctx.noon, ctx.tz_min),
        for_you: for_you_of(union, ctx.me, &ctx.day),
        health: health_of(union, ctx.now_ms),
        timeline: timeline_of(today, ctx.start_ms),
    };
    let mut latest: HashMap<&str, i64> = HashMap::new();
    for r in today {
        for id in &r.ids {
            latest
                .entry(id.as_str())
                .and_modify(|m| *m = (*m).max(r.ms))
                .or_insert(r.ms);
        }
    }
    let mut ids: Vec<(&str, i64)> = latest.into_iter().collect();
    ids.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
    for (id, _) in ids.into_iter().take(LAST_N) {
        let (title, file) = repos
            .iter()
            .find_map(|v| last_in(&v.panels.delivered.last, id))
            .unwrap_or((id.to_string(), String::new()));
        all.delivered.last.push(LastRow {
            id: id.to_string(),
            title,
            file,
        });
    }
    for v in repos {
        all.delivered.rows += v.panels.delivered.rows;
        merge_map(&mut all.delivered.by_client, &v.panels.delivered.by_client);
    }
    all
}

fn last_in(last: &[LastRow], id: &str) -> Option<(String, String)> {
    last.iter()
        .find(|l| l.id == id)
        .map(|l| (l.title.clone(), l.file.clone()))
}

fn merge_map(into: &mut BTreeMap<String, usize>, add: &BTreeMap<String, usize>) {
    for (k, v) in add {
        *into.entry(k.clone()).or_default() += v;
    }
}

/// A repo whose log gained a row today shows up even with no usage — filing
/// from the CLI is today's activity too.
fn log_added_today(log: &Log, noon: i64, tz_min: i32) -> bool {
    log.rows.iter().any(|r| {
        !r.kind.is_empty()
            && !is_alias_row(r)
            && !is_carrier_row(r)
            && ts_ms(&r.ts).is_some_and(|t| day_number(t, tz_min) == noon)
    })
}

/// Whole days since the epoch in the viewer's timezone — the one predicate
/// every "today" check shares, so a +07:00 midnight never splits a day.
pub(super) fn day_number(ms: i64, tz_min: i32) -> i64 {
    (ms + tz_min as i64 * 60_000).div_euclid(86_400_000)
}

fn tz_string(tz_min: i32) -> String {
    let (sign, abs) = if tz_min < 0 {
        ('-', (-tz_min) as u32)
    } else {
        ('+', tz_min as u32)
    };
    format!("{sign}{:02}:{:02}", abs / 60, abs % 60)
}
