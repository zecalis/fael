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

/// What `select` did, split into classes that each have one exact next call:
/// `omitted` rows (the row cap and token budget cut them — `fael find --files
/// <f>` returns them), `background_dirs` hidden same-dir rows (`fael find
/// --files <dir>/`), and `background_keys` hidden shared-key rows, key ->
/// count (`fael find --key <k>`).
#[derive(Debug)]
pub struct Selection<'a> {
    pub shown: Vec<&'a Row>,
    pub omitted: usize,
    pub background_dirs: usize,
    pub background_keys: Vec<(String, usize)>,
}

impl Selection<'_> {
    /// How many rows `fael find --files <f>` returns beyond what rendered: the
    /// row cap cut plus the rows the token budget cut. `rendered` is the row
    /// count render actually printed.
    pub fn findable_after(&self, rendered: usize) -> usize {
        self.omitted + self.shown.len().saturating_sub(rendered)
    }
}

fn plan_key(p: &PlanFocus) -> String {
    format!("plan:{}:chunk-{}", p.name, p.chunk)
}

/// Count one key in encounter order — deterministic: same log, same order.
fn bump_key(keys: &mut Vec<(String, usize)>, key: String) {
    match keys.iter().position(|(k, _)| *k == key) {
        Some(i) => keys[i].1 += 1,
        None => keys.push((key, 1)),
    }
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

/// Cut tiered L1 rows to what the push renders. `max_rows = 0` is no cap —
/// nothing is suppressed and L1's own order stands (today's push, token budget
/// only). Otherwise Now rows move ahead of File and always render (only the
/// token budget caps them, at render); the row cap eats the File tail after
/// them. Background rows never render — each class is counted by the exact
/// call that reaches it: same-dir rows by the query's directory, shared-key
/// rows by their key.
pub fn select<'a>(
    rows: Vec<(&'a Row, usize)>,
    focus: &Focus,
    policy: &PushPolicy,
) -> Selection<'a> {
    if policy.max_rows == 0 {
        return Selection {
            shown: rows.into_iter().map(|(r, _)| r).collect(),
            omitted: 0,
            background_dirs: 0,
            background_keys: Vec::new(),
        };
    }
    let mut now: Vec<&Row> = vec![];
    let mut file: Vec<&Row> = vec![];
    let mut background_dirs = 0usize;
    let mut background_keys: Vec<(String, usize)> = vec![];
    for (r, t) in rows {
        match bucket(r, t, focus) {
            Bucket::Now => now.push(r),
            Bucket::File => file.push(r),
            Bucket::Background => match policy.background {
                // tier 2 is a shared key; tier 1 (or a keyless row) a neighbour
                Background::CountLine => match t {
                    2 => match r.key.clone() {
                        Some(k) => bump_key(&mut background_keys, k),
                        None => background_dirs += 1,
                    },
                    _ => background_dirs += 1,
                },
            },
        }
    }
    // Now rows always show; the cap only limits how much of File joins them.
    let mut shown = now;
    let room = policy.max_rows.saturating_sub(shown.len());
    let omitted = file.len().saturating_sub(room);
    shown.extend(file.into_iter().take(room));
    Selection {
        shown,
        omitted,
        background_dirs,
        background_keys,
    }
}
