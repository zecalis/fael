//! `fael chunk apply --json -` (SPEC-fael-board §4): the planner's merge / split / pair /
//! after as one batch in one `BEGIN IMMEDIATE` — a bad item fails the batch, nothing half
//! applied. Nothing is deleted: an old chunk becomes `replaced` with `replaced_by` edges,
//! and every op is an `event` row.

use super::State;
use super::apply_graph::{inherit, replace};
use super::chunk::{Fields, Row, add_after, batch, last_event, row, scope, set, tx};
use super::store::{Store, err};
use rusqlite::{Transaction, params};
use serde::Deserialize;

/// One item of the batch, `{"op": "merge" | "split" | "pair" | "after", …}`.
#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "lowercase", deny_unknown_fields)]
pub enum Op {
    /// a new `open` chunk where the first one stood: briefs joined, `after` and `scope`
    /// the union, unless the item says otherwise; the old ones → `replaced`
    Merge {
        chunks: Vec<String>,
        title: Option<String>,
        brief: Option<String>,
        size: Option<String>,
        model: Option<String>,
        client: Option<String>,
        scope: Option<Vec<String>>,
    },
    /// new chunks in its place, each inheriting its `after`; it → `replaced`
    Split { chunk: String, into: Vec<Part> },
    /// `chunk start` of one claims them all (SPEC §4)
    Pair { chunks: Vec<String> },
    /// `chunk` needs `on` first; its state stays (`blocked` is derived)
    After {
        chunk: String,
        on: String,
        why: String,
    },
}

/// One chunk a split makes; no brief = `draft`, as `chunk add`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Part {
    pub title: String,
    pub brief: Option<String>,
    pub size: Option<String>,
    pub model: Option<String>,
    pub client: Option<String>,
    pub scope: Option<Vec<String>>,
}

/// A chunk an op may touch: in a `db` plan, not running, not history.
pub(super) struct Src {
    pub(super) row: Row,
    pub(super) plan: i64,
    pub(super) title: String,
    pub(super) brief: String,
    pub(super) scope: Vec<String>,
}

fn src(tx: &Transaction, uid: &str) -> Result<Src, String> {
    let r = row(tx, uid)?;
    if r.state == State::Running {
        return Err(format!(
            "rejected: chunk {uid} is running — its agent ends it first (done, wait or after)"
        ));
    }
    if r.state.terminal() {
        return Err(format!(
            "rejected: chunk {uid} is {} — history, not planned",
            r.state.as_str()
        ));
    }
    let (plan, title, brief, scope): (i64, String, String, Option<String>) = tx
        .query_row(
            "SELECT plan, title, brief, scope FROM chunk WHERE id = ?1",
            [r.id],
            |q| Ok((q.get(0)?, q.get(1)?, q.get(2)?, q.get(3)?)),
        )
        .map_err(err)?;
    let scope = scope.iter().flat_map(|s| s.lines()).map(String::from);
    Ok(Src {
        row: r,
        plan,
        title,
        brief,
        scope: scope.collect(),
    })
}

/// `uids` resolved, each named once.
fn srcs(tx: &Transaction, uids: &[String], min: usize, what: &str) -> Result<Vec<Src>, String> {
    if uids.len() < min {
        return Err(format!("rejected: {what} takes at least {min} chunks"));
    }
    if (1..uids.len()).any(|i| uids[..i].contains(&uids[i])) {
        return Err(format!("rejected: {what} names a chunk twice"));
    }
    uids.iter().map(|u| src(tx, u)).collect()
}

