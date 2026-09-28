//! L2 build + L3 rank + L4 select for the read/edit push (PLAN-fael-push-focus):
//! `Focus::from_rows` turns the session's start branch and its open rows into
//! what the push ranks against, `bucket` lands each row in Now | File |
//! Background, `select` cuts to the row cap. Pure — no git spawn, no file
//! reads: the hook builds the Focus at session start (`hook/focus.rs`) and
//! the push only reads it back.

use crate::Row;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// What the session works on: the start branch and the keys of the open rows
/// filed on it. Built at session start (git is allowed there) and read back
/// from the session state by the push; with no session state
/// `Focus::default()` — no branch, no keys — keeps today's order and only
/// adds the row cap. fael never infers which plan a session is in: `plan:*`
/// keys are a fapony convention, and fapony's kickoff asks for them itself
/// (PLAN-fael-plan-focus). A focus file an older fael wrote with a `plan`
/// field still reads — serde skips the field.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Focus {
    /// the branch the session started on (`row.branch() == focus.branch`
    /// rows are Now)
    pub branch: Option<String>,
    /// keys of my open rows on this branch (those rows are Now)
    pub keys: HashSet<String>,
}

impl Focus {
    /// L2 build, pure: the start branch plus the keys of the open rows filed
    /// on it — `rows` arrives already open (closed and superseded filtered by
    /// the caller, as `find` does), so this only reads `branch` and `key`.
    /// No branch (detached HEAD, no session) = `Focus::default()` — today's
    /// order, only the row cap applies.
    pub fn from_rows(branch: Option<&str>, rows: &[&Row]) -> Focus {
        let Some(branch) = branch else {
            return Focus::default();
        };
        let keys = rows
            .iter()
            .filter(|r| r.branch() == Some(branch))
            .filter_map(|r| r.key.clone())
            .collect();
        Focus {
            branch: Some(branch.to_string()),
            keys,
        }
    }
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
    let now_key = r.key.as_deref().is_some_and(|k| focus.keys.contains(k));
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
