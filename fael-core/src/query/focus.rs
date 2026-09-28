//! L3 rank + L4 select for the read/edit push (PLAN-fael-push-focus chunk
//! 1): bucket each gathered row into Now | File | Background, then cut to
//! the row cap. Pure — no git spawn, no file reads; chunk 2 builds the
//! Focus at session start and the push only reads it.

use crate::Row;
use std::collections::HashSet;

/// What the session works on: the start branch, the active plan chunk, and
/// the keys of my open rows on this branch. Chunk 2 builds it; the push
/// with no session state uses `Focus::default()` — no branch, no plan, no
/// keys — which keeps today's order and only adds the row cap.
#[derive(Debug, Default, Clone)]
pub struct Focus {
    /// the branch the session started on (`row.branch() == focus.branch`
    /// rows are Now)
    pub branch: Option<String>,
    /// the active plan chunk (`plan:<name>:chunk-<n>` rows are Now)
    pub plan: Option<PlanFocus>,
    /// keys of my open rows on this branch (those rows are Now)
    pub keys: HashSet<String>,
}

/// The active plan chunk — found via `Config::plan_dirs` in chunk 3; the
/// shape lives here so `bucket` stays pure.
#[derive(Debug, Clone)]
pub struct PlanFocus {
    pub name: String,
    pub chunk: u32,
    pub path: Option<String>,
}

/// Where one gathered row lands: Now shows first (budget still caps), File
/// fills the row cap, Background never renders — one count line instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bucket {
    Now,
    File,
    Background,
}

/// What happens to Background rows — one variant today (D2), shaped so it
/// can become config later without touching callers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Background {
    CountLine,
    // Hidden, Rows — future config values
}

/// L4 policy: the row cap, then the token budget as the hard cap. `select`
/// enforces `max_rows` (`0` = no row cap, token budget only); `budget`
/// flows through the hook into render, which enforces it as today.
#[derive(Debug, Clone, Copy)]
pub struct PushPolicy {
    pub max_rows: usize,
    pub budget: usize,
    pub background: Background,
}

/// D2 — background rows never render: one count line with the exact next call.
pub const PUSH_BACKGROUND: Background = Background::CountLine;

/// What `select` cut: the rows to render, plus how many never render (the
/// same-dir / shared-key tiers and the tier-0 rows past the cap).
#[derive(Debug)]
pub struct Selection<'a> {
    pub shown: Vec<&'a Row>,
    pub omitted: usize,
}

fn plan_key(p: &PlanFocus) -> String {
    format!("plan:{}:chunk-{}", p.name, p.chunk)
}

/// Bucket one gathered row. `tier` is L1's match (0 exact file/zone, 1
/// same-dir, 2 shared key): an open issue is Now whatever its tier, a
/// tier-0 decision or note is File, the rest is Background. Inside a bucket
/// the existing `cmp_rows` order holds — no second ranking.
pub fn bucket(r: &Row, tier: usize, focus: &Focus) -> Bucket {
    let now_key = r.key.as_deref().is_some_and(|k| {
        focus.keys.contains(k) || focus.plan.as_ref().is_some_and(|p| k == plan_key(p))
    });
    let now_branch = r
        .branch()
        .is_some_and(|b| focus.branch.as_deref() == Some(b));
    if r.kind == "issue" || r.urgent_value().is_some() || now_key || now_branch {
        return Bucket::Now;
    }
    if tier == 0 {
        Bucket::File
    } else {
        Bucket::Background
    }
}

/// Cut tiered L1 rows to what the push renders. The input stays in L1's
/// `cmp_rows` order — Now rows move ahead of File, Background only counts —
/// so with `Focus::default()` this is today's order, capped. `max_rows = 0`
/// skips the row cap (token budget only); Background never shows either way.
pub fn select<'a>(
    rows: Vec<(&'a Row, usize)>,
    focus: &Focus,
    policy: &PushPolicy,
) -> Selection<'a> {
    let mut shown: Vec<&Row> = vec![];
    let mut file: Vec<&Row> = vec![];
    let mut bg = 0usize;
    for (r, t) in rows {
        match (bucket(r, t, focus), policy.background) {
            (Bucket::Now, _) => shown.push(r),
            (Bucket::File, _) => file.push(r),
            (Bucket::Background, Background::CountLine) => bg += 1,
        }
    }
    // Now first, then File — the cap eats the File tail before any Now row.
    shown.extend(file);
    let total = shown.len() + bg;
    if policy.max_rows > 0 && shown.len() > policy.max_rows {
        shown.truncate(policy.max_rows);
    }
    Selection {
        omitted: total - shown.len(),
        shown,
    }
}
