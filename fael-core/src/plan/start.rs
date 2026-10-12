//! `fael chunk start <uid> [--run R] [--client C] [--force]` (SPEC §1 run rules, §4 pair):
//! claim the chunk and every chunk paired with it in one `BEGIN IMMEDIATE` — all or none —
//! insert one run row each under the same R, and hand back what the brief prints.

use super::State;
use super::agent::unwait;
use super::chunk::{Here, end_live, row, set, tx};
use super::lookup::lookup;
use super::ready::{Ready, ready_in};
use super::section::{self, Row as Section};
use super::store::{Store, err};
use rusqlite::{OptionalExtension, Transaction, params};

#[derive(Debug, Default, Clone)]
pub struct Start {
    /// R: the launcher passes a fresh one, else fael makes it; a reused R is rejected
    pub run: Option<String>,
    pub client: Option<String>,
    /// what the launcher runs it with → `run.model`
    pub model: Option<String>,
    /// a client that writes a file and exits (image gen): the run holds no worktree, so it
    /// takes no slot, and `done --run R` from the launching shell finishes it
    pub no_worktree: bool,
    /// take over a live run (its end names who took it), never a kill
    pub force: bool,
    /// the calling agent's session (`CLAUDE_CODE_SESSION_ID`) on a start without `--run`:
    /// kept for `--resume` even when `done` ends the run before any Stop
    pub session: Option<String>,
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
    pub client_hint: Option<String>,
    pub scope: Option<String>,
    /// `app/name` or `name`
    pub plan: String,
    pub plan_title: String,
    /// the md that keeps Goal, Scope, Done and Constraints after cutover (`""` = none)
    pub source: String,
    /// the plan's sections in order (none yet = `source` is read instead)
    pub sections: Vec<Section>,
    /// `(ref, section)` per `--reads` line, `None` = no such section
    pub reads: Vec<(String, Option<Section>)>,
    /// the checkout the code lives in when it is not this repo (`fael plan repo`)
    pub repo: Option<String>,
    /// this db's repo root, where `fael chunk` runs when `repo` is elsewhere; the CLI sets it
    pub root: Option<String>,
    pub refs: Vec<String>,
    /// the owner's answers since this chunk last started (SPEC §1 "owner said")
    pub said: Vec<String>,
    /// `fael chunk note` texts since this chunk last started
    pub notes: Vec<String>,
    /// `(title, handoff)`: the done / review word of each chunk this one is after
    pub after: Vec<(String, String)>,
    /// the run this start took over or restarted — where the last agent stopped
    pub resume: Option<Prev>,
    /// the ledger issue ids it fixes (`add --issue`); the CLI swaps each for its row's text
    pub issues: Vec<String>,
}

/// The last run of a chunk a start takes over: what a resume needs to find its work.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prev {
    pub start: String,
    pub client: Option<String>,
    pub session: Option<String>,
    pub worktree: Option<String>,
    pub branch: Option<String>,
    pub started: String,
    pub last_seen: Option<String>,
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

fn prev(tx: &Transaction, id: i64) -> Result<Option<Prev>, String> {
    tx.query_row(
        "SELECT start, client, session, worktree, branch, started, last_seen
         FROM run WHERE chunk = ?1 ORDER BY id DESC LIMIT 1",
        [id],
        |r| {
            Ok(Prev {
                start: r.get(0)?,
                client: r.get(1)?,
                session: r.get(2)?,
                worktree: r.get(3)?,
                branch: r.get(4)?,
                started: r.get(5)?,
                last_seen: r.get(6)?,
            })
        },
    )
    .optional()
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
        let wt = (!o.no_worktree).then_some(here.worktree);
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
            let pair = if m == first.uid {
                String::new()
            } else {
                format!(" (paired with {uid})")
            };
            let mut was = None;
            if r.state == State::Running {
                was = prev(&tx, r.id)?;
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
                            wt.unwrap_or("no worktree")
                        )
                    }
                    None => format!("started again by run {run}: the last run ended"),
                };
                set(&tx, &r, State::Running, "agent", Some(&text), here.now)?;
            } else {
                // a chunk whose work shipped before its after was met still needs a run
                // to reach done: `--force` starts it, and the event says what it skipped
                let text = match ready_in(&tx, r.id, today)? {
                    Ready::Yes => None,
                    Ready::Blocked(u) => {
                        let w: Vec<_> = u.into_iter().map(|u| u.uid.unwrap_or(u.what)).collect();
                        if !o.force {
                            return Err(format!(
                                "rejected: chunk {m}{pair} is blocked — after {} first, or `--force` starts it anyway",
                                w.join(", ")
                            ));
                        }
                        Some(format!("started by --force past after {}", w.join(", ")))
                    }
                    Ready::No(why) => {
                        return Err(format!("rejected: chunk {m}{pair} is not ready: {why}"));
                    }
                };
                set(&tx, &r, State::Running, "agent", text.as_deref(), here.now)?;
            }
            claim.push((r, was));
        }
        if let Some(b) = wt.map(|w| busy(&tx, w)).transpose()?.flatten() {
            return Err(format!(
                "rejected: {} holds run {b} — one start per worktree: `fael run end {b}` first, or another worktree",
                here.worktree
            ));
        }
        let mut chunks = Vec::new();
        for (r, was) in claim {
            tx.execute(
                "INSERT INTO run(start, chunk, client, model, worktree, branch, started, session)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    run,
                    r.id,
                    o.client,
                    o.model,
                    wt,
                    wt.and(here.branch),
                    here.now,
                    o.session
                ],
            )
            .map_err(err)?;
            unwait(&tx, r.id)?;
            let mut b = brief(&tx, r.id, 1)?;
            b.resume = was;
            chunks.push(b);
        }
        tx.commit().map_err(err)?;
        Ok(Started { run, chunks })
    }
}

