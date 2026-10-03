//! Per-repo panel builders for the day view — one scope (a repo's usage
//! rows and its log) in, one `DayPanels` out. Pure.

use super::super::parse::UsageRow;
use super::{BUCKET_MIN, BUCKETS, Context, Ctx, Delivered, ForYou, Health, LAST_N, LastRow};
use super::{DayPanels, Memory, STALE_DAYS, Timeline};
use crate::{Log, Row, closed, is_alias_row, is_carrier_row, superseded, to_matches, ts_ms};
use std::collections::{BTreeMap, HashMap, HashSet};

pub(super) fn panels(
    rows: &[&UsageRow],
    by_id: &HashMap<&str, &Row>,
    log: &Log,
    ctx: &Ctx,
) -> DayPanels {
    DayPanels {
        delivered: delivered(rows, by_id),
        context: context_of(rows),
        memory: memory_of(log, ctx.noon, ctx.tz_min),
        for_you: for_you_of(log, ctx.me, &ctx.day),
        health: health_of(log, ctx.now_ms),
        timeline: timeline_of(rows, ctx.start_ms),
    }
}

pub(super) fn delivered(rows: &[&UsageRow], by_id: &HashMap<&str, &Row>) -> Delivered {
    let mut by_client: BTreeMap<String, usize> = BTreeMap::new();
    let mut latest: HashMap<&str, i64> = HashMap::new();
    let mut n = 0;
    for r in rows {
        if r.ids.is_empty() {
            continue;
        }
        n += 1;
        *by_client.entry(r.client.clone()).or_default() += 1;
        for id in &r.ids {
            latest
                .entry(id.as_str())
                .and_modify(|m| *m = (*m).max(r.ms))
                .or_insert(r.ms);
        }
    }
    let mut ids: Vec<(&str, i64)> = latest.into_iter().collect();
    ids.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
    let last = ids
        .into_iter()
        .take(LAST_N)
        .map(|(id, _)| {
            let (title, file) = by_id
                .get(id)
                .map(|r| {
                    (
                        r.display_title(),
                        r.files.first().cloned().unwrap_or_default(),
                    )
                })
                .unwrap_or((id.to_string(), String::new()));
            LastRow {
                id: id.to_string(),
                title,
                file,
            }
        })
        .collect();
    Delivered {
        rows: n,
        by_client,
        last,
    }
}

pub(super) fn context_of(rows: &[&UsageRow]) -> Context {
    let fael_tokens: usize = rows.iter().map(|r| r.toks).sum();
    let (mut session, mut samples) = (0u64, 0usize);
    for r in rows {
        if let Some(v) = r.real_input {
            session += v;
            samples += 1;
        }
    }
    let share = (samples > 0 && session > 0).then(|| fael_tokens as f64 / session as f64);
    Context {
        fael_tokens,
        session_tokens: session,
        share,
    }
}

pub(super) fn memory_of(log: &Log, noon: i64, tz_min: i32) -> Memory {
    let mut added: BTreeMap<String, usize> = BTreeMap::new();
    for r in &log.rows {
        if r.kind.is_empty() || is_alias_row(r) || is_carrier_row(r) {
            continue;
        }
        if ts_ms(&r.ts).is_some_and(|t| super::day_number(t, tz_min) == noon) {
            *added.entry(r.kind.clone()).or_default() += 1;
        }
    }
    let closed_n = log
        .closes
        .iter()
        .filter(|r| ts_ms(&r.ts).is_some_and(|t| super::day_number(t, tz_min) == noon))
        .count();
    Memory {
        added,
        closed: closed_n,
        open_issues: open_issues(log).len(),
        superseded: superseded(log).len(),
    }
}

/// Open issues as they stand — the backlog `open_issues` and the stale
/// count share, so the two cannot drift.
fn open_issues(log: &Log) -> Vec<&Row> {
    let hide: HashSet<&str> = closed(log).union(&superseded(log)).copied().collect();
    log.rows
        .iter()
        .filter(|r| {
            r.kind == "issue"
                && !hide.contains(r.id.as_str())
                && !is_alias_row(r)
                && !is_carrier_row(r)
        })
        .collect()
}

pub(super) fn for_you_of(log: &Log, me: Option<&str>, day: &str) -> Option<ForYou> {
    let me = me.filter(|m| !m.is_empty())?;
    let hide: HashSet<&str> = closed(log).union(&superseded(log)).copied().collect();
    let mut from: BTreeMap<String, usize> = BTreeMap::new();
    let (mut urgent, mut due, mut n) = (0, 0, 0);
    for r in &log.rows {
        if hide.contains(r.id.as_str()) || is_alias_row(r) || is_carrier_row(r) {
            continue;
        }
        if !r
            .to_who()
            .is_some_and(|t| to_matches(t, me) || to_matches(me, t))
        {
            continue;
        }
        n += 1;
        *from.entry(r.by.clone()).or_default() += 1;
        urgent += r.urgent_value().is_some() as usize;
        due += r.revisit().is_some_and(|v| due_by(v, day)) as usize;
    }
    Some(ForYou {
        rows: n,
        from,
        urgent,
        revisit_due: due,
    })
}

/// A `revisit` date at or before today wakes the row — free text never
/// counts, only `YYYY-MM-DD` (or a `YYYY-MM` inside this month).
fn due_by(v: &str, day: &str) -> bool {
    let b = v.as_bytes();
    if v.len() == 10 && b[4] == b'-' && b[7] == b'-' {
        return v <= day;
    }
    v.len() == 7 && b[4] == b'-' && v <= &day[..7]
}

pub(super) fn health_of(log: &Log, now_ms: i64) -> Health {
    let cutoff = now_ms - STALE_DAYS * 86_400_000;
    let stale = open_issues(log)
        .iter()
        .filter(|r| ts_ms(&r.ts).is_some_and(|t| t < cutoff))
        .count();
    Health {
        stale_issues: stale,
    }
}

pub(super) fn timeline_of(rows: &[&UsageRow], start_ms: i64) -> Timeline {
    let mut delivered = vec![0usize; BUCKETS];
    let mut fael_tokens = vec![0usize; BUCKETS];
    for r in rows {
        let b = ((r.ms - start_ms) / (BUCKET_MIN as i64 * 60_000)) as usize;
        if b < BUCKETS {
            delivered[b] += (!r.ids.is_empty()) as usize;
            fael_tokens[b] += r.toks;
        }
    }
    super::Timeline {
        bucket_min: BUCKET_MIN,
        delivered,
        fael_tokens,
    }
}

/// First row per id — titles and files resolve through the repo's own log;
/// a repo that moved (or never had the id) leaves title = id, file = "".
pub(super) fn lookup(log: &Log) -> HashMap<&str, &Row> {
    let mut m = HashMap::new();
    for r in &log.rows {
        m.entry(r.id.as_str()).or_insert(r);
    }
    m
}
