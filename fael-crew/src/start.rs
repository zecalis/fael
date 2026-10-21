//! `fael chunk start <uid> [--run R] [--client C] [--force]` (SPEC §1 run rules, §4 pair):
//! claim the chunk and every chunk paired with it in one `BEGIN IMMEDIATE` — all or none —
//! insert one run row each under the same R, and hand back what the brief prints.
//! One worktree is one seat: a live run there is an agent that left, so a start ends it
//! (`--force` is only for a live run in another worktree).

use super::State;
use super::agent::unwait;
use super::chunk::{Here, end_live, row, set, tx};
use super::holder::{
    clashes, end_in_worktree, holder, holder_detail, holder_worktree, members, ready_paired,
};
use super::lookup::{lookup, no_chunk};
use super::ready::Ready;
use super::section::{self, Row as Section};
use super::store::{Store, err};
use rusqlite::{OptionalExtension, Transaction, params};

#[derive(Debug, Default, Clone)]
pub struct Start {
    /// R: the launcher passes a fresh one, else fael makes it; a reused R is rejected
    pub run: Option<String>,
    /// → `run.client`; None = the approval's client, else the chunk's `client_hint`
    pub client: Option<String>,
    /// what the launcher runs it with → `run.model`; None = the approval's, else `model_hint`
    pub model: Option<String>,
    /// a client that writes a file and exits (image gen): the run holds no worktree, so it
    /// takes no slot, and `done --run R` from the launching shell finishes it
    pub no_worktree: bool,
    /// take over a live run in another worktree (its end names who took it), never a kill;
    /// a live run in the caller's own worktree is taken over without it
    pub force: bool,
    /// the calling agent's session (`CLAUDE_CODE_SESSION_ID`) on a start without `--run`:
    /// kept for `--resume` even when `done` ends the run before any Stop
    pub session: Option<String>,
    /// HEAD's sha in the worktree at start → `run.base`, where `done`'s diff begins
    pub base: Option<String>,
    /// `.fael/shared` paths: nobody's claim, never an overlap (as on the board)
    pub shared: Vec<String>,
}

/// One claimed chunk, with what its brief needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Brief {
    pub uid: String,
    pub label: Option<String>,
    pub title: String,
    pub brief: String,
    pub size: Option<String>,
    /// the model this start runs: the run row's, else the approval's, else the hint
    pub model: Option<String>,
    pub scope: Option<String>,
    /// `app/name` or `name`
    pub plan: String,
    pub plan_title: String,
    /// the plan's sections in order
    pub sections: Vec<Section>,
    /// `(ref, section)` per `--reads` line, `None` = no such section
    pub reads: Vec<(String, Option<Section>)>,
    /// the owner's answers since this chunk last started (SPEC §1 "owner said")
    pub said: Vec<String>,
    /// `fael chunk note` texts since this chunk last started
    pub notes: Vec<String>,
    /// `(title, handoff, touched)`: the done / review word of each chunk this one is after,
    /// and the files its done claimed (one path per line, empty when none)
    pub after: Vec<(String, String, String)>,
    /// the same for done chunks that touched files under this scope since the brief was written
    /// (newest first, capped), and how many more there were
    pub by_file: Vec<(String, String, String)>,
    pub by_file_more: usize,
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

/// A reused R is rejected: one R per start.
fn check_fresh(tx: &Transaction, run: &str) -> Result<(), String> {
    let reused: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM run WHERE start = ?1)",
            [run],
            |r| r.get(0),
        )
        .map_err(err)?;
    if reused {
        return Err(format!(
            "rejected: run {run} was already used — one R per start"
        ));
    }
    Ok(())
}

/// A retry from the session holding the chunk: the live brief again, not a
/// takeover. `None` = another session holds it (or none does) — the caller
/// runs the usual held/blocked checks.
fn reuse_live(
    tx: &Transaction,
    first: &super::chunk::Row,
    ms: &[String],
    session: Option<&str>,
) -> Result<Option<Started>, String> {
    let Some(session) = session else {
        return Ok(None);
    };
    if first.state != State::Running {
        return Ok(None);
    }
    let Some((live, held)) = holder_detail(tx, first.id)? else {
        return Ok(None);
    };
    if held.as_deref() != Some(session) {
        return Ok(None);
    }
    let same = ms.iter().all(|m| {
        row(tx, m).is_ok_and(|r| {
            r.state != State::Running
                || tx
                    .query_row(
                        "SELECT start FROM run WHERE chunk = ?1 AND ended IS NULL ORDER BY id DESC LIMIT 1",
                        [r.id],
                        |q| q.get::<_, String>(0),
                    )
                    .is_ok_and(|s| s == live)
        })
    });
    if !same {
        return Ok(None);
    }
    let mut chunks = Vec::new();
    for m in ms {
        let r = row(tx, m)?;
        let mut b = brief(tx, r.id, 1, Some(&live))?;
        b.resume = prev(tx, r.id)?;
        chunks.push(b);
    }
    Ok(Some(Started { run: live, chunks }))
}