/// `n` free seqs right after chunk `id` in its plan; with no gap left the plan is
/// renumbered by 1024 first (SPEC §7: seq is order only).
fn slots(tx: &Transaction, plan: i64, id: i64, n: i64) -> Result<Vec<i64>, String> {
    let q = |sql: &str| -> Result<Option<i64>, String> {
        tx.query_row(sql, params![plan, id], |r| r.get(0))
            .map_err(err)
    };
    let next = "SELECT MIN(seq) FROM chunk WHERE plan = ?1
                AND seq > (SELECT seq FROM chunk WHERE id = ?2)";
    let at = "SELECT seq FROM chunk WHERE id = ?2 AND plan = ?1";
    let (mut lo, mut hi) = (q(at)?.unwrap_or_default(), q(next)?);
    if hi.is_some_and(|h| h - lo <= n) {
        let ids: Vec<i64> = tx
            .prepare("SELECT id FROM chunk WHERE plan = ?1 ORDER BY seq")
            .map_err(err)?
            .query_map([plan], |r| r.get(0))
            .map_err(err)?
            .collect::<Result<_, _>>()
            .map_err(err)?;
        // two passes: UNIQUE(plan, seq) holds after every single update
        for sign in [-1, 1] {
            for (k, c) in ids.iter().enumerate() {
                let seq = sign * (k as i64 + 1) * 1024;
                tx.execute("UPDATE chunk SET seq = ?1 WHERE id = ?2", params![seq, c])
                    .map_err(err)?;
            }
        }
        (lo, hi) = (q(at)?.unwrap_or_default(), q(next)?);
    }
    let step = hi.map_or(1024, |h| (h - lo) / (n + 1));
    Ok((1..=n).map(|k| lo + k * step).collect())
}

/// A new chunk at `seq`; no brief = `draft`. Returns `(id, uid)`.
pub(super) fn insert(
    tx: &Transaction,
    plan: i64,
    seq: i64,
    f: &Fields,
    text: &str,
    now: &str,
) -> Result<(i64, String), String> {
    let title = f.title.as_deref().map(str::trim).unwrap_or_default();
    if title.is_empty() {
        return Err("rejected: a chunk needs a title".into());
    }
    let brief = f.brief.as_deref().map(str::trim).filter(|b| !b.is_empty());
    let state = brief.map_or(State::Draft, |_| State::Open).as_str();
    super::chunk::size(f)?;
    let uid = crate::ulid();
    tx.execute(
        "INSERT INTO chunk(uid, plan, seq, title, state, brief, size, model_hint, scope,
                           client_hint, reads)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            uid,
            plan,
            seq,
            title,
            state,
            brief.unwrap_or_default(),
            f.size,
            f.model,
            scope(f.scope.as_deref().unwrap_or_default())?,
            f.client,
            super::section::reads(f.reads.as_deref().unwrap_or_default())?
        ],
    )
    .map_err(err)?;
    let id = tx.last_insert_rowid();
    tx.execute(
        "INSERT INTO event(chunk_uid, at, by, to_state, text, plan)
         VALUES (?1, ?2, 'planner', ?3, ?4, ?5)",
        params![uid, now, state, text, plan],
    )
    .map_err(err)?;
    Ok((id, uid))
}

fn merge(tx: &Transaction, uids: &[String], f: Fields, now: &str) -> Result<String, String> {
    let old = srcs(tx, uids, 2, "merge")?;
    if old.iter().any(|o| o.plan != old[0].plan) {
        return Err("rejected: merge joins chunks of one plan".into());
    }
    let join = |g: &dyn Fn(&Src) -> &str, sep| old.iter().map(g).collect::<Vec<_>>().join(sep);
    let mut scope: Vec<String> = Vec::new();
    for p in old.iter().flat_map(|o| &o.scope) {
        if !scope.contains(p) {
            scope.push(p.clone());
        }
    }
    let f = Fields {
        title: f.title.or_else(|| Some(join(&|o| &o.title, " + "))),
        brief: f.brief.or_else(|| Some(join(&|o| &o.brief, "\n\n"))),
        scope: f.scope.or(Some(scope)),
        ..f
    };
    let seq = slots(tx, old[0].plan, old[0].row.id, 1)?[0];
    let text = format!("merged from {}", uids.join(", "));
    let new = insert(tx, old[0].plan, seq, &f, &text, now)?;
    let ids: Vec<i64> = old.iter().map(|o| o.row.id).collect();
    inherit(tx, &ids, &[new.0])?;
    replace(tx, &old, std::slice::from_ref(&new), now)?;
    Ok(format!("merged {} → {}", uids.join(", "), new.1))
}

