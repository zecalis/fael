//! `fael chunk start <uid> [--run R] [--client C] [--force]` (SPEC §1 run rules, §4 pair):
//! claim the chunk and every chunk paired with it in one `BEGIN IMMEDIATE` — all or none —
//! insert one run row each under the same R, and hand back what the brief prints.

use super::State;
use super::chunk::{Here, end_live, row, set, tx};
use super::ready::{Ready, ready_in};
use super::store::{Store, err};
use rusqlite::{OptionalExtension, Transaction, params};

#[derive(Debug, Default, Clone)]
pub struct Start {
    /// R: the launcher passes a fresh one, else fael makes it; a reused R is rejected
    pub run: Option<String>,
    pub client: Option<String>,
    /// take over a live run (its end names who took it), never a kill
    pub force: bool,
}

/// One claimed chunk, with what its brief needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Brief {
    pub uid: String,
    pub label: Option<String>,
    pub title: String,
    pub brief: String,
    pub size: Option<String>,
    pub model_hint: Option<String>,
    pub scope: Option<String>,
    /// `app/name` or `name`
    pub plan: String,
    pub plan_title: String,
    /// the md that keeps Goal, Scope, Done and Constraints after cutover (`""` = none)
    pub source: String,
    pub refs: Vec<String>,
    /// the owner's answers since this chunk last started (SPEC §1 "owner said")
    pub said: Vec<String>,
    /// `fael chunk note` texts since this chunk last started
    pub notes: Vec<String>,
    /// `(title, handoff)`: the done / review word of each chunk this one is after
    pub after: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Started {
    pub run: String,
    pub chunks: Vec<Brief>,
}

/// The chunk and every chunk a `pair` edge joins to it, in plan order.
fn members(tx: &Transaction, id: i64) -> Result<Vec<String>, String> {
    let mut q = tx
        .prepare(
            "SELECT c.uid FROM chunk c WHERE c.id = ?1 OR c.id IN (
               SELECT dst FROM edge WHERE src = ?1 AND kind = 'pair'
               UNION SELECT src FROM edge WHERE dst = ?1 AND kind = 'pair')
             ORDER BY c.plan, c.seq",
        )
        .map_err(err)?;
    q.query_map([id], |r| r.get(0))
        .map_err(err)?
        .collect::<Result<_, _>>()
        .map_err(err)
}

/// Who holds a running chunk now: its live run, if any.
fn holder(tx: &Transaction, id: i64) -> Result<Option<String>, String> {
    tx.query_row(
        "SELECT start || ' (' || COALESCE(client, 'agent') || ') in ' || COALESCE(worktree, '?')
                || COALESCE(' on ' || branch, '') || ' since ' || started
         FROM run WHERE chunk = ?1 AND ended IS NULL ORDER BY id DESC LIMIT 1",
        [id],
        |r| r.get(0),
    )
    .optional()
    .map_err(err)
}

impl Store {
    pub fn start(&mut self, uid: &str, o: &Start, here: &Here) -> Result<Started, String> {
        let run = o.run.clone().unwrap_or_else(crate::ulid);
        let today = &here.now[..here.now.len().min(10)];
        let tx = tx(self)?;
        let first = row(&tx, uid)?;
        let reused: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM run WHERE start = ?1)",
                [&run],
                |r| r.get(0),
            )
            .map_err(err)?;
        if reused {
            return Err(format!(
                "rejected: run {run} was already used — one R per start"
            ));
        }
        let mut claim = Vec::new();
        for m in members(&tx, first.id)? {
            let r = row(&tx, &m)?;
            let pair = if m == uid {
                String::new()
            } else {
                format!(" (paired with {uid})")
            };
            if r.state == State::Running {
                // a running chunk whose run ended (the agent exited) starts again as is
                let text = match holder(&tx, r.id)? {
                    Some(h) if !o.force => {
                        return Err(format!(
                            "rejected: chunk {m}{pair} is held by run {h} — `--force` takes it over"
                        ));
                    }
                    Some(h) => {
                        end_live(&tx, r.id, here.now)?;
                        let who = o.client.as_deref().unwrap_or("agent");
                        format!(
                            "taken over from run {h} by run {run} ({who}) in {}",
                            here.worktree
                        )
                    }
                    None => format!("started again by run {run}: the last run ended"),
                };
                set(&tx, &r, State::Running, "agent", Some(&text), here.now)?;
            } else {
                match ready_in(&tx, r.id, today)? {
                    Ready::Yes => {}
                    Ready::Blocked(u) => {
                        let w: Vec<_> = u.into_iter().map(|u| u.uid.unwrap_or(u.what)).collect();
                        return Err(format!(
                            "rejected: chunk {m}{pair} is blocked — after {} first",
                            w.join(", ")
                        ));
                    }
                    Ready::No(why) => {
                        return Err(format!("rejected: chunk {m}{pair} is not ready: {why}"));
                    }
                }
                set(&tx, &r, State::Running, "agent", None, here.now)?;
            }
            claim.push(r);
        }
        let busy: Option<String> = tx
            .query_row(
                "SELECT start FROM run WHERE worktree = ?1 AND ended IS NULL LIMIT 1",
                [here.worktree],
                |r| r.get(0),
            )
            .optional()
            .map_err(err)?;
        if let Some(b) = busy {
            return Err(format!(
                "rejected: {} holds run {b} — one start per worktree: `fael run end {b}` first, or another worktree",
                here.worktree
            ));
        }
        let mut chunks = Vec::new();
        for r in &claim {
            tx.execute(
                "INSERT INTO run(start, chunk, client, worktree, branch, started)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![run, r.id, o.client, here.worktree, here.branch, here.now],
            )
            .map_err(err)?;
            tx.execute(
                "UPDATE chunk SET wait_on = NULL, wait_text = NULL, wait_until = NULL WHERE id = ?1",
                [r.id],
            )
            .map_err(err)?;
            chunks.push(brief(&tx, r.id)?);
        }
        tx.commit().map_err(err)?;
        Ok(Started { run, chunks })
    }
}

