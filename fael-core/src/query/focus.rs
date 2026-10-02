//! L2 build + L3 rank + L4 select for the read/edit push (PLAN-fael-push-focus):
//! `Focus::from_rows` turns the session's start branch and its open rows into
//! what the push ranks against, `bucket` lands each row in Now | File |
//! Background, `select` cuts to the row cap. Pure — no git spawn, no file
//! reads: the hook builds the Focus at session start (`hook/focus.rs`) and
//! the push only reads it back.

use super::select::plan_anchor;
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

/// PLAN-fael-visible-secretary chunk 3: the open issues on this session's
/// work, which session start lists in full — filed on the start branch,
/// keyed by a Focus key, or naming an anchor (`plan:<name>`, or the
/// `PLAN-<name>.md` that names it) that a row filed on the start branch
/// names too. fael infers no plan (#66): the anchors come only from what the
/// branch's own rows say. `rows` arrives open, as for `from_rows`; no branch
/// = none.
pub fn on_work<'a>(focus: &Focus, rows: &[&'a Row], prefixes: &[String]) -> Vec<&'a Row> {
    let Some(branch) = focus.branch.as_deref() else {
        return vec![];
    };
    let anchors = |r: &Row| -> Vec<String> {
        r.files
            .iter()
            .filter_map(|f| match crate::anchor(f) {
                Some(_) => Some(f.clone()),
                None => plan_anchor(f, prefixes),
            })
            .collect()
    };
    let named: HashSet<String> = rows
        .iter()
        .filter(|r| r.branch() == Some(branch))
        .flat_map(|r| anchors(r))
        .collect();
    rows.iter()
        .copied()
        .filter(|r| r.kind == "issue")
        .filter(|r| {
            r.branch() == Some(branch)
                || r.key.as_deref().is_some_and(|k| focus.keys.contains(k))
                || anchors(r).iter().any(|a| named.contains(a))
        })
        .collect()
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

/// A hub file (a spec, a plan, PRODUCT.md) is cited by more open rows than
/// the cap holds, and off the Focus those File rows rank by freshness only —
/// any `push_rows` of them is a guess (issue push:hub-files: 72% of vela's
/// read-push rows touched such a file). Past this many File rows none render:
/// Now rows still do, the rest is the `fael find --files` count line.
/// `push_rows` itself stays (decision push:rows). Counted per push, so a
/// Grep hit list whose files add up past it is cut the same way.
pub const PUSH_HUB_ROWS: usize = 8;

/// What `select` did, split into classes that each have one exact next call:
/// `omitted` rows (the row cap cut tier-0 rows — `fael find --files <f>`
/// returns them) and `background_dirs` same-dir rows hidden by the policy
/// (`fael find --files <dir>/`). A shared-key row is never Background: it
/// rides only on a Focus key, which makes it Now. The token budget cuts later, at render, so
/// the tier of every shown row travels along (`tiers`) — see `hidden`.
#[derive(Debug)]
pub struct Selection<'a> {
    pub shown: Vec<&'a Row>,
    /// L1 tier of each `shown` row, parallel — the budget cut needs it: a
    /// same-dir or shared-key row the token budget cut is not reachable by
    /// `fael find --files <f>`.
    tiers: Vec<usize>,
    pub omitted: usize,
    pub background_dirs: usize,
}

/// The rows that did not render, split by the exact `fael find` call that
/// reaches each.
#[derive(Debug, Default)]
pub struct Hidden {
    pub file: usize,
    pub dirs: usize,
    pub keys: Vec<(String, usize)>,
}

impl Selection<'_> {
    /// The rows hidden after render printed `rendered` of `shown`: the cap cut
    /// (`omitted`) plus the token-budget cut (`shown[rendered..]`), each routed
    /// by its L1 tier — tier 0 to `fael find --files <f>`, tier 1 to
    /// `fael find --files <dir>/`, tier 2 to `fael find --key <k>`. A
    /// budget-cut Now row (an issue on a same-dir file, a shared-key row) is
    /// not reachable by `--files <f>`, which is why the tier travels with it.
    pub fn hidden(&self, rendered: usize) -> Hidden {
        let mut h = Hidden {
            file: self.omitted,
            dirs: self.background_dirs,
            keys: vec![],
        };
        for (r, t) in self.shown.iter().zip(&self.tiers).skip(rendered) {
            match t {
                2 => match r.key.clone() {
                    Some(k) => bump_key(&mut h.keys, k),
                    None => h.dirs += 1,
                },
                1 => h.dirs += 1,
                _ => h.file += 1,
            }
        }
        h
    }

    /// How many rows `fael find --files <f>` reaches beyond what rendered —
    /// the tier-0 budget cut plus the cap cut. `rendered` is the row count
    /// render actually printed.
    pub fn findable_after(&self, rendered: usize) -> usize {
        self.hidden(rendered).file
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
/// same-dir, 2 shared key): an open issue on the file itself is Now, a
/// tier-0 decision or note is File, the rest is Background — an issue on a
/// sibling file is counted like any sibling row, never pushed (a vela edit of
/// upload.ts pushed an OCR outage issue that names no such file). Urgent, a
/// Focus key or the session branch is Now at any tier. Inside a bucket the
/// existing `cmp_rows` order holds — no second ranking.
pub fn bucket(r: &Row, tier: usize, focus: &Focus) -> Bucket {
    let now_key = r.key.as_deref().is_some_and(|k| focus.keys.contains(k));
    let now_branch = r
        .branch()
        .is_some_and(|b| focus.branch.as_deref() == Some(b));
    if (r.kind == "issue" && tier == 0) || r.urgent_value().is_some() || now_key || now_branch {
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
/// call that reaches it: same-dir rows by the query's directory. A shared-key
/// row off the Focus keys is dropped first, never counted.
pub fn select<'a>(
    rows: Vec<(&'a Row, usize)>,
    focus: &Focus,
    policy: &PushPolicy,
) -> Selection<'a> {
    // a shared-key sibling (tier 2) rides only on a key this session works on
    // (decision push:shared-key-siblings): a broad key spanning plans would
    // otherwise drag another plan's rows into the push
    let rows = rows
        .into_iter()
        .filter(|(r, t)| *t != 2 || r.key.as_deref().is_some_and(|k| focus.keys.contains(k)));
    if policy.max_rows == 0 {
        let (shown, tiers) = rows.into_iter().unzip();
        return Selection {
            shown,
            tiers,
            omitted: 0,
            background_dirs: 0,
        };
    }
    let mut now: Vec<(&Row, usize)> = vec![];
    let mut file: Vec<(&Row, usize)> = vec![];
    let mut background_dirs = 0usize;
    for (r, t) in rows {
        match bucket(r, t, focus) {
            Bucket::Now => now.push((r, t)),
            Bucket::File => file.push((r, t)),
            Bucket::Background => match policy.background {
                Background::CountLine => background_dirs += 1,
            },
        }
    }
    // Now rows always show; the cap only limits how much of File joins them,
    // and a hub's File rows join not at all (PUSH_HUB_ROWS)
    let room = if file.len() > PUSH_HUB_ROWS {
        0
    } else {
        policy.max_rows.saturating_sub(now.len())
    };
    let omitted = file.len().saturating_sub(room);
    now.extend(file.into_iter().take(room));
    let (shown, tiers) = now.into_iter().unzip();
    Selection {
        shown,
        tiers,
        omitted,
        background_dirs,
    }
}