fn split(tx: &Transaction, uid: &str, into: Vec<Part>, now: &str) -> Result<String, String> {
    let old = src(tx, uid)?;
    if into.len() < 2 {
        return Err(
            "rejected: split makes at least 2 chunks — `fael chunk edit` changes one".into(),
        );
    }
    let seqs = slots(tx, old.plan, old.row.id, into.len() as i64)?;
    let text = format!("split from {uid}");
    let mut new = Vec::new();
    for (p, seq) in into.into_iter().zip(seqs) {
        let f = Fields {
            title: Some(p.title),
            brief: p.brief,
            size: p.size,
            model: p.model,
            client: p.client,
            scope: p.scope,
            ..Fields::default()
        };
        new.push(insert(tx, old.plan, seq, &f, &text, now)?);
    }
    let ids: Vec<i64> = new.iter().map(|(i, _)| *i).collect();
    let dropped = inherit(tx, &[old.row.id], &ids)?;
    replace(tx, std::slice::from_ref(&old), &new, now)?;
    let names: Vec<&str> = new.iter().map(|(_, u)| u.as_str()).collect();
    let mut line = format!("split {uid} → {}", names.join(", "));
    if dropped > 0 {
        line.push_str(&format!(
            " · its {dropped} pair(s) dropped — pair a part again"
        ));
    }
    Ok(line)
}

fn pair(tx: &Transaction, uids: &[String], now: &str) -> Result<String, String> {
    let all = srcs(tx, uids, 2, "pair")?;
    for (i, a) in all.iter().enumerate() {
        for b in &all[i + 1..] {
            tx.execute(
                "INSERT INTO edge(src, dst, kind) SELECT ?1, ?2, 'pair' WHERE NOT EXISTS(
                   SELECT 1 FROM edge WHERE kind = 'pair'
                   AND ((src = ?1 AND dst = ?2) OR (src = ?2 AND dst = ?1)))",
                params![a.row.id, b.row.id],
            )
            .map_err(err)?;
        }
        let text = format!("paired with {}", uids.join(", "));
        set(tx, &a.row, a.row.state, "planner", Some(&text), now)?;
    }
    Ok(format!("paired {}", uids.join(", ")))
}

fn after(tx: &Transaction, uid: &str, on: &str, why: &str, now: &str) -> Result<String, String> {
    let a = src(tx, uid)?;
    let b = row(tx, on)?;
    if a.row.id == b.id {
        return Err("rejected: a chunk cannot wait on itself".into());
    }
    add_after(tx, a.row.id, b.id)?;
    set(
        tx,
        &a.row,
        a.row.state,
        "planner",
        Some(&format!("after {on}: {why}")),
        now,
    )?;
    Ok(format!("{uid} after {on}"))
}

/// One op in the caller's transaction: the line it reports.
pub(super) fn run(tx: &Transaction, op: Op, now: &str) -> Result<String, String> {
    match op {
        Op::Merge {
            chunks,
            title,
            brief,
            size,
            model,
            client,
            scope,
        } => {
            let f = Fields {
                title,
                brief,
                size,
                model,
                client,
                scope,
                ..Fields::default()
            };
            merge(tx, &chunks, f, now)
        }
        Op::Split { chunk, into } => split(tx, &chunk, into, now),
        Op::Pair { chunks } => pair(tx, &chunks, now),
        Op::After { chunk, on, why } => after(tx, &chunk, &on, &why, now),
    }
}

impl Store {
    /// `fael chunk apply --json -`: every op or none. Returns one line per op.
    pub fn apply(&mut self, ops: Vec<Op>, now: &str) -> Result<Vec<String>, String> {
        if ops.is_empty() {
            return Err("rejected: nothing to apply — stdin held an empty array".into());
        }
        let tx = tx(self)?;
        let since = last_event(&tx)?;
        let mut out = Vec::new();
        for (i, op) in ops.into_iter().enumerate() {
            let line = run(&tx, op, now);
            out.push(line.map_err(|e| format!("op {}: {e} — nothing applied", i + 1))?);
        }
        batch(&tx, since)?;
        tx.commit().map_err(err)?;
        Ok(out)
    }
}

#[cfg(test)]
#[path = "apply_tests.rs"]
mod tests;
