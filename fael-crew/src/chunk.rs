//! The chunk contract (SPEC-fael-board §1): each command is one `BEGIN IMMEDIATE`, writes an
//! `event` row, and a transition the table does not allow is rejected with the command that
//! makes it valid.

use super::lookup::{label, lookup, no_chunk};
use super::store::{Store, err};
use super::{State, check};
use rusqlite::{OptionalExtension, Transaction, TransactionBehavior, params};

/// Where an agent command runs. `wait`, `after` and `done` are accepted only from the
/// worktree of the chunk's latest run, so an agent taken over by `--force` is fenced off —
/// or, with `run`, from the shell that holds that start R (a client that cannot call fael).
pub struct Here<'a> {
    pub worktree: &'a str,
    pub branch: Option<&'a str>,
    pub now: &'a str,
    pub run: Option<&'a str>,
}

/// The fields `add` sets and `edit` changes (None = leave as is).
#[derive(Debug, Default, Clone)]
pub struct Fields {
    pub title: Option<String>,
    pub brief: Option<String>,
    pub size: Option<String>,
    pub model: Option<String>,
    /// the client the planner picked (`chunk.client_hint`); `approve` falls back to it
    pub client: Option<String>,
    /// repo-relative path prefixes (SPEC §2 overlaps), stored one per line
    pub scope: Option<Vec<String>>,
    /// the sections it works from, `<key>` or `<plan>/<key>`: its brief prints those
    pub reads: Option<Vec<String>>,
    /// a short name (`b10`), unique in its plan; any command takes it for the uid
    pub label: Option<String>,
    /// `add` only: open ledger issue ids the chunk fixes, checked by the caller (l2)
    pub issues: Vec<String>,
}

pub(super) struct Row {
    pub(super) id: i64,
    pub(super) uid: String,
    pub(super) state: State,
}

pub(super) fn tx(s: &mut Store) -> Result<Transaction<'_>, String> {
    s.conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(err)
}

/// A chunk, by uid or label.
pub(super) fn row(tx: &Transaction, uid: &str) -> Result<Row, String> {
    let (id, uid, state) = lookup(tx, uid)?.ok_or_else(|| no_chunk(uid))?;
    let state = State::parse(&state).ok_or(format!("crew.db: chunk {uid} has state {state}"))?;
    Ok(Row { id, uid, state })
}

/// Move `r` to `to` (or keep its state when `to == r.state`) and write the event.
pub(super) fn set(
    tx: &Transaction,
    r: &Row,
    to: State,
    by: &str,
    text: Option<&str>,
    now: &str,
) -> Result<(), String> {
    if to != r.state {
        check(r.state, to).map_err(|e| format!("{e} ({})", r.uid))?;
        tx.execute(
            "UPDATE chunk SET state = ?1 WHERE id = ?2",
            params![to.as_str(), r.id],
        )
        .map_err(err)?;
    }
    tx.execute(
        "INSERT INTO event(chunk_uid, at, by, from_state, to_state, text, plan)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, (SELECT plan FROM chunk WHERE id = ?7))",
        params![r.uid, now, by, r.state.as_str(), to.as_str(), text, r.id],
    )
    .map_err(err)?;
    Ok(())
}

/// The newest event id, taken before a `chunk apply` so [`batch`] knows its rows.
pub(super) fn last_event(tx: &Transaction) -> Result<i64, String> {
    tx.query_row("SELECT COALESCE(MAX(id), 0) FROM event", [], |r| r.get(0))
        .map_err(err)
}

/// One ulid on every event after `since`: the rows one apply wrote (it holds the lock).
pub(super) fn batch(tx: &Transaction, since: i64) -> Result<usize, String> {
    tx.execute(
        "UPDATE event SET batch = ?1 WHERE id > ?2",
        params![fael_core::ulid(), since],
    )
    .map_err(err)
}

/// End the chunk's live run: the slot frees even while the shell still runs.
pub(super) fn end_live(tx: &Transaction, chunk: i64, now: &str) -> Result<usize, String> {
    tx.execute(
        "UPDATE run SET ended = ?1 WHERE chunk = ?2 AND ended IS NULL",
        params![now, chunk],
    )
    .map_err(err)
}