fn brief(tx: &Transaction, id: i64) -> Result<Brief, String> {
    let mut b = tx
        .query_row(
            "SELECT c.uid, c.label, c.title, c.brief, c.size, c.model_hint, c.scope,
                    CASE p.app WHEN '' THEN p.name ELSE p.app || '/' || p.name END,
                    p.title, p.source, COALESCE(p.refs, '')
             FROM chunk c JOIN plan p ON p.id = c.plan WHERE c.id = ?1",
            [id],
            |r| {
                Ok(Brief {
                    uid: r.get(0)?,
                    label: r.get(1)?,
                    title: r.get(2)?,
                    brief: r.get(3)?,
                    size: r.get(4)?,
                    model_hint: r.get(5)?,
                    scope: r.get(6)?,
                    plan: r.get(7)?,
                    plan_title: r.get(8)?,
                    source: r.get(9)?,
                    refs: r
                        .get::<_, String>(10)?
                        .lines()
                        .map(str::to_string)
                        .collect(),
                    said: vec![],
                    notes: vec![],
                    after: vec![],
                })
            },
        )
        .map_err(err)?;
    // answers and notes after the start before this one (this start's own event is the
    // newest; a note on a running chunk is not a start)
    let since = |who: &str| {
        format!(
            "SELECT text FROM event
             WHERE chunk_uid = ?1 AND {who} AND text IS NOT NULL
               AND id > COALESCE((SELECT id FROM event WHERE chunk_uid = ?1
                                    AND to_state = 'running' AND by != 'note'
                                  ORDER BY id DESC LIMIT 1 OFFSET 1), 0)
             ORDER BY id"
        )
    };
    b.said = texts(
        tx,
        &since("by = 'owner' AND to_state = 'open' AND from_state IN ('waiting', 'review')"),
        &b.uid,
    )?;
    b.notes = texts(tx, &since("by = 'note'"), &b.uid)?;
    let mut q = tx
        .prepare(
            "SELECT c.title, e.text FROM edge g
             JOIN chunk c ON c.id = g.dst
             JOIN event e ON e.id = (SELECT MAX(id) FROM event WHERE chunk_uid = c.uid
                                       AND by = 'agent' AND to_state IN ('done', 'review'))
             WHERE g.src = ?1 AND g.kind = 'after' AND e.text != ''
             ORDER BY c.seq",
        )
        .map_err(err)?;
    b.after = q
        .query_map([id], |r| Ok((r.get(0)?, r.get(1)?)))
        .map_err(err)?
        .collect::<Result<_, _>>()
        .map_err(err)?;
    Ok(b)
}

fn texts(tx: &Transaction, sql: &str, uid: &str) -> Result<Vec<String>, String> {
    tx.prepare(sql)
        .map_err(err)?
        .query_map([uid], |r| r.get(0))
        .map_err(err)?
        .collect::<Result<_, _>>()
        .map_err(err)
}

#[cfg(test)]
#[path = "start_tests.rs"]
mod tests;
