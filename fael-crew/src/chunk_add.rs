//! `fael chunk add` (SPEC-fael-board §1): a new chunk at the end of a plan, in the caller's
//! transaction (`add_in`).

use super::State;
use super::chunk::{Fields, add_after, scope, size, tx, uid_id};
use super::lookup::label;
use super::store::{Store, err};
use rusqlite::{OptionalExtension, Transaction, params};

/// A new chunk at the end of `plan` (`name` or `app/name`); `inbox` is created as a `db`
/// plan on first use. No brief = `draft`. `by` writes the event. Returns `(id, uid)`.
pub(super) fn add_in(
    tx: &Transaction,
    plan: &str,
    f: &Fields,
    after: &[String],
    by: &str,
    now: &str,
) -> Result<(i64, String), String> {
    let title = f.title.as_deref().map(str::trim).unwrap_or_default();
    if title.is_empty() {
        return Err("rejected: a chunk needs a title".into());
    }
    let scope = scope(f.scope.as_deref().unwrap_or_default())?;
    let reads = super::section::reads(f.reads.as_deref().unwrap_or_default())?;
    size(f)?;
    if plan == "inbox" {
        tx.execute(
            "INSERT OR IGNORE INTO plan(app, name, title, state)
             VALUES ('', 'inbox', 'inbox', 'active')",
            [],
        )
        .map_err(err)?;
        // a done/parked inbox hides every chunk from the board; heal it
        tx.execute(
            "UPDATE plan SET state = 'active'
             WHERE app = '' AND name = 'inbox' AND state IN ('done', 'parked')",
            [],
        )
        .map_err(err)?;
    }
    let pid: i64 = tx
        .query_row(
            "SELECT id FROM plan
             WHERE name = ?1 OR (app <> '' AND app || '/' || name = ?1)
             ORDER BY app = '' DESC LIMIT 1",
            [plan],
            |r| r.get(0),
        )
        .optional()
        .map_err(err)?
        .ok_or(format!(
            "rejected: no plan '{plan}' — `fael chunk add --plan inbox` takes new work"
        ))?;
    let brief = f.brief.as_deref().map(str::trim).filter(|b| !b.is_empty());
    let state = if brief.is_some() {
        State::Open
    } else {
        State::Draft
    };
    if let Some(l) = &f.label {
        label(tx, pid, 0, l)?;
    }
    let uid = fael_core::ulid();
    tx.execute(
        "INSERT INTO chunk(uid, plan, seq, title, state, brief, size, model_hint, scope,
                           label, reads, client_hint)
         VALUES (?1, ?2, (SELECT COALESCE(MAX(seq), 0) + 1024 FROM chunk WHERE plan = ?2),
                 ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            uid,
            pid,
            title,
            state.as_str(),
            brief.unwrap_or_default(),
            f.size,
            f.model,
            scope,
            f.label,
            reads,
            f.client
        ],
    )
    .map_err(err)?;
    let id = tx.last_insert_rowid();
    for a in after {
        let dst = uid_id(tx, pid, a)?;
        add_after(tx, id, dst)?;
    }
    super::issue::link(tx, id, &f.issues)?;
    tx.execute(
        "INSERT INTO event(chunk_uid, at, by, to_state, plan) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![uid, now, by, state.as_str(), pid],
    )
    .map_err(err)?;
    Ok((id, uid))
}

impl Store {
    /// `fael chunk add`: see [`add_in`]; the planner writes it.
    pub fn add(
        &mut self,
        plan: &str,
        f: &Fields,
        after: &[String],
        now: &str,
    ) -> Result<String, String> {
        let tx = tx(self)?;
        let (_, uid) = add_in(&tx, plan, f, after, "planner", now)?;
        tx.commit().map_err(err)?;
        Ok(uid)
    }
}
