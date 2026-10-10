//! The chunk contract (SPEC-fael-board §1): each command is one `BEGIN IMMEDIATE`, writes an
//! `event` row, and a transition the table does not allow is rejected with the command that
//! makes it valid. Only `truth = db` plans: an md plan's chunks follow its file until cutover.

use super::store::{Store, err};
use super::{State, check};
use rusqlite::{OptionalExtension, Transaction, TransactionBehavior, params};

/// Where an agent command runs. `wait`, `after` and `done` are accepted only from the
/// worktree of the chunk's latest run, so an agent taken over by `--force` is fenced off.
pub struct Here<'a> {
    pub worktree: &'a str,
    pub branch: Option<&'a str>,
    pub now: &'a str,
}

/// The fields `add` sets and `edit` changes (None = leave as is).
#[derive(Debug, Default, Clone)]
pub struct Fields {
    pub title: Option<String>,
    pub brief: Option<String>,
    pub size: Option<String>,
    pub model: Option<String>,
    /// repo-relative path prefixes (SPEC §2 overlaps), stored one per line
    pub scope: Option<Vec<String>>,
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

/// A chunk of a `db` plan, by uid.
pub(super) fn row(tx: &Transaction, uid: &str) -> Result<Row, String> {
    let (id, state, truth, plan, source): (i64, String, String, String, String) = tx
        .query_row(
            "SELECT c.id, c.state, p.truth, p.name, p.source
             FROM chunk c JOIN plan p ON p.id = c.plan WHERE c.uid = ?1",
            [uid],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .optional()
        .map_err(err)?
        .ok_or(format!(
            "rejected: no chunk {uid} — `fael plan export` lists every uid"
        ))?;
    if truth != "db" {
        return Err(format!(
            "rejected: chunk {uid} is in md plan {plan}: its state follows {source} until the plan's cutover (SPEC §9)"
        ));
    }
    let state = State::parse(&state).ok_or(format!("plans.db: chunk {uid} has state {state}"))?;
    Ok(Row {
        id,
        uid: uid.to_string(),
        state,
    })
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
        "INSERT INTO event(chunk_uid, at, by, from_state, to_state, text)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![r.uid, now, by, r.state.as_str(), to.as_str(), text],
    )
    .map_err(err)?;
    Ok(())
}

/// End the chunk's live run: the slot frees even while the shell still runs.
pub(super) fn end_live(tx: &Transaction, chunk: i64, now: &str) -> Result<usize, String> {
    tx.execute(
        "UPDATE run SET ended = ?1 WHERE chunk = ?2 AND ended IS NULL",
        params![now, chunk],
    )
    .map_err(err)
}

/// A running chunk answers only to the worktree of its latest run.
pub(super) fn fence(tx: &Transaction, r: &Row, here: &Here) -> Result<(), String> {
    if r.state != State::Running {
        return Ok(());
    }
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
    match wt {
        Some(w) if w != here.worktree => Err(format!(
            "rejected: chunk {} was taken over by run {start} ({}) in {w} — this session no longer holds it",
            r.uid,
            client.as_deref().unwrap_or("agent")
        )),
        _ => Ok(()),
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

pub(super) fn uid_id(tx: &Transaction, uid: &str) -> Result<i64, String> {
    tx.query_row("SELECT id FROM chunk WHERE uid = ?1", [uid], |r| r.get(0))
        .optional()
        .map_err(err)?
        .ok_or(format!(
            "rejected: no chunk {uid} — an `after` names a chunk that exists"
        ))
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

impl Store {
    /// `fael chunk add`: a new chunk at the end of `plan` (`name` or `app/name`); `inbox` is
    /// created as a `db` plan on first use. No brief = `draft`.
    pub fn add(
        &mut self,
        plan: &str,
        f: &Fields,
        after: &[String],
        now: &str,
    ) -> Result<String, String> {
        let title = f.title.as_deref().map(str::trim).unwrap_or_default();
        if title.is_empty() {
            return Err("rejected: a chunk needs a title".into());
        }
        let scope = scope(f.scope.as_deref().unwrap_or_default())?;
        let tx = tx(self)?;
        if plan == "inbox" {
            tx.execute(
                "INSERT OR IGNORE INTO plan(app, name, title, state, source, truth)
                 VALUES ('', 'inbox', 'inbox', 'active', '', 'db')",
                [],
            )
            .map_err(err)?;
        }
        let (pid, truth, source): (i64, String, String) = tx
            .query_row(
                "SELECT id, truth, source FROM plan
                 WHERE name = ?1 OR (app <> '' AND app || '/' || name = ?1)
                 ORDER BY app = '' DESC LIMIT 1",
                [plan],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()
            .map_err(err)?
            .ok_or(format!(
                "rejected: no plan '{plan}' — `fael chunk add --plan inbox` takes new work"
            ))?;
        if truth != "db" {
            return Err(format!(
                "rejected: plan '{plan}' is md: its chunks are lines in {source} until its cutover (SPEC §9)"
            ));
        }
        let brief = f.brief.as_deref().map(str::trim).filter(|b| !b.is_empty());
        let state = if brief.is_some() {
            State::Open
        } else {
            State::Draft
        };
        let uid = crate::ulid();
        tx.execute(
            "INSERT INTO chunk(uid, plan, seq, title, state, brief, size, model_hint, scope)
             VALUES (?1, ?2, (SELECT COALESCE(MAX(seq), 0) + 1024 FROM chunk WHERE plan = ?2),
                     ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                uid,
                pid,
                title,
                state.as_str(),
                brief.unwrap_or_default(),
                f.size,
                f.model,
                scope
            ],
        )
        .map_err(err)?;
        let id = tx.last_insert_rowid();
        for a in after {
            let dst = uid_id(&tx, a)?;
            add_after(&tx, id, dst)?;
        }
        tx.execute(
            "INSERT INTO event(chunk_uid, at, by, to_state) VALUES (?1, ?2, 'planner', ?3)",
            params![uid, now, state.as_str()],
        )
        .map_err(err)?;
        tx.commit().map_err(err)?;
        Ok(uid)
    }

    /// `fael chunk edit`: fields only, never the state. Any change clears the approval
    /// (SPEC §8), so the queue never starts work the owner did not approve as it now reads.
    pub fn edit(&mut self, uid: &str, f: &Fields, now: &str) -> Result<(), String> {
        let tx = tx(self)?;
        let r = row(&tx, uid)?;
        if r.state.terminal() {
            return Err(format!(
                "rejected: chunk {uid} is {} — history, not edited",
                r.state.as_str()
            ));
        }
        let scope = f.scope.as_deref().map(scope).transpose()?;
        let mut what = Vec::new();
        for (col, v) in [
            ("title", f.title.as_deref().map(str::trim)),
            ("brief", f.brief.as_deref().map(str::trim)),
            ("size", f.size.as_deref()),
            ("model_hint", f.model.as_deref()),
            ("scope", scope.as_ref().map(|s| s.as_deref().unwrap_or(""))),
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
                "rejected: nothing to edit — --title, --brief, --size, --model or --scope".into(),
            );
        }
        let n = tx
            .execute(
                "UPDATE chunk SET approved_at = NULL, approved_model = NULL, approved_client = NULL
                 WHERE id = ?1 AND approved_at IS NOT NULL",
                [r.id],
            )
            .map_err(err)?;
        let mut text = format!("edited {}", what.join(", "));
        if n > 0 {
            text.push_str(" · approval cleared");
        }
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