impl Store {
    pub fn start(&mut self, uid: &str, o: &Start, here: &Here) -> Result<Started, String> {
        let run = o.run.clone().unwrap_or_else(fael_core::ulid);
        let today = &here.now[..here.now.len().min(10)];
        let wt = (!o.no_worktree).then_some(here.worktree);
        let tx = tx(self)?;
        let first = row(&tx, uid)?;
        check_fresh(&tx, &run)?;
        let mut claim = Vec::new();
        let ms = members(&tx, first.id)?;
        // a retry from the session that already holds it is success, not a
        // takeover: the brief header names the live R, so re-running start in
        // the same session must print that brief, never demand `--force`.
        if let Some(reused) = reuse_live(&tx, &first, &ms, o.session.as_deref())? {
            tx.commit().map_err(err)?;
            return Ok(reused);
        }
        let clash = clashes(&tx, &ms, &o.shared)?;
        if !clash.is_empty() && !o.force {
            return Err(format!(
                "rejected: chunk {uid} shares files with a running chunk — {}; `--force` starts it anyway",
                clash.join("; ")
            ));
        }
        for m in ms.iter().cloned() {
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
                // the same worktree is the same seat: the agent that held it is gone
                let seat = wt.is_some() && holder_worktree(&tx, r.id)?.as_deref() == wt;
                let text = match holder(&tx, r.id)? {
                    Some(h) if !o.force && !seat => {
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
                let text = match ready_paired(&tx, r.id, today, &ms)? {
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
        // one start per worktree: a run of another chunk still live here is an agent that left
        if let Some(w) = wt {
            end_in_worktree(&tx, w, here.now)?;
        }
        let mut chunks = Vec::new();
        for (r, was) in claim {
            // issue 01M4XN3S: no --client/--model = the approval's, else the chunk's hint
            tx.execute(
                "INSERT INTO run(start, chunk, client, model, worktree, branch, started, session, base)
                 SELECT ?1, id, COALESCE(?3, approved_client, client_hint),
                        COALESCE(?4, approved_model, model_hint), ?5, ?6, ?7, ?8, ?9
                 FROM chunk WHERE id = ?2",
                params![
                    run,
                    r.id,
                    o.client,
                    o.model,
                    wt,
                    wt.and(here.branch),
                    here.now,
                    o.session,
                    wt.and(o.base.as_deref())
                ],
            )
            .map_err(err)?;
            unwait(&tx, r.id)?;
            let mut b = brief(&tx, r.id, 1, Some(&run))?;
            b.resume = was;
            chunks.push(b);
        }
        tx.commit().map_err(err)?;
        Ok(Started { run, chunks })
    }
}

impl Store {
    /// `fael chunk show <uid|label>`: the brief, owner answers and notes the next start
    /// prints — for a running chunk, the ones its start printed. Read-only, md plans too.
    pub fn show(&self, key: &str) -> Result<Brief, String> {
        let tx = self.conn.unchecked_transaction().map_err(err)?;
        let (id, _, state, ..) = lookup(&tx, key)?.ok_or_else(|| no_chunk(key))?;
        brief(&tx, id, u32::from(state == "running"), None)
    }
}

/// `skip` = how many starts back the answers and notes count from: 1 inside a start (its
/// own event is the newest), 0 for the start still to come. `start` = this start's R:
/// the brief names the model it runs; None = the next start's (latest run, approval, hint).
fn brief(tx: &Transaction, id: i64, skip: u32, start: Option<&str>) -> Result<Brief, String> {
    let (mut b, pid, reads) = tx
        .query_row(
            "SELECT c.uid, c.label, c.title, c.brief, c.size, c.model_hint, c.scope,
                    CASE p.app WHEN '' THEN p.name ELSE p.app || '/' || p.name END,
                    p.title, p.id, COALESCE(c.reads, '')
             FROM chunk c JOIN plan p ON p.id = c.plan WHERE c.id = ?1",
            [id],
            |r| {
                let b = Brief {
                    uid: r.get(0)?,
                    label: r.get(1)?,
                    title: r.get(2)?,
                    brief: r.get(3)?,
                    size: r.get(4)?,
                    model: r.get(5)?,
                    scope: r.get(6)?,
                    plan: r.get(7)?,
                    plan_title: r.get(8)?,
                    sections: vec![],
                    reads: vec![],
                    said: vec![],
                    notes: vec![],
                    after: vec![],
                    by_file: vec![],
                    by_file_more: 0,
                    resume: None,
                    issues: vec![],
                };
                Ok((b, r.get::<_, i64>(9)?, r.get::<_, String>(10)?))
            },
        )
        .map_err(err)?;
    // the model the agent runs: this start's run row, else the latest run's, the
    // approval's, the hint — a `--model` override or approval never prints stale
    b.model = match start {
        Some(r) => tx
            .query_row(
                "SELECT model FROM run WHERE start = ?1 AND chunk = ?2",
                rusqlite::params![r, id],
                |row| row.get(0),
            )
            .map_err(err)?,
        None => tx
            .query_row(
                "SELECT COALESCE((SELECT model FROM run WHERE chunk = ?1 ORDER BY id DESC LIMIT 1),
                                approved_model, model_hint)
                 FROM chunk WHERE id = ?1",
                [id],
                |row| row.get(0),
            )
            .map_err(err)?,
    };
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
    b.after = super::handoff::after(tx, id)?;
    (b.by_file, b.by_file_more) = super::handoff::by_file(tx, id, b.scope.as_deref())?;
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