/// A running chunk answers only to the worktree of its latest run, or to `--run R` naming it.
pub(super) fn fence(tx: &Transaction, r: &Row, here: &Here) -> Result<(), String> {
    let Some((start, wt, client)): Option<(String, Option<String>, Option<String>)> = tx
        .query_row(
            "SELECT start, worktree, client FROM run WHERE chunk = ?1 ORDER BY id DESC LIMIT 1",
            [r.id],
            |q| Ok((q.get(0)?, q.get(1)?, q.get(2)?)),
        )
        .optional()
        .map_err(err)?
    else {
        return Ok(());
    };
    let who = client.as_deref().unwrap_or("agent");
    match (here.run, wt) {
        (Some(run), _) if run != start => Err(format!(
            "rejected: chunk {}'s latest run is {start} ({who}), not {run} — that start no longer holds it",
            r.uid
        )),
        (Some(_), _) => Ok(()),
        _ if r.state != State::Running => Ok(()),
        (None, Some(w)) if w != here.worktree => Err(format!(
            "rejected: chunk {} was taken over by run {start} ({who}) in {w} — this session no longer holds it; if it is yours (moved worktree), pass --run {start}",
            r.uid
        )),
        _ => Ok(()),
    }
}

/// S, M or L: the app routes a chunk with no hint by its size; `""` clears it on `edit`.
pub(super) fn size(f: &Fields) -> Result<(), String> {
    match f.size.as_deref() {
        None | Some("S" | "M" | "L" | "") => Ok(()),
        Some(v) => Err(format!("rejected: --size {v} — S, M or L")),
    }
}

/// `a/b.rs`, `docs/` — no root, no `..`, no globs (SPEC §2): two scopes overlap by prefix.
pub(super) fn scope(paths: &[String]) -> Result<Option<String>, String> {
    for p in paths {
        let bad = p.is_empty()
            || p.starts_with('/')
            || p.contains(['\\', '*', '?', '[', ']', '\n'])
            || p.trim_end_matches('/')
                .split('/')
                .any(|s| s.is_empty() || s == "." || s == "..");
        if bad {
            return Err(format!(
                "rejected: scope {p:?} — a repo-relative path prefix (`src/a.rs`, `docs/`), no `..`, no globs"
            ));
        }
    }
    Ok((!paths.is_empty()).then(|| paths.join("\n")))
}

/// The chunk an `after` names: a label in plan `pid` first (a label is unique in its plan,
/// not across plans), else what `lookup` finds.
pub(super) fn uid_id(tx: &Transaction, pid: i64, uid: &str) -> Result<i64, String> {
    let here: Option<i64> = tx
        .query_row(
            "SELECT id FROM chunk WHERE plan = ?1 AND label = ?2",
            params![pid, uid],
            |r| r.get(0),
        )
        .optional()
        .map_err(err)?;
    if let Some(id) = here {
        return Ok(id);
    }
    Ok(lookup(tx, uid)?
        .ok_or(format!(
            "rejected: no chunk {uid} — an `after` names a chunk that exists"
        ))?
        .0)
}

/// The `after` edge `src → dst`, unless `dst` already waits on `src` (a cycle never opens).
pub(super) fn add_after(tx: &Transaction, src: i64, dst: i64) -> Result<(), String> {
    let cycle: bool = tx
        .query_row(
            "WITH RECURSIVE w(id) AS (SELECT ?1 UNION SELECT e.dst FROM edge e JOIN w ON e.src = w.id
               WHERE e.kind = 'after' AND e.dst IS NOT NULL)
             SELECT EXISTS(SELECT 1 FROM w WHERE id = ?2)",
            params![dst, src],
            |r| r.get(0),
        )
        .map_err(err)?;
    if cycle {
        return Err(
            "rejected: that `after` closes a loop — the other chunk already waits on this one"
                .into(),
        );
    }
    tx.execute(
        "INSERT INTO edge(src, dst, kind) SELECT ?1, ?2, 'after'
         WHERE NOT EXISTS(SELECT 1 FROM edge WHERE src = ?1 AND dst = ?2 AND kind = 'after')",
        params![src, dst],
    )
    .map_err(err)?;
    Ok(())
}

