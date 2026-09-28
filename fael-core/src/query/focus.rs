//! L2 build + L3 rank + L4 select for the read/edit push (PLAN-fael-push-focus):
//! `open_plans` reads the open `plan:<name>:chunk-<n>` rows into L1 facts,
//! `resolve_plan` turns facts + the session branch + a declared intent into the
//! active plan (PLAN-fael-plan-focus), `Focus::from_rows` turns the start branch
//! and its open rows into what the push ranks against, `bucket` lands each row
//! in Now | File | Background, `select` cuts to the row cap. Pure — no git
//! spawn, no file reads: the hook builds the Focus at session start
//! (`hook/focus.rs`) and the push only reads it back.

use crate::Row;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashSet};

/// What the session works on: the start branch, the resolved active plan, and
/// the keys of the open rows filed on that branch. Built at session start
/// (git is allowed there) and read back from the session state by the push;
/// with no session state `Focus::default()` — no branch, no plan, no keys —
/// keeps today's order and only adds the row cap.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Focus {
    /// the branch the session started on (`row.branch() == focus.branch`
    /// rows are Now)
    pub branch: Option<String>,
    /// the active plan resolution (PLAN-fael-plan-focus); only
    /// `Active { chunk: Some(n) }` puts the `plan:<name>:chunk-<n>` row in Now
    pub plan: PlanResolution,
    /// keys of my open rows on this branch (those rows are Now)
    pub keys: HashSet<String>,
}

/// L1 fact: one plan still carrying an open `plan:<name>:chunk-<n>` row.
/// `chunks` holds every open chunk number, `branches` every branch that filed
/// one. Built pure from the open rows — the intent (which plan this branch
/// means) is not a fact and never lands here (PLAN-fael-plan-focus invariant 4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanFact {
    pub name: String,
    pub chunks: BTreeSet<u32>,
    pub branches: BTreeSet<String>,
}

/// Where an `Active` plan came from — declared intent outranks inference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlanSource {
    Declared,
    Branch,
    Only,
}

/// One plan the log cannot choose between — name plus its highest open chunk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanCandidate {
    pub name: String,
    pub chunk: u32,
}

/// L3 output: which plan (if any) this session is inside. `Ambiguous` is a real
/// answer — the log holds more than one open plan and no intent breaks the tie,
/// so no plan enters Now (invariant 3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum PlanResolution {
    Active {
        name: String,
        chunk: Option<u32>,
        source: PlanSource,
    },
    Ambiguous {
        candidates: Vec<PlanCandidate>,
    },
    #[default]
    None,
}

impl Focus {
    /// L2 build, pure: the start branch plus the keys of the open rows filed
    /// on it — `rows` arrives already open (closed and superseded filtered by
    /// the caller, as `find` does), so this only reads `branch` and `key`. The
    /// active plan is not inferred here: callers set `plan` with `resolve_plan`
    /// (there is no intent without a branch). No branch (detached HEAD, no
    /// session) = `Focus::default()` — today's order, only the row cap applies.
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
            plan: PlanResolution::None,
            keys,
        }
    }
}

/// `plan:<name>:chunk-<n>` → name and chunk. The key shape is the
/// `plan:<name>` anchor plus the chunk (docs/format.md): a plan name is one
/// segment, the chunk the last one.
fn plan_chunk(key: &str) -> Option<(&str, u32)> {
    let (name, chunk) = key.strip_prefix("plan:")?.rsplit_once(":chunk-")?;
    if name.is_empty() {
        return None;
    }
    Some((name, chunk.parse().ok()?))
}

/// L1, pure: the plans with at least one open `plan:<name>:chunk-<n>` row.
/// `rows` arrives already open (closed and superseded filtered by the caller,
/// as `find` does). A plan is one name; its open chunks and the branches that
/// filed them accumulate. Sorted by name, so the result is deterministic.
pub fn open_plans(rows: &[&Row]) -> Vec<PlanFact> {
    let mut by_name: BTreeMap<String, PlanFact> = BTreeMap::new();
    for r in rows {
        let Some((name, chunk)) = r.key.as_deref().and_then(plan_chunk) else {
            continue;
        };
        let fact = by_name.entry(name.to_string()).or_insert_with(|| PlanFact {
            name: name.to_string(),
            chunks: BTreeSet::new(),
            branches: BTreeSet::new(),
        });
        fact.chunks.insert(chunk);
        if let Some(b) = r.branch() {
            fact.branches.insert(b.to_string());
        }
    }
    by_name.into_values().collect()
}

/// The highest open chunk of a plan — never the newest row by id: a row filed
/// later to fix an earlier chunk's data must not move the pointer backwards.
fn highest(fact: &PlanFact) -> Option<u32> {
    fact.chunks.iter().next_back().copied()
}

/// Sorted candidates for an `Ambiguous` answer — deterministic, never "newest".
fn candidates(facts: &[&PlanFact]) -> Vec<PlanCandidate> {
    let mut out: Vec<PlanCandidate> = facts
        .iter()
        .map(|f| PlanCandidate {
            name: f.name.clone(),
            chunk: highest(f).unwrap_or(0),
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// L3, pure — no I/O, no mutation, no guess (PLAN-fael-plan-focus invariant 5).
/// Order:
/// 1. **declared** — `intent` names a plan: `Active` whatever the facts say,
///    `chunk = None` when the plan has no open chunk;
/// 2. **branch** — exactly one plan has an open row filed on this branch:
///    `Active`; more than one: `Ambiguous`;
/// 3. **only** — the whole log holds exactly one open plan: `Active`;
/// 4. more than one: `Ambiguous`; none: `None`.
///
/// No branch (detached HEAD, no session) = `None`.
pub fn resolve_plan(
    facts: &[PlanFact],
    branch: Option<&str>,
    intent: Option<&str>,
) -> PlanResolution {
    let Some(branch) = branch else {
        return PlanResolution::None;
    };
    if let Some(name) = intent {
        let chunk = facts.iter().find(|f| f.name == name).and_then(highest);
        return PlanResolution::Active {
            name: name.to_string(),
            chunk,
            source: PlanSource::Declared,
        };
    };
    let on_branch: Vec<&PlanFact> = facts
        .iter()
        .filter(|f| f.branches.contains(branch))
        .collect();
    match on_branch.len() {
        1 => {
            let f = on_branch[0];
            PlanResolution::Active {
                name: f.name.clone(),
                chunk: highest(f),
                source: PlanSource::Branch,
            }
        }
        0 => match facts.len() {
            0 => PlanResolution::None,
            1 => PlanResolution::Active {
                name: facts[0].name.clone(),
                chunk: highest(&facts[0]),
                source: PlanSource::Only,
            },
            _ => PlanResolution::Ambiguous {
                candidates: candidates(&facts.iter().collect::<Vec<_>>()),
            },
        },
        _ => PlanResolution::Ambiguous {
            candidates: candidates(&on_branch),
        },
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

fn plan_key(name: &str, chunk: u32) -> String {
    format!("plan:{name}:chunk-{chunk}")
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
        focus.keys.contains(k)
            || matches!(
                &focus.plan,
                PlanResolution::Active { name, chunk: Some(n), .. } if k == plan_key(name, *n)
            )
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