/// The live start R in a worktree, if any.
fn busy(tx: &Transaction, worktree: &str) -> Result<Option<String>, String> {
    tx.query_row(
        "SELECT start FROM run WHERE worktree = ?1 AND ended IS NULL LIMIT 1",
        [worktree],
        |r| r.get(0),
    )
    .optional()
    .map_err(err)
}

impl Store {
    /// `fael chunk show <uid|label>`: the brief, owner answers and notes the next start
    /// prints — for a running chunk, the ones its start printed. Read-only, md plans too.
    pub fn show(&self, key: &str) -> Result<Brief, String> {
        let tx = self.conn.unchecked_transaction().map_err(err)?;
        let (id, _, state, ..) = lookup(&tx, key)?.ok_or(format!(
            "rejected: no chunk {key} — `fael board` lists every uid"
        ))?;
        brief(&tx, id, u32::from(state == "running"))
    }
}

/// `skip` = how many starts back the answers and notes count from: 1 inside a start (its
/// own event is the newest), 0 for the start still to come.
fn brief(tx: &Transaction, id: i64, skip: u32) -> Result<Brief, String> {
    let (mut b, pid, reads) = tx
        .query_row(
            "SELECT c.uid, c.label, c.title, c.brief, c.size, c.model_hint, c.scope,
                    CASE p.app WHEN '' THEN p.name ELSE p.app || '/' || p.name END,
                    p.title, p.source, COALESCE(p.refs, ''), c.client_hint, p.repo, p.id,
                    COALESCE(c.reads, '')
             FROM chunk c JOIN plan p ON p.id = c.plan WHERE c.id = ?1",
            [id],
            |r| {
                let b = Brief {
                    uid: r.get(0)?,
                    label: r.get(1)?,
                    title: r.get(2)?,
                    brief: r.get(3)?,
                    size: r.get(4)?,
                    model_hint: r.get(5)?,
                    client_hint: r.get(11)?,
                    scope: r.get(6)?,
                    plan: r.get(7)?,
                    plan_title: r.get(8)?,
                    source: r.get(9)?,
                    repo: r.get(12)?,
                    root: None,
                    sections: vec![],
                    reads: vec![],
                    refs: r
                        .get::<_, String>(10)?
                        .lines()
                        .map(str::to_string)
                        .collect(),
                    said: vec![],
                    notes: vec![],
                    after: vec![],
                    resume: None,
                    issues: vec![],
                };
                Ok((b, r.get::<_, i64>(13)?, r.get::<_, String>(14)?))
            },
        )
        .map_err(err)?;
    b.sections = section::list(tx, pid)?;
    b.reads = reads
        .lines()
        .map(|r| Ok((r.to_string(), section::read(tx, pid, r)?)))
        .collect::<Result<_, String>>()?;
    // answers and notes after the start before this one (this start's own event is the
    // newest; a note on a running chunk is not a start)
    let since = |who: &str| {
        format!(
            "SELECT text FROM event
             WHERE chunk_uid = ?1 AND {who} AND text IS NOT NULL
               AND id > COALESCE((SELECT id FROM event WHERE chunk_uid = ?1
                                    AND to_state = 'running' AND by != 'note'
                                  ORDER BY id DESC LIMIT 1 OFFSET {skip}), 0)
             ORDER BY id"
        )
    };
    b.said = texts(
        tx,
        &since("by = 'owner' AND to_state = 'open' AND from_state IN ('waiting', 'review')"),
        &b.uid,
    )?;
    b.notes = texts(tx, &since("by = 'note'"), &b.uid)?;
    b.issues = super::issue::of(tx, id)?;
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