/// `fael chunk edit`: fields, and the state only to open a draft that gains a brief
/// (issue 01M4KJQE: an inbox draft has a way out). Any change clears the approval
/// (SPEC §8), so the queue never starts work the owner did not approve as it now reads —
/// but a `--scope` alone that keeps every old path covered: the owner approved the work and
/// its model, and a wider scope only lets the overlap check see more (owner, 2026-10-15).
pub(super) fn edit_in(
    tx: &Transaction,
    uid: &str,
    f: &Fields,
    now: &str,
) -> Result<String, String> {
    let r = row(tx, uid)?;
    if r.state.terminal() {
        return Err(format!(
            "rejected: chunk {uid} is {} — history, not edited",
            r.state.as_str()
        ));
    }
    let scope = f.scope.as_deref().map(scope).transpose()?;
    let reads = f.reads.as_deref().map(super::section::reads).transpose()?;
    size(f)?;
    if let Some(l) = f.label.as_deref().filter(|l| !l.is_empty()) {
        let plan: i64 = tx
            .query_row("SELECT plan FROM chunk WHERE id = ?1", [r.id], |q| q.get(0))
            .map_err(err)?;
        label(tx, plan, r.id, l)?;
    }
    let old: Option<String> = tx
        .query_row("SELECT scope FROM chunk WHERE id = ?1", [r.id], |q| {
            q.get(0)
        })
        .map_err(err)?;
    let mut what = Vec::new();
    for (col, v) in [
        ("title", f.title.as_deref().map(str::trim)),
        ("brief", f.brief.as_deref().map(str::trim)),
        ("size", f.size.as_deref()),
        ("model_hint", f.model.as_deref()),
        ("client_hint", f.client.as_deref()),
        ("scope", scope.as_ref().map(|s| s.as_deref().unwrap_or(""))),
        ("reads", reads.as_ref().map(|s| s.as_deref().unwrap_or(""))),
        ("label", f.label.as_deref()),
    ] {
        let Some(v) = v else { continue };
        if col == "title" && v.is_empty() {
            return Err("rejected: a chunk needs a title".into());
        }
        let v = (!v.is_empty() || matches!(col, "brief" | "title")).then_some(v);
        tx.execute(
            &format!("UPDATE chunk SET {col} = ?1 WHERE id = ?2"),
            params![v, r.id],
        )
        .map_err(err)?;
        what.push(col);
    }
    if what.is_empty() {
        return Err(
            "rejected: nothing to edit — --title, --brief, --size, --model, --client, --scope, --reads or --label"
                .into(),
        );
    }
    let new = scope.flatten().unwrap_or_default();
    let widened = what == ["scope"]
        && old
            .as_deref()
            .unwrap_or("")
            .lines()
            .all(|p| new.lines().any(|q| super::board_math::under(p, q)));
    let n = if widened {
        0
    } else {
        tx.execute(
            "UPDATE chunk SET approved_at = NULL, approved_model = NULL, approved_client = NULL
             WHERE id = ?1 AND approved_at IS NOT NULL",
            [r.id],
        )
        .map_err(err)?
    };
    let mut text = format!("edited {}", what.join(", "));
    if n > 0 {
        text.push_str(" · approval cleared");
    }
    let briefed = f.brief.as_deref().is_some_and(|b| !b.trim().is_empty());
    let to = if r.state == State::Draft && briefed {
        State::Open
    } else {
        r.state
    };
    set(tx, &r, to, "owner", Some(&text), now)?;
    Ok(text)
}

impl Store {
    /// `fael chunk edit`: see [`edit_in`].
    pub fn edit(&mut self, uid: &str, f: &Fields, now: &str) -> Result<(), String> {
        let tx = tx(self)?;
        edit_in(&tx, uid, f, now)?;
        tx.commit().map_err(err)
    }

    /// `fael chunk unafter`: `uid` stops waiting on `other` — the planner's undo of an
    /// `after` without a bill (vela, 2026-10-15). The approval stays, as a bill's after
    /// change keeps it: the work the owner approved did not change.
    pub fn unafter(&mut self, uid: &str, other: &str, why: &str, now: &str) -> Result<(), String> {
        let tx = tx(self)?;
        let r = row(&tx, uid)?;
        if r.state.terminal() {
            return Err(format!(
                "rejected: chunk {uid} is {} — it waits on nothing",
                r.state.as_str()
            ));
        }
        let pid: i64 = tx
            .query_row("SELECT plan FROM chunk WHERE id = ?1", [r.id], |q| q.get(0))
            .map_err(err)?;
        let dst = uid_id(&tx, pid, other)?;
        let n = tx
            .execute(
                "DELETE FROM edge WHERE src = ?1 AND dst = ?2 AND kind = 'after'",
                params![r.id, dst],
            )
            .map_err(err)?;
        if n == 0 {
            return Err(format!("rejected: chunk {uid} does not wait on {other}"));
        }
        let text = format!("no longer after {other}: {why}");
        set(&tx, &r, r.state, "owner", Some(&text), now)?;
        tx.commit().map_err(err)
    }
}

pub(super) fn is_date(d: &str) -> bool {
    let b = d.as_bytes();
    b.len() == 10
        && b.iter().enumerate().all(|(i, c)| {
            if i == 4 || i == 7 {
                *c == b'-'
            } else {
                c.is_ascii_digit()
            }
        })
}

#[cfg(test)]
#[path = "chunk_tests.rs"]
pub(super) mod tests;

#[cfg(test)]
#[path = "chunk_edit_tests.rs"]
mod edit_tests;
